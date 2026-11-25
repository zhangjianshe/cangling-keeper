---
name: earth-workbench
description: 在 Keeper 三维地球中定位、叠加项目图片与 GeoJSON、加载 GLB 三维模型、读取选中要素及提供资源下载链接。用于地理展示任务，不用于普通图片编辑。
---

# 地球工作台

使用本轮实际声明的 earth_* 工具控制当前会话项目的原生 GPU 地球。技能只提供工作指导，不改变工具审批或文件权限。没有相应工具时说明限制。

## 定位与选中要素

- earth_fly_to：WGS84 longitude、latitude，height 为相机高度（米）。经纬度须来自用户或可信地理数据，不能猜测。
- earth_get_selection：读取地球选点、矢量属性或测量结果。地名不等同于精确坐标。
- earth_get_view：读取相机、businessLayers、imageLayers、revision 与 renderedRevision，以及 cameraCommandRevision 与 renderedCameraCommandRevision。

## GeoJSON 矢量

通过 earth_add_geojson_layer 的 path 加载项目相对路径。数据须为 WGS84，经度在前、纬度在后；投影坐标先用 GDAL/OGR 转换。支持点、线、多边形及 Multi 类型，多边形仅显示边界，不填充。
原生渲染器转换为 ECEF 并自动按范围、视距和预算细分。二维坐标采样 DEM 贴地；第三维为绝对椭球高，不能为贴地效果随意补零。工具错误时检查数据，不绕过验证。

## 图片与下载

1. 先确认文件真实存在。PNG/JPEG/WebP 才能直接叠加；GeoTIFF 应先读取坐标系、有效值和范围，再重投影到 EPSG:4326 并生成北向上 PNG。生成操作用文件/命令工具，遵守现有审批，不覆盖用户原文件。
2. 用 earth_publish_image({"path":"output/preview.png"}) 获取真实 url、downloadUrl。返回链接为本机当前账户地址，不是公网分享。回复可写 Markdown 下载链接。
3. 用 earth_add_image_layer({"path":"output/preview.png","polygon":[[116,40],[117,40],[117,39],[116,39]]}) 加载。示例坐标仅解释格式，不作为实际任务位置。
4. 图片按北向上包围矩形映射后裁剪；角点必须沿边界顺序排列，不能交叉。不支持孔洞、日期变更线或任意角点配准。用重投影后图片的范围，不能将未重投影的旋转图片直接铺满经纬度外接矩形并宣称精确配准。

限制：单张4 MiB、边长8192、24M像素；显示纹理最大2048；最多8个图片图层。原始分析数据和预览图是不同产物。
用 earth_set_layer_visibility 的 layer 指定 GeoJSON 路径或图片 ID 控制显隐。

## GLB 三维模型

用 earth_publish_model({"path":"3dmodules/Duck.glb"}) 获得当前账户的真实下载URL。
用 earth_add_model({"path":"3dmodules/Duck.glb","longitude":116.4,"latitude":39.9,"height":0,"heading":0,"scale":1,"altitudeMode":"relativeToGround"}) 加载；示例坐标不能替代用户要求的位置。longitude/latitude是WGS84度，height是米，relativeToGround表示模型原点相对地形的高度；absolute表示椭球高。模型使用glTF的Y向上、米单位，scale等比缩放，heading为绕竖直方向旋转的度数。
第一版支持静态、纹理与缓冲区内嵌的GLB 2.0三角网格、节点变换、基础颜色及纹理；不支持动画、蒙皮、Draco等必需扩展或完整PBR。单文件16MiB、8个模型；不要把这种支持描述为完整glTF渲染器。earth_get_view返回modelLayers，可用模型ID控制显隐。更新位置使用同一文件再次调用earth_add_model，不重复增加图层。
核对revision/renderedRevision后再报告加载状态，不通过DOM模拟模型渲染。

## 完成核验

以本轮工具返回值为证据，不能把历史回复中的文件名、版本号或“已完成”当事实。
核对文件存在、目标图层出现在列表中，然后核对新版本 revision 与 renderedRevision。旧场景已绘制不代表新图层已添加；没有工具执行就不能声称生成或加载成功。
场景保存但未确认绘制时说明“已保存，等待地球页面加载”，不要循环重复添加。绘制确认也不是目视验收，不保证图层在当前相机可见范围内；需要时定位到真实范围中心再核对。

视角独立同步：手动拖动不会改变图层 revision。使用 earth_fly_to 后须核对 renderedCameraCommandRevision == cameraCommandRevision；只有图层 renderedRevision 相等不能证明定位已显示。

## 原生 3D Tiles
使用 `earth_add_tileset` 添加 Cangling Map 发布的 HTTP(S) `/3dtiles/<id>/tileset.json`，
可指定 name 和 maximumScreenSpaceError（2–128，默认16）。数据集已有地理坐标，
不要把 GLB 经度纬度放置参数套用到 tileset。然后用 earth_fly_to 定位到数据范围。
支持显式 1.0/1.1 瓦片树、嵌套 tileset、静态 GLB / b3dm、ADD / REPLACE；
暂不支持隐式瓦片、点云、i3dm/cmpt 和压缩扩展。资源服务必须允许浏览器 CORS，
HTTPS 页面须使用 HTTPS 资源，无登录凭证和查询参数，子资源必须在数据集目录内。
瓦片树遍历、视锥裁剪、屏幕误差 LOD、模型解码与绘制均由原生 Rust/WGPU 完成。
使用 earth_get_view 核对 tilesetStatus.layers 的 drawing、pending、errors 和 reportedAt；
这是最近浏览器报告，不代表浏览器仍在线。renderedRevision 只确认场景应用，
不能证明瓦片全部加载。用 earth_set_layer_visibility 的图层 ID 控制显示或隐藏。
