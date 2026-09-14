# bangdream-account

保留的国服账号导入模块，供后续新导入功能继续开发。

当前应用未接入此模块：服务端不注册国服专用完整账号导入路由，前端所有服务器统一通过 Bestdori 读取主乐队公开资料。

工作区成员和依赖保留，以便独立检查或重构。旧登录态格式示例保留在 `../../var/bangdream-account/persist.example.json`，仅作为开发参考；真实 `persist.json` 仍由 Git 忽略，不属于当前部署配置。
