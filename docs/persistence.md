# 持久化结构

本文档描述网页端与桌面端的持久化边界。新增字段时应先判断数据归属，再选择对应存储，不要通过扩充核心计算结构来保存前端状态。

## 数据分层

| 数据 | 内容 | 网页端 | 桌面端 | 生命周期 |
| --- | --- | --- | --- | --- |
| 用户配置 | 玩家卡片、区域道具、角色加成、活动与歌曲选择、计算目标和各模式输入参数 | IndexedDB `bangdream-optimize-user-data-v1` | `app_data/user-data` 下的 JSON 文件 | 长期，按配置档案隔离 |
| 结果缓存 | 最近 20 条计算结果、诊断和结果摘要 | IndexedDB `bangdream-optimize-result-cache-v1` | WebView 的同一 IndexedDB | 可删除、可由重新计算恢复 |
| 游戏数据缓存 | Bestdori/镜像的核心数据、谱面、manifest 与修正文件 | IndexedDB `bangdream-optimize-game-data` | `app_data/game-data` 或环境变量指定目录 | 可删除、可重新下载 |
| 临时 UI 状态 | 当前页面、顶部栏折叠、筛选面板和输入校验状态 | 不保存 | 不保存 | 页面会话 |
| 运行时缓存 | 已解析核心数据、卡片搜索索引、计算 Worker、内存结果 | 不落盘 | 不落盘 | 进程会话 |

“清空游戏缓存”会清除游戏数据和依赖该数据的结果缓存，不会删除用户配置。“清空本地缓存”仅在网页端提供，会删除所有用户配置档案；执行前必须等待正在进行的配置保存结束。

## 用户配置

### 数据边界

用户配置是前端拥有的完整 JSON 文档，而不是核心 Rust `PlayerConfig` 的磁盘表示。它同时包含：

- 核心计算输入：`playerId`、`currentEvent`、`eventSongs`、卡片、区域道具和角色加成等；
- 前端上下文：`server`、`calculationMode`、`activityMode`；
- 模式参数：`scoreRange`、`ptMaximize`；
- 活动缓存与自定义活动：`eventPresets`、`eventOverrides`。

桌面端必须按原始 `serde_json::Value` 保存和读取用户配置。只有进入 `calculate_for_config`、`score_range_for_config` 或 `pt_maximize_for_config` 时，才反序列化为核心强类型请求。否则核心 `PlayerConfig` 不认识的前端字段会在一次保存后丢失。

`playerConfigVersion` 是用户配置 schema 版本。当前版本为 1。旧配置没有版本时仍交给统一规范化入口补全；未来发生不能仅靠默认值完成的迁移时，应按该字段逐版本迁移，并在成功写入新格式后再提升版本。

### 默认值和规范化

所有 `scoreRange`、`ptMaximize` 默认值与旧数据补全规则集中在
`apps/web/src/models/player-settings.js`。存储层的示例配置、读取后的规范化和表单恢复不得各自复制默认值。

首次创建配置时固定使用国服和“最大 PT”计算目标。`currentEvent` 不写死活动 ID；核心活动数据加载后，
优先选择当前正在进行且尚未结束的国服活动，存在重叠时取开始时间最晚的一场；没有进行中活动时选择
开始时间最近的下一场。选择结果、活动预设、活动模式和默认歌曲随后立即写入当前档案。若数据中没有
任何带国服结束时间且尚未结束的受支持活动，则保留未选择状态，不回退到已经结束的活动。

最大 PT 的演出模式保存在 `ptMaximize.liveVariantByEventType`，按活动类型分别记忆最后选择。例如 Challenge 的挑战演出不会覆盖 5v5 的团队演出选择。旧的单值 `ptMaximize.liveVariant` 不读取、不迁移，也不会再次写入。

持久化读取流程：

1. 运行时读取当前档案的原始 JSON；
2. `normalizePlayer` 补全 schema、丢弃无效值并生成独立的四名队友参数；
3. 表单只渲染规范化后的值；
4. 下一次写入保存当前 schema 的完整文档。

持久化写入流程：

1. 表单更新内存中的玩家配置；
2. `writePlayer` 再次规范化，并同步当前活动模式与歌曲缓存；
3. 普通输入使用 250 ms 防抖；
4. 所有实际写入通过单一 Promise 队列串行执行；
5. 显式计算、切换档案和破坏性清理会先等待保存完成。

保存失败会反馈到统一错误处理；前一次失败不会阻塞后续保存队列。

### 网页端档案

IndexedDB object store 为 `settings`，主要键如下：

- `player-config-profiles`：档案元数据列表；
- `active-player-config-id`：当前档案 ID；
- `player-config:<id>`：对应的完整用户配置；
- `player-config`：仅用于旧单档案格式迁移。

首次打开多档案版本时，迁移顺序必须是：

1. 尝试读取当前库的旧键 `player-config`；
2. 若不存在，再读取旧库 `bangdream-optimize-user-data`；
3. 把旧配置写入 `default` 档案；
4. 写入档案列表与当前档案 ID；
5. 全部成功后才删除旧键和旧库。

禁止先删除旧库再创建默认档案。

### 桌面端档案

默认目录为 Tauri `app_data/user-data`：

- `user-configs/<id>.json`：完整用户配置 JSON；
- `user-config-profiles.json`：档案元数据；
- `active-user-config.json`：当前档案。

写文件使用同目录临时文件后重命名，避免把半份 JSON 暴露给下一次读取。旧的 `players/<playerId>.json` 与 `active-player.json` 属于已经废弃的按玩家 ID 存储，只保留兼容清理接口，不参与当前前端档案链路。

## 结果缓存

结果缓存与用户档案解耦，但缓存键包含完整的规范化玩家输入、活动和计算模式，所以不同档案或不同模式不会误命中同一结果。

- 存储 schema：`CACHE_SCHEMA_VERSION`；
- 缓存键算法：`RESULT_CACHE_KEY_VERSION`；
- 当前条数上限：20；
- 排序依据：创建/访问时间；
- 游戏数据刷新、全量同步或清空时一并清除。

计算成功但结果缓存写入失败时，仍显示本次结果，并明确提示缓存未保存。用户主动删除或清空结果时，写入失败会回滚内存列表，避免界面状态与磁盘状态不一致。

修改计算语义但结果结构不变时，也必须提升缓存键版本；修改持久化结果结构时提升存储 schema。两者不要混为同一个概念。

桌面端当前复用 WebView IndexedDB 保存结果缓存，没有写入原生用户配置目录。因此清理 WebView 站点数据会丢失历史结果，但不会影响原生用户配置。

## 游戏数据缓存

网页端 `GameDataClient` 负责 `bangdream-optimize-game-data`；桌面端由 `BestdoriCachedFilesystemCalculator` 负责原生目录。用户配置只保存活动/歌曲选择和活动快照，不复制整份游戏数据。

桌面默认数据源选择顺序：

1. 显式文件系统目录；
2. 显式静态镜像 URL；
3. 项目内 `var/game-data`；
4. Bestdori 原始 API。

账号导入 API 地址不参与游戏数据缓存的数据源选择。

## 新增字段检查清单

1. 判断字段属于用户配置、结果缓存、游戏数据还是临时 UI。
2. 用户配置字段加入统一规范化入口和默认值工厂。
3. 确认表单读取和恢复都使用相同字段名、单位和数组长度。
4. 增加旧配置缺字段、非法值和完整往返测试。
5. 桌面端按原始 JSON 验证字段不会经过核心 `PlayerConfig` 丢失。
6. 若字段影响计算结果，将其加入结果缓存键或提升缓存键版本。
7. 若迁移会删除旧数据，必须在新数据完整写入后再删除。

## 已知后续项

- 档案元数据与配置文件目前分别写入；异常中断可能留下孤立配置文件或缺失实体的元数据。后续可在列举档案时增加一致性扫描与恢复。
- 桌面端遇到损坏 JSON 会明确报错，但尚未维护上一版本备份文件。
- 浏览器三套 IndexedDB 目前各自维护少量 Promise 封装；只有在需要统一事务、升级或故障注入测试时再抽成公共模块，避免仅为消除重复增加迁移风险。
