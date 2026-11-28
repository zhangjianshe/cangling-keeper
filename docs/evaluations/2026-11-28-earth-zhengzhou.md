# 郑州建筑物图层与定位未生效：地球初始化重入修复

2026-11-28，修复版本 v0.0.141。

用户已通过工具加载郑州 CBD 的建筑物 3D Tiles 数据集并设置 WGS84 相机
（113.728°E、34.775°N、高度 5000 米），场景保存成功但页面仍停留在全球视图。
服务端回执显示 `sceneApplied=false`、`cameraApplied=false`。

浏览器复现后定位到 WebAssembly 的 `src/keeper_bridge.rs`：
`RefCell already borrowed`。渲染器在 `RedrawRequested` 中仍持有可变借用时，
同步发送 `cangling-earth-frame` / `cangling-earth-ready`；JavaScript 回调再同步调用
`keeper_command`，重新借用同一渲染器而触发 panic，导致就绪握手中断。

修复包含：

- 原生 Web 渲染器在向浏览器发送画面/就绪事件前释放可变借用。
- Keeper 导出桥接短暂复制句柄并使用 `try_borrow_mut`，意外重入返回可处理的 busy 错误，避免再次使 WASM 实例陷入 panic。
- 就绪初始化与漫游状态采样延后到当前原生回调退出后执行；错误显式回传父页面。
- 桥接与漫游测试模拟原生帧中禁止再次调用内核，验证事件处理正确延后。

渲染器修复源提交：`cangling-desktop` 的 `93e38ee4254239ad8886a8280b07d8268c4dcdb1`。
Keeper 已重新构建并导入 WASM、JavaScript 与精确源归档；文件校验见 `ui/earth/manifest.json`。

本机验证：

- WASM release 构建通过。
- 地球桥接、漫游、分屏面板、独立窗口、瓦片传输、工作台与后端 HTTP 回归通过。
- 实际郑州数据集清单 HTTP 200，CORS 允许页面请求。
- v0.0.141 本地服务健康检查通过；现有图层和相机均无需重建。
- 浏览器打开原有项目后，郑州 CBD 建筑物可见；图层为 `draw_submitted`，`drawing=1`、`pending=0`、无错误。
- 服务端窗口回执为 `sceneApplied=true`、`cameraApplied=true`、`layerReportCurrent=true`。
- 修复加载后的浏览器日志中未发现新的 panic / RefCell 错误。

截图保存在本机 `/tmp/keeper-zhengzhou-buildings-fixed.png`；没有将用户对话截图或数据库提交到仓库。
