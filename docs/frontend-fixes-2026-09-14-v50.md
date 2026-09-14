# 歌曲、角色加成与生产纹理修正

## 交互与显示

- 恢复活动曲：直接将当前活动的预设曲目写回当前档案，不打开歌曲选择窗口。巡回演出保留三曲位，其他模式保留一曲位；预设不足时其余位置留空。
- 歌曲选择中的“活动曲”范围按活动 `seq` 顺序排列，无 `seq` 时保留预设数组顺序；“全部歌曲”和“本次已选”仍按当前服发布时间新到旧排列。
- 角色加成始终显示潜能、任务的演出／技巧／形象共六项百分比。点击角色仍可编辑，保留乐队筛选、搜索、持有筛选及批量设置。

## 生产纹理原因及修复

2026-09-14 对 `https://calc.krkrdkdk.cn` 的只读检查：

- 头图 DOM 为 `data-texture-state="fallback"`，原图为 `data-pixel-readable="false"`。
- `/bestdori/header/assets/tw/characters/resourceset/res018083_rip/card_normal.png` 返回 HTTP 200、`Content-Type: text/html`、55228 字节，实际落到了 SPA 首页。
- 该问题发生在图片读取阶段，纹理 Worker 尚未获得可读取的图像像素。无需调整纹理算法或模糊强度。

在生产站点实际生效的 **HTTPS server 块内**增加（或修正）以下 location。保留现有证书、站点及其他代理设置；仓库示例完整配置见 `docs/nginx-reverse-proxy.conf`。

```nginx
location ^~ /bestdori/header/ {
    proxy_pass http://127.0.0.1:3100;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

`proxy_pass` 末尾不要加 `/`，后端需要收到完整的 `/bestdori/header/…` 路径。使用 `^~` 防止其他图片后缀规则截获请求。

验证配置后重载，再通过公网入口检查真实 PNG：

```bash
sudo nginx -t && sudo systemctl reload nginx
bash scripts/check-header-proxy.sh https://calc.krkrdkdk.cn
```

检查脚本验证状态码、PNG MIME 类型和 PNG 文件签名；失败时退出非零。可将可用的其他 `assets/.../*.png` 路径作为第二个参数。

后续更新可用 `bash scripts/update-production.sh --public-url https://calc.krkrdkdk.cn`，以便在发布结束前执行同一检查。更新脚本不会覆盖服务器现有 Nginx 配置，仍需首次补充代理规则。

本次仅做公网只读诊断，尚未修改生产服务器配置。前端保留头图代理的具体失败原因，便于后续从 DOM 的 `data-texture-error` 排查。

## 本地验收

- `node --test apps/web/test/*.test.js`：102 项通过。新增打乱数组顺序但保留 `seq` 的案例，覆盖单曲／三曲恢复、难度保留、空位补齐及无 `seq` 回退。
- 在 `localhost:8093` 独立浏览器存储中验收，未修改 `127.0.0.1:8093` 的原有档案。活动 #341 首曲替换为 #567 后，点击恢复立即回到 #810、#773、#567，无打开的对话框；刷新后顺序不变。
- 角色六项分别设置为 1.1%、2.2%、3.3%、4.4%、5.5%、6%，显示和刷新持久化均正确。全部 40 位角色都显示六项；宽屏每行五位，390px 窄屏两列，无横向溢出。
- 本地活动 #341 的头图达到 `data-texture-state="ready"`。
- `check-header-proxy.sh`：本地真实 PNG 返回退出码 0，生产 HTML 返回退出码 1。两个部署脚本的 Bash 语法检查通过。
