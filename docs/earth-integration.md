# Keeper 三维地球接入

## 使用

在项目更多菜单或会话顶部选择「三维地球」。选择同项目会话后，可开启「对话分屏」。
场景按当前账户和项目保存到 SQLite settings；重新打开页面恢复相机、图层和最后选择。
已打开的地球面板及分屏状态按账户记忆，刷新后自动恢复；同项目会话切换保留地球画布，点击返回会话才关闭。
相机手动操作每 1.5 秒保存一次，关闭后不再轮询；切换账户销毁 iframe，释放渲染器和 worker。
后台页暂停渲染，前台恢复。Keeper 内嵌页默认使用已验证的 WebGL2 兼容后端，避免当前浏览器 WebGPU/Vulkan 分配失败反复发生。独立地球入口仍保留 WebGPU 自动选择与 WebGL2 回退。此调整并未证明底层驱动/显存分配故障已根治。
影像、DEM 等仍由 cangling-desktop 现有地图服务在线提供；只内嵌渲染代码，不含离线全球数据。

地球页使用精简布局，只显示项目标题、状态、分屏和返回入口。定位、图层切换及 GeoJSON 加载通过智能体工具进行，不再显示功能操作面板。
通过 `earth_add_geojson_layer` 输入项目相对路径（如 `keeper-earth-demo.geojson`）加载 WGS84 GeoJSON。
支持 Point/MultiPoint、LineString/MultiLineString、Polygon/MultiPolygon 的点和线框展示。
点击点/轮廓保存选择信息，智能体通过 earth_get_selection 读取；地球画面不再显示属性或测量结果消息框。
每层最多 500 个要素、5000 个坐标，整个场景约 1 MiB、最多 8 层。拒绝路径穿越、链接、受保护文件和自定义 CRS。
底层桥接保留测量和截图能力，当前页面不提供操作入口。两点测量使用拾取位置计算近似球面距离，不计地形起伏；不是工程精密测量。
截图在 GPU 提交画面时复制 canvas，再叠加业务图层，显示预览后可下载 PNG。

## 智能体工具

- `earth_get_view`：读取保存的相机、图层与最近渲染确认。
- `earth_fly_to`：按经纬度与高度立即定位（当前版本没有飞行动画）。
- `earth_set_layer_visibility`：切换 boundaries、graticule 或 GeoJSON 路径对应图层。
- `earth_get_selection`：读取最后选择的属性/测量结果。
- `earth_add_geojson_layer`：使用当前任务项目的受限文件读取能力加载 GeoJSON。

工具沿用 ReAct 执行记录和停止机制。同步场景修改完成后立即返回，不等待页面打开；停止不会撤销已保存的场景。
修改结果为 `saved_not_rendered`。只有打开地球页面并绘制新帧后，才确认 renderedRevision。
确认表示该修订已应用到 GPU 画面，**不保证远程影像/地形瓦片全部下载完成**。
关闭页面后的确认是历史记录，须结合 renderedAt 时间理解，不能当作当前在线状态。
页面只接受同源、指定 iframe 的消息；保存/确认还校验账户键和场景修订，拒绝过期覆盖。
项目目录沿用 Keeper 当前的本机共享规则；场景按账户隔离，不改变目录本身的访问权限。

## 编译产物与重建

`ui/earth/` 是随 Keeper 打包的运行产物，无需用户安装 Rust 或启动另一个地球服务。
执行 `python3 scripts/import-earth-web.py ../cangling-desktop` 编译并导入。
需要 wasm32-unknown-unknown target 以及与 Cargo.lock 匹配的 wasm-bindgen CLI（本次 0.2.129）。
`ui/earth/manifest.json` 记录源码提交、工作区是否有未提交修改、各源码与产物 SHA-256。
本次使用 cangling-desktop 当前工作区，包含接入前已存在的功能修改；没有擅自提交该仓库原有修改。
为避免仅依赖脏工作区，在 `vendor/cangling-earth/source.tar.gz` 保存了本次精确编译输入（无 .git、凭证、target）。
可解压到空目录后用上述导入脚本重建；没有 Git 元数据时，清单只记录文件校验值。
字体版权文件随产物保存在 `ui/earth/licenses/`。

## 验证（2026-11-23）

- Web/WASM release 编译及 Keeper debug 编译通过。
- `cargo test --lib`：225 passed，1 ignored（原有）。
- `python3 scripts/test-earth-http.py`：隔离 SQLite、重启恢复、乐观锁、账户切换、链接和越界路径、非法坐标；模拟模型通过 ReAct 连续执行五种地球工具；WASM MIME 正确。
- `NODE_PATH=/tmp/keeper-ui-check/node_modules node scripts/test-earth-panel-ui.cjs`：消息来源校验、绘制确认、分屏、过期响应与 iframe 清理。
- 项目浏览器和智能体配置界面回归通过。
- 本机 Chrome：实际地球渲染、相机定位、GeoJSON 点和面轮廓叠加、属性选择、刷新后场景恢复、两点测量（约 91.8 公里）、PNG 截图预览（1317×633）。浏览器使用 WebGL2 回退；未据此宣称其他浏览器/WebGPU 路径全部验证。

## 后续

业务图层当前为有界 SVG 屏幕投影叠加，处理椭球背面隐藏，尚无精确地形遮挡和贴地细分。
大栅格、3D Tiles/glTF、地形精密量测、海量矢量 GPU 管线和场景备份导出将在后续独立实现。
当前会话备份范围不包含地球场景 settings。

## 地球工作台

会话顶部的“地球工作台”进入全屏地球背景模式。消息区域固定浮在地球右侧，输入区独立固定在窗口底部居中。两者使用简单边框，
取消窗口标题栏、阴影和圆角，继续使用同一套消息、流式回复、工具确认和停止生成功能。
“收起消息 / 显示消息”释放地球操作空间，收起消息后仍可输入和查询项目，
“返回会话”退出工作台。

输入完整 `/projects` 自动打开项目与会话查询，也可点“项目 / 会话”。
查询使用当前账户的分页接口，按项目分组；点击项目名进入该项目会话列表，
点击“打开项目”切换项目，点击会话进入对应对话。支持上下方向键、Enter 和 Esc。
该命令不发送给模型。搜索沿用现有会话检索（包含消息正文）。

同项目切换会话复用地球实例，跨项目恢复该项目已保存的场景。
页面刷新恢复会话、地球场景以及工作台模式、消息区收起状态（旧版窗口位置与尺寸不再应用）。
布局按账户保存在本机浏览器，场景沿用 SQLite；账户切换关闭查询并丢弃旧查询响应。
空项目允许直接开始新对话，首次发送时按现有机制创建会话。

验证：`NODE_PATH=/tmp/keeper-ui-check/node_modules node scripts/test-earth-workbench-ui.cjs`
覆盖命令拦截、跨项目切换、布局恢复、收起与账户隔离；
`test-earth-panel-ui.cjs` 继续验证消息来源、场景确认和 iframe 生命周期。

自动恢复等待主页面加载完成后创建地球。若浏览器将动态 iframe 留在
`about:blank`，仅尝试一次读取本机 `/earth/index.html` 并以同源内联文档恢复；
固定 base URL 指向 WebGL2 查看器，桥接仍校验来源与 iframe 窗口。
已加载或已经就绪的地球不会被该分支重启。此分支包含在工作台 UI 回归中。

## 定位接口与对话联动

在打开地球的 Keeper 同源页面中调用：

```javascript
await window.keeperEarth.flyTo({longitude: 116.3913, latitude: 39.9075, height: 20000});
const scene = await window.keeperEarth.getView();
```

坐标为 WGS84 十进制度，高度单位米；省略高度保留当前值。接口保留方向、倾角和图层。
返回 `rendered` 仅表示本次修订已收到画面确认，瓦片仍可能加载中；否则返回 `saved_not_rendered`。
未打开项目地球时拒绝调用。请求期间切换项目或账户后，不把旧响应应用到新项目。

本机 HTTP 接口沿用 Keeper 的来源与账户校验（不新增匿名公网入口）：
`POST /api/invoke/agent_earth_fly_to`，JSON 参数示例：

```json
{"projectId":"general","accountKey":"guest","expectedRevision":0,"longitude":116.3913,"latitude":39.9075,"height":20000}
```

`accountKey` 使用当前登录账户标识，`expectedRevision` 从 `agent_earth_get` 读取；不能固定写死。
HTTP 保存成功并不表示页面已经渲染，读取 `renderedRevision` 核对。

对话中可输入：“将地球定位到东经116.3913、北纬39.9075，观察高度20公里。”
Agent 使用已有 `earth_fly_to`，打开的地球约1.5秒内同步；关闭时保存位置，重新打开恢复。
地球画面双击可选取一个位置，随后在对话中输入“读取我在地球上选中的位置”，
Agent 通过 `earth_get_selection` 获取拾取坐标。没有内置地名搜索服务，不应编造精确坐标。

## 双屏独立地球窗口

打开三维地球后，在“仅地球／对话分屏”和“返回会话”旁点击“地球新标签页”，再将浏览器标签页拖出到第二块屏幕。
原窗口保留对话，独立页面 `/earth-window.html?project=...&account=...` 固定显示打开时的项目。
对话中的 `earth_fly_to`、图层修改通过同一后端场景同步；双击地球选点后，模型可读取该项目的选择。
可关闭原对话页面而保留地球，也可刷新地球页恢复项目和相机；不依赖 `window.opener`。
切换会话到其他项目不会自动改变独立窗口，需为新项目重新打开。
独立页不修改主窗口的地球布局记忆；打开它时关闭当前嵌入式地球，减少双渲染器占用。
场景约每1.5秒同步；页面被浏览器隐藏时暂停，恢复可见后继续。双屏使用时请拖成独立窗口。
账户改变后拒绝旧账户的场景读取和写入并关闭画布；从当前账户对话重新打开。

## 项目图片下载与地理覆盖层

智能体可用 `earth_publish_image({path:"output/preview.png"})` 发布当前项目图片，返回
`url` 和 `downloadUrl`（后者带 `?download=1`，可放入回复的 Markdown 下载链接）。
地址相对于 Keeper 的 origin，例如 `http://127.0.0.1:4301/api/earth/images/<id>?download=1`。
图片按内容哈希去重并快照存入 SQLite，后续源文件修改不会改变旧链接；仅当前账户在本机访问，不能作为公网共享链接。

`earth_add_image_layer` 将图片添加到当前项目地球：

```json
{"path":"output/preview.png","polygon":[[116,39],[117,39],[117,40],[116,40]]}
```

外部本机调用可 POST `/api/invoke/agent_earth_publish_image`（projectId、accountKey、path），
或 POST `/api/invoke/agent_earth_add_image`（另加 polygon、expectedRevision）。
浏览器当前地球页面也可调用 `window.keeperEarth.addImageLayer({path,polygon})`。
HTTP 调用遵循已有 loopback / Origin 校验，场景修改使用乐观锁；仅图片加载并绘制后确认 renderedRevision。
`earth_get_view` 返回 imageLayers；`earth_set_layer_visibility` 可用图片 ID 控制显隐。
独立地球标签页通过既有场景同步接收图层。

限制：PNG/JPEG/WebP，单张4 MiB、边长8192、24M像素，显示纹理最大2048；最多8层，账户快照总量128 MiB。
多边形为 WGS84 经度/纬度3–32点，不支持孔洞、自交和跨日期变更线。
图片由 cangling-desktop 的原生 ImageLayerRenderer 绘制：Rust 解码并裁剪，上传 GPU 纹理，以64×64经纬度网格接入地球相机及深度缓冲，随地形采样高度更新。前端仅 fetch 二进制并通过 WASM 传递，不创建图片 DOM/Canvas 覆盖层。
图片按北向上经纬度包围矩形映射；不支持任意角点配准。固定网格对大范围、陡峭地形存在精度限制，不等同于多级地形瓦片。
GeoTIFF 应先重投影到 EPSG:4326 并生成 PNG 预览，保留准确范围；普通图片必须由用户提供范围，不猜测定位。
图片快照目前不纳入会话备份包；原项目文件仍应保留。

验证：`python3 scripts/test-earth-http.py` 覆盖七种工具、图片下载、重启恢复、账户隔离及范围校验；
`node scripts/test-earth-image-overlay.cjs` 覆盖二进制传输、缓存、可见性与清理；原生 image_layer 测试验证多边形遮罩、WGSL 及实际 GPU 管线/纹理上传。

## 原生 GeoJSON 矢量渲染

GeoJSON 业务图层使用 Rust `VectorLayerRenderer`，不再创建 SVG / DOM 图层。
支持 Point、MultiPoint、LineString、MultiLineString、Polygon、MultiPolygon；多边形目前绘制外环及内环边界，不填充面。
输入采用 WGS84 经纬度，转换为 ECEF 三维位置，再按相机转换成 GPU 三角网格。
没有第三维时采样地形；显式第三维按绝对椭球高（米）处理。线段经纬度插值细分，跨日期变更线取短路径。
点标记和线宽保持屏幕像素尺寸，使用地球深度缓冲；超过网格预算的数据会报错，需简化或拆分。
原生 `vector_scene` 接收现有 overlays，`vector_pick` 返回点击位置附近要素的 layer/feature/properties，继续写入原有选择状态。
拾取按投影后的点/边距离判断，不支持多边形内部选择；复杂地形遮挡处的拾取仍需后续 GPU picking 完善。
地球截图直接来自主渲染画布，包含原生图片和矢量图层。

验证：cangling-desktop `cargo test --lib vector_layer` 包括坐标/几何解析、WGSL 校验、本机 GPU 绘制命令提交、中心点拾取及清理；Keeper 页面测试检查选择桥接且不再创建 SVG。

### 地形插值修正

矢量线段不再按0.25度固定间隔细分：依据相机高度和视口高度选择5–5000米采样间距，近距离查看公里级边界时可获得数百个顶点。
每个细分点重新采样当前DEM的双线性高度，再转换为ECEF位置；DEM加载和视角变化后逐帧更新。
总网格预算不足时统一降低细分密度，保留每条原始线段，不因前部数据耗尽预算而截断后续边界。
显式GeoJSON第三维仍按绝对椭球高处理；只有未提供第三维的坐标默认贴地。
