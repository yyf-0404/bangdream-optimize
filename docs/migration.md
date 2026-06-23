# 迁移计划

## 当前目标

首个 Rust 阶段并不是旧 `/calcResult` 路由的直接替代。
它的目标是建立新的计算边界与统一 JSON API，供 `tsugu-bangdream-bot` 后续适配。

## Crate 边界

- `core`：领域 schema、校验、计算结果组装。
- `solver`：medley 题目的数值优化求解。
- `team-prune`：共享支配图、覆盖与同组剪枝基元。
- `data`：Bestdori 与 game-data 加载的 provider。
- `service`：共享优化编排层，接管玩家存储与计算数据。
- `desktop`：原生桌面应用边界。
- `storage-local`：本地玩家配置 JSON 存储。
- `storage-mongodb`：MongoDB 玩家配置接入。
- `apps/server`：HTTP API 路由与 `/game-data` 静态服务，`web-root` 仅在显式配置时启用。
- `apps/web`：共享静态 UI，供浏览器和桌面使用。
- `apps/desktop`：Tauri 壳，加载共享 `apps/web`。
- `tools/sync-bestdori`：静态 Bestdori 镜像生成。

## 下一步迁移

1. 从现有 TypeScript/C++ 实现产出黄金测试样例。
2. 迁移 `Chart.init`、`getMaxMetaOrder` 与 `getScore` 到 `core`。已完成。
3. 迁移卡牌、技能、活动与 area-item 属性准备逻辑到 `core`。已完成。
4. 迁移队伍枚举与道具组合优化到 `core`。已完成。
5. 将 `data` 接入 Bestdori JSON 与本地修正文件。映射、可复用计算快照构建、文件系统源与静态镜像缓存源已完成。
6. 将 `storage-mongodb` 接入 `server`，提供 `/v1/calc-result`。HTTP 路由、Mongo 玩家存储边界与本地 game-data 注入已完成。
7. 将 C++ AVX2 medley solver 迁移为 `solver::avx2`。已完成确定性 AVX2 路径与标量回退。
8. 直接在 `core` 上构建桌面应用。已完成本地 JSON 玩家配置存储、原生优化边界、共享 Web 运行时适配器与 Tauri 外壳骨架。
9. 用 `core` 重写 Web 应用。已完成基于静态数据的 WASM 边界、浏览器 `/game-data` 同步（含 manifest 刷新）、Rust 镜像生成 CLI，以及面向卡牌、活动曲目、area-item 与角色加成编辑的运行时。

## 已完成核心覆盖

- `chart`：时序 Meta、技能顺序、队长选择、得分计算。
- `solver`：带 AVX2 加速的精确 medley 队伍选择；不支持 AVX2 的环境自动回退标量。
- `team-prune`：共享支配图、同组覆盖、跨组覆盖与同组保留，分别供单曲 DP 与 medley 候选预剪枝使用。
- `core::medley::prune`：medley 签名、硬支配、贡献支配、上界、池构建、全局 trace、trace 结果格式化。
- `core::medley::enumeration`：签名池枚举、incumbent 候选过滤、原始候选追踪、掩码压缩。
- `core::medley::scoring`：精确 medley 队伍候选计分、技能 Meta 缓存、技能排序、队长选择与签名分类。
- `core::medley::seed`：为大规模 medley 输入生成贪心/局部搜索 seed incumbent。
- `preparation`：卡牌预处理、活动加成、area-item 比例、选中道具统计。
- `team`：5 卡队伍枚举（含去重角色过滤）与候选生成。
- `optimization`：队伍/属性/杂志道具搜索与最终 `BuildResult` 选择。
- `data`：Bestdori JSON 映射（卡牌/技能/活动/area-item/谱面）。
- `data::calculation`：可复用快照构建器，将玩家配置、活动数据、谱面与卡片定义转换为 `BuildResult`。
- `data::filesystem`：本地 Bestdori JSON 加载器，支持懒加载活动详情与谱面。
- `data::cache`：静态 `/game-data` 镜像同步到本地缓存目录，再通过文件系统加载器计算。
- `service`：共享 `OptimizerService`，供各端加载玩家配置并触发计算。
- `desktop`：同步本地玩家配置和文件系统/静态镜像 game-data 的原生优化入口，并返回共享 UI 所需引用数据。
- `storage-local`：桌面/原生端的本地玩家配置 JSON 存储。
- `tools/sync-bestdori`：Rust CLI，用于生成 `/game-data` 与 `manifest.json`，支持 ETag 增量同步与完整镜像参数。
- `web-wasm`：浏览器入口，接收静态 Bestdori JSON 并返回 `BuildResult` JSON。
- `apps/web`：数据驱动的静态 UI，支持浏览器与桌面运行时适配。
  - 浏览器模式：IndexedDB + WASM。
  - 桌面模式：Tauri 命令 + 原生 Rust 计算。
- `apps/server`：新增
  `/v1/calc-result/from-candidates`（直接候选队伍计算）与
  `/v1/calc-result`（Mongo 玩家配置计算，需配置 Mongo 与文件系统/静态镜像 game-data）。

## 服务器环境变量

- `BANGDREAM_OPTIMIZE_HOST`：HTTP 绑定主机，默认 `127.0.0.1`。
- `BANGDREAM_OPTIMIZE_PORT`：HTTP 绑定端口，默认 `3100`。
- `BANGDREAM_OPTIMIZE_MONGODB_URI` 或 `MONGODB_URI`：启用 Mongo 玩家配置读取。
- `BANGDREAM_OPTIMIZE_MONGODB_DB` 或 `MONGODB_DB`：Mongo 数据库名，默认 `tsugu-bangdream-bot`。
- `BANGDREAM_OPTIMIZE_WEB_ROOT`：可由该目录服务静态 Web UI。默认本地启动脚本不配置该项，因为 Web 与后端分离端口。
- `BANGDREAM_OPTIMIZE_GAME_DATA_ROOT`：从该目录提供 `/game-data`，并在未显式配置 `BANGDREAM_OPTIMIZE_BESTDORI_ROOT` 时作为文件系统计算源。
- `BANGDREAM_OPTIMIZE_GAME_DATA_BASE_URL`：启用静态镜像同步，例如 `https://example.com/game-data`。
- `BANGDREAM_OPTIMIZE_GAME_DATA_CACHE_ROOT`：静态镜像缓存本地目录。
- `BANGDREAM_OPTIMIZE_BESTDORI_ROOT`：启用本地 Bestdori 文件加载。
- `BANGDREAM_OPTIMIZE_ENABLE_CALC_ROUTES`：设置为 `false` 时仅用于代理类部署，关闭计算路由；默认 `true`。该模式仍保留 `/bestdori/player/...`，默认也保留 `/bangdream/user-data/import`，除非显式设置 `BANGDREAM_OPTIMIZE_ENABLE_BD_IMPORT=false`。
- `BANGDREAM_OPTIMIZE_ENABLE_BD_IMPORT`、`BANGDREAM_OPTIMIZE_BD_PERSIST`、`BANGDREAM_OPTIMIZE_BD_PERSIST_DIR`：国服游戏账号导入默认开启，后端使用固定 persist 登录态，前端只提交 `userId`。默认 persist 路径为 `var/bangdream-account/persist.json`。

默认 `BANGDREAM_OPTIMIZE_BESTDORI_ROOT` 目录结构：

- `api/cards/all.5.json`、`api/characters/main.3.json`、`api/skills/all.10.json`、`api/areaItems/main.5.json`、`api/events/all.6.json`、`api/songs/all.7.json`
- `api/charts/{songId}/{difficultyName}.json`，示例：`api/charts/1/expert.json`
- `api/cards/{cardId}.json`（完整卡片详情），该文件按 `api/cards/all.5.json` 列表生成，仅在计算时读取缺失等级时使用
- 可选：`api/events/{eventId}.json` 的完整活动详情
- 可选修正文件：`cardsCNfix.json`、`skillsCNfix.json`、`areaItemFix.json`、`eventCharacterParameterBonusFix.json`

每个默认路径都可通过以下环境变量覆盖：
`BANGDREAM_OPTIMIZE_BESTDORI_CARDS`、`BANGDREAM_OPTIMIZE_BESTDORI_CHARACTERS`、`BANGDREAM_OPTIMIZE_BESTDORI_SKILLS`、`BANGDREAM_OPTIMIZE_BESTDORI_AREA_ITEMS`、`BANGDREAM_OPTIMIZE_BESTDORI_EVENTS`、`BANGDREAM_OPTIMIZE_BESTDORI_SONGS`、`BANGDREAM_OPTIMIZE_BESTDORI_CHARTS_DIR`、`BANGDREAM_OPTIMIZE_BESTDORI_CARDS_DIR`、`BANGDREAM_OPTIMIZE_BESTDORI_EVENT_DETAILS_DIR`、`BANGDREAM_OPTIMIZE_BESTDORI_CARDS_FIX`、`BANGDREAM_OPTIMIZE_BESTDORI_SKILLS_FIX`、`BANGDREAM_OPTIMIZE_BESTDORI_AREA_ITEMS_FIX`、或 `BANGDREAM_OPTIMIZE_BESTDORI_EVENT_CHARACTER_PARAMETER_BONUS_FIX`。
