# 建筑材质开发计划与验证

## 第一轮：模型自带基础颜色贴图（已实现）

普通 GLB 与 3D Tiles 的 GLB/b3dm 内容共用 ModelLayerRenderer：

- 读取嵌入图片、材质引用的 TEXCOORD 集合；纹理存在但对应 UV 缺失时明确报错。
- 尊重 glTF 的 Repeat / MirroredRepeat / ClampToEdge，以及放大、缩小和 mipmap 过滤设置。
- 基础颜色纹理以 sRGB 纹理上传；baseColorFactor 在线性空间由着色器相乘。无贴图采用白色 1×1 纹理与颜色系数。
- 在线性、预乘透明度空间生成完整 mipmap，降低远处重复纹理的闪烁及透明边缘黑边。
- 小纹理保持原尺寸，仅大于 2048 的图片缩小。修复旧 thumbnail 调用把小图放大，造成纹理预算虚增的问题。
- 保留单图解码尺寸、单模型纹理预算及整体模型缓存预算，估算包含 mipmap 开销。

当前范围是自包含 GLB 内嵌 PNG/JPEG 等图片。外部图片 URI 不会自动下载；KTX2/Basis、纹理变换扩展、完整 glTF Alpha/PBR 材质仍未支持。

验证资产为原创 `assets/textured-building.glb`，可运行源归档中的 `scripts/build-textured-building.py` 重建：四面墙使用重复窗格，屋顶使用纯色材质。

自动验证：142 项 Earth 测试通过，2 项需要外部条件的测试默认跳过；另外指定该资产执行 `upload_native_model -- --ignored`，GPU 绘制通过。测试覆盖 UV、采样方式、颜色系数、线性 mipmap 和奇数尺寸。浏览器使用 WebGL2 检查立面纹理和无贴图屋顶。

## 第二轮：郑州建筑程序化外观（已实现）

近景墙面按边长与高度生成重复窗格 UV，屋顶独立纯色材质。485 个建筑、三级几何与高度保持不变，本机独立发布 `zhengzhou-cbd-textured`，原数据集不变。浏览器近景绘制 23 个可见叶节点，拉远到 18 km 后绘制 1 个 low 模型；pending=0、errors=[]、budgetLimited=false。450 m 高度近距离可见窗格。该外观是程序化生成，不是真实立面照片。

## 第三轮：材质质量与资源优化（资源部分已实现）

已按完整像素和尺寸复用同一渲染器内的 GPU 纹理，跨 primitive 和瓦片共享；采样器和颜色系数仍各自独立。使用弱引用索引，最后一个模型移除后清理纹理及缓存键；预算保守按每份资源计入 CPU 键和 GPU mipmap 开销。GPU 回归验证重复模型不增加纹理数量、清空场景释放缓存。远、中 LOD 不带纹理，近景才使用立面图片。

法线贴图、粗糙度/金属度等完整 PBR 通道尚未实现，留在后续材质扩展轮次。
