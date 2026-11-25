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

### 按范围自动选取细分间隔

每个独立点线/环在加载时计算ECEF包围范围，避免日期变更线附近被误判为跨全球。
默认间距取“范围尺度 / 256”和“视距 / 视口高度”中的较小值，限制在1–5000米；放大时进一步加密。
例如范围尺度约1公里时约4米、100公里时约390米（近距离查看会更细）。超过总顶点预算时统一降低密度，保留原始线段。
这是几何采样精度，不会增加源DEM的实际分辨率。此策略替代上一节的固定5米下限。

## 系统内置地球技能

`skills/earth-workbench/SKILL.md` 随 Keeper 二进制内嵌，技能标识为 `system:earth-workbench`。
所有智能体的技能目录默认包含它；“技能”页显示固定选中的 `[系统内置] earth-workbench`，无需手动关联。旧回复重新生成时也补齐缺失的系统技能，用户管理的技能仍沿用原绑定。
系统仅注入名称与描述，正文通过 `load_skill` 分页按需读取；不会复制到项目目录或覆盖用户技能，不改变审批规则。
技能版本按内容哈希绑定到每次回复；旧回复重新生成时如程序内置版本已变化会明确报错，请发起新回复。
流程覆盖定位、选中要素、GeoJSON转换和贴地、预览图重投影、下载、原生叠加及实际工具结果核验。

## 图层管理

地球窗口按 L（包括焦点位于地球 iframe 内时）或点击工具栏“图层（L）”打开已加载图层列表。
列表包含 GeoJSON 与图片，可点击“显示／隐藏”切换可见状态，也可点击对应“删除”或“清空图层”。可见状态随项目场景保存，刷新及独立标签页同步后仍保持。只修改当前项目场景的 overlays/images 并清除旧选择，不删除项目文件、图片快照、下载链接或底图。
场景通过账户校验和版本锁保存，其他地球标签页自动同步；原生渲染器移除网格/纹理引用。
输入框、编辑器内及带 Ctrl/Alt/Meta 的组合键不触发，父页面 Esc 关闭面板。

## Static GLB models

`earth_publish_model` publishes a project `.glb` as an immutable, SHA-256-deduplicated,
current-account download snapshot. `earth_add_model` adds/updates its placement.
Both are available to the chat agent through the system Earth skill. HTTP equivalents:
`POST /api/invoke/agent_earth_publish_model` and `agent_earth_add_model`; provide
`projectId`, `accountKey`, `path`, and for mutation the current `expectedRevision`.
Download routes: `/api/earth/models/{id}` and `?download=1` (local account only).

On an open project Earth page:

```js
await keeperEarth.addModel({
  path: '3dmodules/Duck.glb',
  longitude: 111.6452, latitude: 32.7489,
  height: 5, heading: 0, scale: 30,
  altitudeMode: 'relativeToGround'
});
```

Coordinates are WGS84 degrees; height is metres relative to sampled ground by default,
or ellipsoid height with `absolute`. glTF uses metres, Y up; X maps east and Z south
at heading 0. Placement is at the model origin (not automatically its lowest vertex).
Re-adding the same content updates its existing layer. Models participate in the L-key
layer list, visibility, deletion, clear, revision checks and detached-tab refresh.
An empty model list is compatible with older saved scenes. Snapshots remain downloadable
after layer removal, just like images; deletion does not delete project source files.

Rendering is native Rust/wgpu, using node transforms, triangle geometry, base colour
and embedded PNG/JPEG textures with the globe camera/depth buffer. No DOM model overlay.
Models use imported vertex normals (or area-weighted generated normals when absent),
inverse-transpose normal transforms, ambient/directional diffuse lighting, view fill and
a small specular highlight. Imagery layers retain their original unlit rendering.
This is not a full PBR renderer: no shadows, metallic/roughness,
normal maps, animation, skins, morphs, required compression extensions or external URIs.
Bounds: 16 MiB per GLB, 8 models, 250k vertices, 64 primitives, 32 MiB decoded texture
budget per model, 128 MiB snapshot budget per account. Unsupported geometry and required extensions fail explicitly; additional PBR material channels are ignored.

Checks: `python3 scripts/test-earth-http.py`, `node scripts/test-earth-model-overlay.cjs`.
Native renderer fixture check (in cangling-desktop):
`EARTH_TEST_GLB=/path/to/Duck.glb cargo test --lib model_layer -- --ignored`.
The sample downloaded to `general/3dmodules/Duck.glb` is from KhronosGroup/glTF-Sample-Assets,
credited to Sony (2006), SCEA Shared Source License 1.0; its README and license are stored
beside the sample. The sample is not bundled in Keeper's executable.

Keyboard `T`: toggle 0° nadir / 45° oblique around the screen-centre ground point,
without jumping to another location. Any existing nonzero tilt returns to 0° first.
At globe-scale altitude the existing horizon-safe maximum tilt remains enforced.

## 场景 / 相机分离与 SSE 同步

- `revision` 只表示图层和选择状态；`cameraRevision` 单独表示已保存视角。
- 相机以 `earth:<账户>:<项目>:camera` 单独保存在 SQLite settings；旧场景的 camera 作为迁移初值，已有图层不变。
- `agent_earth_save` 保留当前相机，不接受旧页面夹带的相机覆盖。
- 手动移动通过 `agent_earth_camera_save(camera, expectedCameraCommandRevision)` 合并保存，600ms 防抖；不影响其他窗口视角。多个窗口手动移动时，最后完成保存的视角作为下次打开的初值。
- `agent_earth_fly_to` / `earth_fly_to` 增加 `cameraCommandRevision`，所有打开窗口同步定位；旧定位版本下的延迟拖动保存被拒绝。
- `/api/earth/events?project=...&account=...` 是本机同源 SSE。仅推送版本元数据；首次连接、重连、通知丢失时触发重新读取。每15秒注释心跳；账户变化结束连接。反向代理应关闭缓冲。
- 图层通知保留当前窗口相机。空闲窗口不再每1.5秒调用 `agent_earth_get`；相机检测仅在原生渲染帧后执行，变化才发请求。
- 渲染确认分别检查 `renderedRevision == revision` 与 `renderedCameraCommandRevision == cameraCommandRevision`；保存成功不表示浏览器已渲染。

验证：`python3 scripts/test-earth-http.py`，`NODE_PATH=/zjs/satway/dev/n8n/node_modules node scripts/test-earth-panel-ui.cjs`，`node scripts/test-earth-bridge-sync.cjs`。

## 原生 3D Tiles（2026-11-25）

地球的“图层（L）”面板可添加 Cangling Map 发布的
`http(s)://<map>/3dtiles/<id>/tileset.json`。智能体使用
`earth_add_tileset({url,name?,maximumScreenSpaceError?})`；
HTTP 对应 `agent_earth_add_tileset`，增加 projectId、accountKey、
expectedRevision。图层按账户和项目保存，参与 SSE 同步、显隐、删除、
清空和刷新恢复。旧场景自动视为 tilesets 空数组，不迁移或删除已有资源。

### 原生执行路径
- `src/tiles3d.rs`（Earth 原生源码）解析显式瓦片树，继承变换和 refine，
  计算 sphere/box/region 的保守包围球，视锥裁剪及按屏幕误差选择 LOD。
- ADD 保留父内容；REPLACE 在可见子树覆盖就绪前保留父内容。
  没有粗级内容的分支允许显示已就绪的部分子内容。
- GLB / 现代 b3dm 解码后复用 Rust/WGPU ModelLayerRenderer；
  glTF 节点变换 → Y-up 转 Z-up → RTC_CENTER → 继承 ECEF 变换，
  再转换为 Earth 内部轴与相机相对坐标，与地球共用相机及深度缓冲。
- `ui/earth/tiles-overlay.js` 只负责受限 HTTP 传输，不解析 LOD 或创建 DOM 模型。
  每 250ms 在本机原生状态机上更新选择；空闲时不轮询后端场景。
  状态有变化时才回报服务器，最多每秒一次。页面隐藏暂停网络，恢复后继续。
- Earth 源码精确归档于 `vendor/cangling-earth/source.tar.gz`，
  `ui/earth/manifest.json` 记录源文件及 WASM 哈希，可独立解包重新构建。

### 预算与边界
- 最多4个 tileset、每个10000节点、深度32（包括外部 tileset）。
- 每层最多32个选中的几何资源（包括回退父级），总计128个；
  超限优先保留近处选择及粗级回退，并报告 budgetLimited。
  无粗级内容的宽分支可能只显示部分内容。
- 几何缓存估计上限128 MiB，单资源16 MiB，单JSON4 MiB；
  瓦片GLB最多500k顶点，普通独立GLB仍为250k；其他几何/纹理限制不变。预算不足明确报错并保留可用父级。
  当前不是大规模城市数据的完整性能实现。
- 4个并发请求、15秒超时、最多3次自动尝试，退避等待。
  不再需要的请求取消，离开选择集或隐藏图层时释放几何资源；
  外部JSON树保留到图层删除/URL变化/页面刷新。
- 支持显式1.0/1.1、content/contents、外部 tileset、静态 GLB、现代28字节头 b3dm，
  b3dm RTC_CENTER 当前支持JSON三元组。batch table 暂不提供要素选择。
- 暂不支持 implicit、pnts、i3dm、cmpt、压缩扩展、外部 glTF 缓冲/纹理、
  完整 PBR、动画。重复外部 tileset URI明确拒绝，避免递归引用。
- URL无用户名密码、查询参数和片段；子资源必须同源且在数据集根路径内，
  不接受父目录跳转或重定向；请求不携带凭证，要求源服务CORS。
  HTTPS页面只能加载HTTPS资源。没有新增服务器通用URL代理。
- `earth_get_view` 的 tilesetStatus 为最近浏览器报告：
  reportedAt、revision、drawing、loaded、pending、errors、budgetLimited、selectedContents（最多8个实际选中资源路径）。
  `renderedRevision` 只代表场景应用；不能当作全部瓦片已经显示。
  多个窗口时保留最后一份报告，报告时间不代表窗口持续在线。

规范参考：[3D Tiles](https://github.com/CesiumGS/3d-tiles/tree/main/specification)，
[b3dm 坐标变换](https://github.com/CesiumGS/3d-tiles/tree/main/specification/TileFormats/Batched3DModel)。

### 验证
- Earth `cargo test --lib`：110通过，1个显式GPU资产测试默认忽略；
  另外指定 Duck.glb 运行该GPU测试通过。9项瓦片树测试涵盖LOD、
  REPLACE/ADD、嵌套变换、无效资源、预算、隐藏取消和重试。
- Keeper `cargo test agent_earth_tiles`、`python3 scripts/test-earth-http.py`：
  URL校验、持久化、版本冲突、状态回报、账户隔离及模拟模型真实工具调用。
- `node scripts/test-earth-tiles-transport.cjs`：并发、取消、越界URL、
  请求凭证策略、正常上传、超大资源、删除后的迟到响应。
- `NODE_PATH=<jsdom所在node_modules> node scripts/test-earth-panel-ui.cjs`；
  `node scripts/test-earth-bridge-sync.cjs`。
- `python3 scripts/test-earth-tiles-browser.py --hold` 创建临时 Map + Keeper，
  发布红色父瓦片、绿色GLB子瓦片和外部tileset内蓝色b3dm子瓦片。
  用打印的独立地球URL检查远景红色/近景绿蓝、显隐及刷新恢复；
  回车停止进程并清理测试目录，不触碰本机用户场景。

## Agri 基础影像更新（2026-11-25）

Agri 图块 URL 的内容会原地更新，不能永久优先读取 CacheStorage。Worker 和 WASM 直接请求回退路径均对 `cangling-imagery-agri-*` 禁用 CacheStorage 读取/写入，使用浏览器 HTTP `cache: no-cache` 重新校验；其他地图资源维持原缓存策略。GPU 中已显示的图块在页面重载后重新请求，不新增高频后台轮询。源站无验证响应头时会重新下载图块。

Agri 最大级别由 18 提高至 20，仍受原有 LOD/并发/显存预算限制；源站透明占位图继续走原有低级别回退，不能等同于有真实20级影像。

验证：WASM release 和 Keeper debug 构建通过；`node scripts/test-earth-imagery-cache.cjs` 验证可变影像重新校验、其他资源缓存复用、网络失败不偷偷返回旧影像；3D Tiles 传输回归通过。Chrome 中刷新当前地球页面后郑东新区 CBD 的建筑与道路影像明显更新。
源站抽查 WGS84(113.728,34.775)：12/16/18级有有效影像，20/855544/416137返回256×256全透明PNG。该源站数据覆盖/发布问题不属于客户端缓存修复，未修改服务器数据。

### 已发布三维数据集发现与智能体加载

Map 新增公开只读 `GET /api/3dtiles/discover?q=名称&bbox=西,南,东,北&id=数据集&limit=50`。
只返回启用且已发布的数据集，不暴露磁盘路径。默认50、最多100，返回total/truncated。
范围来自根包围体：region转换为WGS84度；小范围ECEF box/sphere应用根transform后估算包络。
未知/大范围/跨日期变更线不自动定位；bbox查询排除未知范围。包络不是精确建筑覆盖率或逐栋属性。

Keeper 通过 `KEEPER_EARTH_MAP_URL` 设置Map服务根地址（保留反向代理前缀），默认
`https://train.cangling.cn/geoserver/`；本机联调设为 `http://127.0.0.1:4310/`。
模型不能指定任意网络目标，目录请求15秒超时、响应最多1MiB、不跟随重定向、不携带登录凭证。
`earth_search_datasets`查询；`earth_load_dataset`按ID重新解析当前发布版本，用同源版本化URL加载。
图层持久化datasetId/datasetVersion/bounds；同版本重复加载复用图层，新版本不会替换旧版本。
`earth_remove_tileset`删除场景引用。显隐、相机定位和加载报告复用现有工具。
任务停止/超时会取消未完成的目录请求，账户切换后再次校验会话归属再保存。

系统技能指导按“查询→核对范围→加载→定位→核对浏览器报告”执行。
场景已保存与模型已绘制分别报告；不添加按业务关键词猜测是否成功的运行时规则。
新增HTTP目录能力需要先升级Map；旧Map返回404时工具明确报告，不虚构数据或静默改用硬编码URL。

### 默认 Cangling Map MCP

Map提供 `/mcp`（Streamable HTTP，兼容2025-03-26/2025-06-18/2025-11-25协议），
提供`search_3d_datasets`和`get_3d_dataset`只读工具。地球场景、定位、加载报告仍由Keeper本地工具执行。
系统earth-workbench技能优先选择已声明的Cangling Map MCP工具，旧HTTP内置工具保留兼容。

Keeper首次打开账户时添加默认服务并通过initialize/tools/list发现工具；默认地址
`https://train.cangling.cn/geoserver/mcp`。可用`KEEPER_MAP_MCP_URL`覆盖；
未设置时从`KEEPER_EARTH_MAP_URL`服务根地址派生；显式空值关闭自动添加，便于隔离测试。
本机联调使用`http://127.0.0.1:4310/mcp`。本次仅本机运行，公网端点需要部署新Map后才可用。
配置按账户存SQLite，`map_mcp_default_v1:<owner>`记录一次性安装状态；删除后不重新添加，
修改/停用后不覆盖。相同URL的已有HTTP服务保留原配置。初次发现最多等待3秒，
失败不阻止打开会话，设置页显示错误；重试至少间隔60秒，可手动“连接并发现”。
发现成功后使用缓存工具声明，无每轮自动扫描。账户切换或配置修改会阻止旧发现结果覆盖新配置。
MCP调用继续使用现有审批、停止、账户隔离和连接池逻辑，readOnlyHint不会绕过审批。

验证：`scripts/test-map-mcp.py`连接真实本机Map，隔离Keeper数据库并使用模拟模型，
通过默认发现、两次MCP审批、查询/详情、Earth加载/定位/状态、重启、删除保持和账户隔离。
测试前启动Map、发布郑州样例并设置`MAP_PUBLIC_BASE_URL=http://127.0.0.1:4310/`。
自定义智能体仍遵守其MCP工具关联清单；新增默认服务不会自动扩大已有智能体的权限，
需要在智能体的MCP配置中关联这两个工具。通用助手可直接使用已启用服务。
