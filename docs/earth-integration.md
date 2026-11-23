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
