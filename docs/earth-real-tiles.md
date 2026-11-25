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

## 已知显示限制

当前模型shader显示基础色/纹理，没有法线光照和PBR明暗。
该无纹理示例因此呈灰色轮廓，不能据此声称已验证表面光照细节。
LOD几何加载与显示正常；后续应补充基础光照来展示扫描表面细节。
