# Earth 窗口独立确认

每次打开或刷新 Earth 页面生成新的 UUID；嵌入页和独立标签页使用同一协议。
SQLite settings 的账户/项目 `:viewers` 记录保存各窗口状态，旧的全局 renderedRevision 不再作为证据。

## 协议与证据

`agent_earth_ack` 的 phase 为 open、applied、layers、closed，必须携带 windowId。
应用和图层回执同时核对 revision 与 cameraCommandRevision；账户切换、旧版本、未知窗口和未知图层均拒绝。

- sceneApplied：当前版本的场景指令已应用；不表示资源加载完成。
- cameraApplied：对应定位命令已应用。
- layerReportCurrent：当前版本的逐图层报告仍有效。
- loaded：资源准备完成，尚无绘制证据。
- draw_submitted：原生 Renderer 最近一次 render pass 提交过该图层绘制；不保证遮挡后的屏幕像素可见。
- loading、hidden、error 分别表示加载中、隐藏与加载失败。

报告由真实渲染帧触发，最多每 3 秒发送一次。45 秒没有报告即失效；关闭/刷新使用 keepalive 发送关闭回执，异常退出由有效期兜底。后台暂停页面可能显示离线，恢复渲染后重新报告。
最多保留 32 个窗口记录，超过一天清理。基础底图全部瓦片完成状态不属于该协议。图片/模型准备失败目前按该类图层保守报告错误，避免部分成功被误认为全部成功。

`earth_get_view` 返回 windows；可传 windowId 获取 targetWindow。没有指定目标时不得把任意窗口的成功推断为所有窗口成功。技能和工具说明采用同一规则。

## 本机验证（2026-11-26）

- Keeper 单元测试：232 通过、1 忽略。
- Earth 串行单元测试：150 通过、2 忽略；并行运行曾触发原生库 SIGSEGV。
- 独立窗口回归：场景应用不等于图层报告、跨窗口不能继承、旧版本拒绝、关闭拒绝、超时失效、账户隔离。
- 页面和桥接 JS 测试通过。
- 隔离 HTTP 集成测试通过：持久化、账户隔离、场景版本、图片/模型/瓦片及模拟 ReAct 流程。
- Chrome 双独立标签页使用不同 windowId，均收到各自瓦片 draw_submitted；刷新生成新 ID，旧窗口关闭或过期，新窗口重新确认。
