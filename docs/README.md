# 文档索引

## 开发与测试

- [环境准备](guides/environment.md) · [本地运行与内部测试](guides/internal-testing.md) · [脚本入口](../scripts/README.md)
- [前端目录结构](../apps/web/README.md#目录结构) · [界面开发规范](../apps/web/README.md#界面开发规范)

## 部署与发布

- [Web 构建、同源代理和更新](deployment/web-static.md) · [Nginx 配置示例](deployment/nginx-reverse-proxy.conf)
- [后端 systemd 服务](deployment/systemd/bangdream-optimize-backend.service) · [后端环境变量示例](deployment/systemd/bangdream-optimize-backend.env.example)
- [桌面打包、Windows 便携版和上传](deployment/release.md)

## 计算规则与设计

- [计算架构与活动支持范围](calculation/calculation-architecture.md) · [击打优化](calculation/hit-timing.md)
- [控分设计与实现](calculation/score-range-design-draft.md) · [最大平均 PT 设计草案](calculation/pt-maximize-design-draft.md) · [指定队伍 PT](calculation/pt-evaluate-design.md)
- [技能触发顺序](calculation/skill-activation-order.md) · [技能排队](calculation/skill-queue-support.md)

## 数据与接口

- [HTTP API](reference/server-api.md) · [档案和缓存](reference/persistence.md) · [内部遥测](reference/telemetry.md)
- [卡牌发布时间排序](reference/card-release-order.md) · [活动卡池与结果加成](reference/event-card-pool-and-result-bonuses.md) · [游戏账号导入](../crates/bangdream-account/README.md)

## 历史记录

前端验收、项目迁移、计分精度核对和性能实验保存在原工作区的 `docs/archive/`。
