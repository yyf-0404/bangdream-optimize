# 客户端默认键型素材

2026-10-31 从国服 9.4.4 客户端对应资源清单中的两个官方 UnityFS 包提取，替换上一轮第三方 `skin00_rip` 副本。资源条目的版本为 `9.4.3.0`，不是 APK 版本号。

- [普通音符包](https://l4-prod-patch-bd.bilibiligame.net/assetbundle/9.4.3.0/Android/ingameskin/noteskin/skin00)：673,031 字节。
- [定向滑键包](https://l4-prod-patch-bd.bilibiligame.net/assetbundle/9.4.3.0/Android/ingameskin/noteskin/directionalflickskin00)：200,485 字节。

`provenance.json` 保存包 SHA-256、Sprite／Texture2D 名称和对象 ID、尺寸、裁边偏移、PNG 及 RGBA 校验值。提取器为 `scripts/research/extract_hit_timing_client_skins.py`；只解码客户端纹理，按 Sprite 数据恢复透明边距，不重绘或改色。原包抹除了 Unity 版本，解码使用已核对客户端的 `2022.3.62f3` 作为回退版本。

二维平行轨道统一使用正中轨道的 `_3` Sprite，避免引入原透视轨道的倾斜。普通滑键本体和箭头是两张独立贴图；侧滑按实际 `width` 逐轨重复键体，只在方向末端显示一个提示箭头。侧滑轨道轴按客户端 2.2 单位轨距、100 像素／单位换算：308 像素外框含透明边距，不能直接压成一轨；箭头使用客户端动画首帧的 ±1.6 单位位置。时间轴厚度为编辑器显示调整。详见 `docs/hit-timing/history/directional-layout-20261031.md`。

本静态预览省略侧滑背线效果；`left_line`／`right_line` 保留为已提取素材，不再用于手工补线。客户端本身有 `NoteMultipleDirectionalFlickBackLine`，不能据此说原生没有连接线。`long` 使用客户端的完整 `longNoteLine` Texture2D。

显示时顺时针旋转 90°，左滑朝上、右滑朝下；所有 308 × 120 像素的键体共用轨道方向 47.6 像素、时间方向 20 像素的显示框。普通滑键箭头也按同一比例换算，位置固定在客户端动画首帧。斜绿条按时间方向剪切，端面平行于节点击打线。几何显示变换不修改 PNG 或击打坐标。

单键的紫色上缘和青色下缘来自客户端原贴图；旋转后分别在右侧与左侧。游戏素材权利归原权利人。

轨道相对位置已直接与《曇天》国服 SP 原始 BMS 核对：65 组侧滑的 90 个键体分别对应实际轨道中心；提示箭头不表示新增轨道。详见 `docs/hit-timing/history/lane-alignment-20261031.md`。
