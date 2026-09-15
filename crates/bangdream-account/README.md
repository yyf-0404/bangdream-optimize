# bangdream-account

国服账号密码导入与用户资料解析。网页和桌面端共用 `CredentialImporter`；旧 `BangDreamAccountImporter` 保留作兼容参考，新入口不读取 `persist.json`。

## 使用流程

顶部档案旁的「导入」，或档案页的「导入配置」→ 国服 → 账号密码 → 填写 Bilibili 账号、密码并明确选择 bili安卓 / iOS。读取成功后显示游戏昵称、玩家 ID、等级和渠道，核对变化后才应用。也可选择新建档案。国服的 Bestdori 主乐队公开资料入口仍保留，其他服沿用公开资料导入。

完整导入替换游戏卡牌、区域道具、角色潜能及任务加成；保留目标档案的自定义卡牌、活动配置和计算参数。特训状态与显示特训前后图片分别导入，剧情阅读状态优先用实际 episode ID，缺少卡牌映射时沿用追加综合力推断。

## 登录实现

依据 `login_once_portable_20260914.zip` 的协议：

1. 优先读取本地固定版本配置；文件不存在时使用随程序附带的 `default-client-version.json`。当前默认是客户端 `9.4.4`、版本编号 `105`、Unity `2022.3.62f3c1`。正常导入不请求官方 APK 或下载配置。
2. `/application` 校验固定客户端参数，并取得本次数据和主数据版本。配置格式异常或版本预检失败时，才从官方 `bangdream.config.js` 与 HTTPS APK 的 HTTP Range 元数据重新获取客户端参数，再预检一次。验证成功后原子写回本地默认配置，重启后继续使用；失败时保留原配置并停止。官方请求头遵循参考包，简化 UA 会被 CDN 拒绝。分段不可用时尝试另一官方 CDN，然后报错，不下载完整安装包。
3. Bilibili SDK cipher → RSA PKCS#1 v1.5 加密密码 → SDK login，按排序后的未编码参数值生成签名。
4. 仅一次游戏登录；使用响应内的游戏玩家 ID、token 和 nonce，计算紧接着那一次自身 `/suite/user/{id}` 请求的 RID。版本回退在 SDK 登录之前完成；账号密码、验证码或游戏登录失败不触发再次登录。禁止登录自动重试、跟随重定向、传入任意目标玩家 ID，拒绝并发导入以免共享设备会话互相覆盖。
5. 核对 suite 中两处玩家 ID 与登录返回值一致，再调用现有卡牌、道具、角色解析器。

bili安卓 / iOS 由用户选择，不能根据当前电脑系统推断。iOS 游戏头与 Android 游戏头不同，但两个渠道的 SDK 参数和登录体 platform 均遵循参考包的 Android 组合。

密码、SDK access_key、游戏 token 和 RID 不进入日志、档案或磁盘缓存。HTTP 返回 `Cache-Control: no-store`；请求类型不实现 Debug / Serialize；只返回已解析的养成资料与游戏身份。验证码或风控要求由官方客户端处理，接口不尝试绕过。取消窗口会丢弃在途结果，桌面原生网络调用可在后台结束，但不会自动写入档案。

## 默认版本配置

可维护的内置默认值位于 `default-client-version.json`，仅包含 `clientVersion`、`versionCode`、`unityVersion` 三个公开字段。数据版本与主数据版本仍由每次 `/application` 返回。

自动更新的是运行环境的默认配置文件（只写入这三个公开字段，不包含账号密码或会话）：

- 服务端默认：工作目录下 `var/bangdream-account/client-version.json`；可用 `BANGDREAM_OPTIMIZE_CN_VERSION_CONFIG` 指定其他可写路径。
- 桌面端：用户数据目录下 `bangdream-account/client-version.json`。

此文件优先于内置配置，版本回退成功后自动生成或覆盖，重启后生效；不修改已编译程序或仓库里的内置 JSON。目录必须允许服务进程写入；写入失败会在使用账号凭证之前报错。失败的发现或预检不覆盖已有配置。APK 只在版本预检失败时读取，不设置定时刷新。

## 部署

网页新增 `POST /api/import/cn-account`，JSON body 为 `{account, password, channel}`，channel 必填 `android` 或 `ios`。需同时更新 Rust 服务端与网页，并将 `docs/nginx-reverse-proxy.conf` 中此路径的 location 加入当前生效的 **HTTPS server** 块，避免 SPA fallback 返回 HTML。建议 240 秒读取超时、8 KiB 请求大小、关闭请求落盘缓冲和响应缓存；不要在日志格式中加入 `$request_body`。

桌面端使用原生 `import_cn_account` Tauri command，从本机连接官方服务，无需网站中转。网页端通过配置的 API 服务中转，除本机开发地址外只允许 HTTPS。

本地：启动 Rust 服务（默认 127.0.0.1:3100），再运行 `python scripts/serve-web.py --port 8093`。开发预览仅将这一固定账号 POST 路径转发到本机 3100，可用 `--api-port` 改后端端口。

## 验证

- `cargo test -p bangdream-optimize-bangdream-account -j 1`：离线模拟固定版本优先、预检失败回退、写回及重启复用、失败保留旧配置，以及签名、RSA、两渠道、一次登录、身份错误、缺失 token、SDK 拒绝及 APK 二进制清单解析。
- `cargo test -p bangdream-optimize-server -j 1 cn_account`：路由、无效/超大请求、不回显密码、不缓存。
- `node --test apps/web/test/*.test.js`：网页传输及档案合并等回归测试。
- `cargo test -p bangdream-optimize-bangdream-account -j 1 public_version_preflight -- --ignored --nocapture`：显式联网，只读取公开 APK 元数据及两渠道 `/application`，不执行账号登录。

真实账号登录需要用户在界面中填写账号。请勿把密码提交到仓库或测试夹具。
