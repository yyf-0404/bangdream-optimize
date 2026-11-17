# bangdream-optimize Web

该目录为静态 web 目标。

运行模型：

- web 服务器、后端或 CDN 可托管静态 game-data 镜像；
- 浏览器会按需刷新 `${gameDataBaseUrl}/manifest.json` 并同步变更文件到 IndexedDB；
- 用户数据保存在 IndexedDB；
- `bangdream-optimize-web-wasm` 在浏览器本地执行计算；
- 桌面壳复用同一套 UI，并切换到 Tauri runtime adapter，调用原生 Rust 命令，而不是浏览器的 IndexedDB/WASM。

Web 启动时读取 `apps/web/config.js`。
默认指向同源 `/game-data`。
本地运行时 `./scripts/run-web.sh` 会服务 `apps/web`，并将 `/game-data` 映射到 `var/game-data`。
后端在 3100 端口同样提供其 `/game-data`，来源于同一镜像。
最高得分计算或需要参考数据时，浏览器会读取配置的镜像。
目标 PT 搜索按需同步 `api/scoreRangeChartMeta.2.json`，并使用界面选择的 `0.5/0.75` Auto
倍率在 Web Worker 中由 WASM 本地计算；
该文件只保存各技能时长下的激活/未激活节点数和尾部风险，不包含服务器可用性。
活动选择器同时列出受支持的单曲与组曲活动，并根据所选活动自动确定歌曲数量。
目标 PT 搜索固定只请求首个全局最优方案。
服务器与浏览器都从 `api/songs/all.7.json` 判断歌曲和难度是否已在目标服务器发布。
桌面端通过 Tauri 原生命令读取同一模板文件。
计算时还会在 `api/cards/all.5.json` 中缺少选中卡片等级时同步
`api/cards/{cardId}.json`。
用户配置存储在 IndexedDB。
JSON 编辑器仅用于直接编辑当前 schema。

计算结果可含可选 `metrics`。
计算成功后，UI 可导出包含当前玩家配置、结果、metrics 和运行时类型的诊断 JSON。
计算执行失败时会进入结果页，并生成包含错误信息与调用栈的失败诊断；若为桌面模式，
诊断还会附带桌面数据/缓存路径与引用数据计数。
诊断仍可手动导出。结果页复制按钮旁的反馈入口会在诊断存在时自动将其作为 JSON
附件加入反馈；侧栏反馈入口不会自动附加诊断。

侧栏反馈表单通过后端 `/api/feedback` 发送邮件。网页端默认使用
`feedbackApiBaseUrl ?? apiBaseUrl`，生产同域部署时两者都可为空；桌面端必须在
`config.desktop.js` 中设置 `feedbackApiBaseUrl`。反馈可上传 `png/jpg/jpeg/gif/webp`、
`txt/log/json/zip/pdf`，最多 3 个，单个不超过 5 MiB、总计不超过 10 MiB。
SMTP 账户和密码只存在于后端环境变量。

## 目录结构

```
apps/web/
  index.html  design.css  config.js     入口页面、样式入口（定义层级）、运行配置
  styleguide/                           样式参照页（开发用，不进入生产构建）
  test/                                 node --test 测试
  src/
    main.js       组合入口：创建状态、各页面视图和动作并接线
    app/          跨页面的状态、生命周期、页面控制器、档案存储，以及计算、配置这类跨页动作
    domain/       业务规则与数据模型（活动、玩家、自定义卡、加成…），不依赖 DOM
    data/         数据读写与格式：游戏数据同步、Bestdori、结果缓存、用户存储、诊断
    runtime/      浏览器（IndexedDB + WASM Worker）与桌面（Tauri）适配
    shared/       通用工具（utils、资源 URL、偏好设置）和 shared/ui 通用控件
    design/       设计系统：tokens、components、motion，以及确认稿 approved/ 与绑定层 fidelity
    components/   多个页面共用的组件：cards/（卡牌展示、筛选、选择）、custom-cards/
    pages/        每个页面一个目录：shell（侧栏、顶栏、头图、反馈）、activity、cards、player、result、archive
    vendor/       第三方代码
```

依赖方向：`pages` 可以用 `components`、`shared`、`design`、`domain`、`data`；`components` 不依赖 `pages`；`domain` 和 `data` 不依赖界面。
页面目录内的文件按作用命名：`*-view.js` 渲染，`*-actions.js` 处理用户操作，`*-layout.js` 把确认稿模板挂到真实控件上。

## 界面开发规范

新界面要和确认稿风格一致，按以下顺序处理。所有例子、变量和动效都在样式参照页里：本地启动后打开 `http://127.0.0.1:8080/styleguide/`。

1. **优先组合基础组件**：`src/design/components.css` 提供按钮、表单字段、分段选择、筛选标签、标签、开关、区块标题、面板、列表、弹窗、提示条、加载和文字，类名前缀 `.ui-`。参照页里每个组件都有可复制的 HTML。按钮主色分两种：`--cta` 只用于"计算"这类启动计算的入口（与侧栏按钮相同），页面内的保存、确认用 `--primary`。如果确认稿已经画过同样的结构（例如结果页的 `.history-*`、`.result-warning`、`.dialog-*`），挂在对应页面容器内并沿用确认稿的类名，不另写一套。
2. **新样式只用设计变量**：颜色、字号、间距、圆角、阴影、时长都写成 `var(--…)`，变量在 `src/design/tokens.css`，取值来自确认稿的实际渲染。`test/design-system.test.js` 会拦下新 CSS 中的固定色值和时长。
3. **放对位置**：通用组件加到 `components.css`，并在参照页加示例（测试会检查）；只属于某个页面的样式放进该页面目录，在 `design.css` 中以 `layer(components)` 引入。
4. **文字加类名**：旧页面的样式会作用到页面里裸的 `p`、`h3` 等标签，组件内文字使用 `.ui-text` 系列。状态用原生属性表达（`disabled`、`aria-pressed`、`aria-selected`、`aria-invalid`、`aria-busy`）。
5. **动效用现成模式**：悬停、按下、焦点反馈已内置在组件里；进入、退出、展开收起、列表增删与重排、数值变化提示、弹窗进出见 `src/design/motion.css` 与 `motion.js`。系统开启"减少动态效果"时，时长变量自动归零。
6. **改旧页面样式先拍快照**：`node scripts/design/ui-snapshot.mjs capture <地址> <目录>` 记录五个页面和参照页的截图与计算样式，改完再拍一次并用 `compare` 对比（需要 Playwright：`npm install --no-save playwright && npx playwright install chromium`）。

`design.css` 的层级从低到高为 tokens → runtime → bindings → approved → plumbing → components。基础组件在最后一层，页面里的旧样式不会改动它们。

## 构建 WASM

安装 `wasm-bindgen-cli`：

```bash
cargo install wasm-bindgen-cli
```

构建浏览器包：

```bash
./scripts/build-web-assets.sh --no-deploy
```

脚本输出到 `apps/web/pkg`。

## 生成游戏数据

生成静态镜像：

```bash
cargo run -p bangdream-optimize-sync-bestdori -- \
  --out var/game-data \
  --repair-dir tsugu-bangdream-bot/backend/config \
  --event 100 \
  --chart 1:expert
```

生成用于定时部署的完整镜像：

```bash
cargo run -p bangdream-optimize-sync-bestdori -- \
  --out var/game-data \
  --repair-dir tsugu-bangdream-bot/backend/config \
  --all-event-details \
  --all-charts \
  --concurrency 8 \
  --retries 2
```

已有完整谱面目录时，可不联网重建目标 PT 模板及 manifest：

```bash
cargo run -p bangdream-optimize-sync-bestdori -- \
  --out var/game-data \
  --generate-score-range-meta-only
```

## 运行服务

服务 `apps/web/` 静态文件。直接打开 `index.html` 不稳定，因浏览器模块、WASM 与 fetch 需要 HTTP 来源。

仅使用本地计算时直接启动 Web：

```bash
./scripts/run-web.sh
```

需要 Bestdori 主乐队资料代理、反馈服务或由后端托管 `/game-data` 时，再启动后端：

```bash
./scripts/run-server.sh
```

然后打开 `http://127.0.0.1:8080`。

此时：

- Web UI：`http://127.0.0.1:8080`
- Web 镜像：`http://127.0.0.1:8080/game-data`
- 后端镜像：`http://127.0.0.1:3100/game-data`

生产环境建议同源配置（推荐通过反向代理）：

```js
globalThis.BANGDREAM_OPTIMIZE_CONFIG = {
  gameDataBaseUrl: '/game-data',
  apiBaseUrl: '',
};
```

仓库默认在 `127.0.0.1:8080` 自动连接 `127.0.0.1:3100`。其他跨端口开发环境可显式配置：

```js
globalThis.BANGDREAM_OPTIMIZE_CONFIG = {
  gameDataBaseUrl: '/game-data',
  apiBaseUrl: 'http://127.0.0.1:3100',
};
```

生产反向代理示例（Nginx）：

```nginx
server {
  server_name example.com;
  root /path/to/apps/web;
  index index.html;

  location /game-data {
    proxy_pass http://127.0.0.1:3100;
  }

  location /bestdori/player {
    proxy_pass http://127.0.0.1:3100;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
  }

  location /bestdori/header/ {
    proxy_pass http://127.0.0.1:3100;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
  }

  location = /api/feedback {
    client_max_body_size 12m;
    proxy_pass http://127.0.0.1:3100;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
  }

  location / {
    try_files $uri $uri/ /index.html;
  }
}
```

也可直接使用 `docs/deployment/nginx-reverse-proxy.conf` 中的可编辑反向代理示例。

上线注意：
- 前端配置为 `gameDataBaseUrl: '/game-data'`，`apiBaseUrl: ''`，确保请求为同源，不写死 `127.0.0.1`。
- 最高得分与目标 PT 搜索均在本地 WASM 中执行，不需要 `/v1` 计算路由。
