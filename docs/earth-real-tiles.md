# Stanford Dragon 真实 3D Tiles 验收（2026-11-25）

数据来自 [CesiumGS/3d-tiles-samples 的 TilesetWithDiscreteLOD](https://github.com/CesiumGS/3d-tiles-samples/tree/main/1.0/TilesetWithDiscreteLOD)。
上游 README 将数据描述为 Stanford Dragon 网格的三个简化级别，并标注 CC0。
原始 tileset 与三份 b3dm 未做简化、改写变换或替换模型，共4,791,036字节（包含tileset）。

本机发布于 `http://127.0.0.1:4310/3dtiles/stanford-dragon/tileset.json`。
发布版本 `c34549b2fa543e6c1c487bb33fcc4f6be3164508ab4f4397ae00a7616ab7f566`。
源数据、原始许可说明和逐文件 SHA-256 位于
`~/.local/share/cangling-map-local/tiles3d/stanford-dragon/`。
Map 使用用户服务 `cangling-map-local.service`，仅监听127.0.0.1:4310，
未修改其他服务器或设置开机自启。

Keeper 新建独立的“3D Tiles 示例”项目，保留现有 general 场景。
地理中心约为 -75.6120943°, 40.0425306°。在本机独立地球窗口验收：

| 相机高度（正射） | 实际选中资源 | 顶点数 | 已缓存几何资源 |
| --- | --- | ---: | ---: |
| 30,000m | dragon_low.b3dm | 2,324 | 1 |
| 6,000m | dragon_medium.b3dm | 14,794 | 2 |
| 2,000m | dragon_high.b3dm | 269,776 | 3 |

各级状态均为 drawing=1、pending=0、errors=[]、budgetLimited=false；
用原生 `selectedContents` 和已应用相机命令版本确认，不只根据截图推断。
再次拉远到30km后回退low并释放其余资源，再拉近2km可重新加载high。
按T切换45度视角后模型轮廓和空间投影正常。详细观察记录保存于该项目
`LOD-verification.json`，本机模型和服务保留供继续测试。

## 验收中修复

真实高精度瓦片超过原250k顶点上限。仅将3D Tiles渲染器提升到500k，
普通独立模型仍为250k，16MiB单资源限制及128MiB瓦片缓存总预算保持不变。
增加真实b3dm解码测试，验证普通预算拒绝、瓦片预算接受，且顶点数为269,776。

- 原生 `cargo test --lib`：110通过、2个外部资产测试默认忽略。
- 指定 `EARTH_TEST_B3DM` 运行 `decode_real_tiles_budget -- --ignored`：通过。
- 浏览器WASM/WebGL2：三级模型显示、REPLACE远近往返切换、倾斜视角通过。

## 法线与基础光照复验

原先的纯基础色显示已替换为环境光、方向漫反射、少量视角补光和高光。
优先读取 glTF NORMAL，缺失时按三角形面积生成顶点法线；节点变换和地理放置
均使用逆转置矩阵，支持非均匀缩放。法线的 CPU 存储计入缓存预算。
影像图层不使用此模型光照，不改变底图亮度。

- 原生 `cargo test --lib`：112通过，2个外部资产测试默认忽略。
- Duck GLB GPU上传/绘制及法线测试：3通过；包含退化面/未使用顶点、
  非均匀缩放、镜像和奇异矩阵校验。
- Keeper 本机 WASM/WebGL2 中 Dragon 的曲面凹凸、头部和身体明暗可见。
- 45°视角复验 30km/6km/2km，分别选择 low/medium/high；均 drawing=1、
  pending=0、errors=[]、budgetLimited=false。
- 当前是基础光照，不含投影阴影、法线贴图或完整 PBR 材质。
