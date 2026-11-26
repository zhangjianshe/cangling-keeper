//! Account/project-scoped Earth state. Browser rendering is acknowledged separately.
use crate::store::Store;
use serde_json::{Value, json};
pub(crate) fn key(s: &Store, project: &str) -> Result<String, String> {
    let exists: bool = s
        .conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_projects WHERE id=?1)",
            [project],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err("项目不存在".into());
    }
    Ok(format!(
        "earth:{}:{project}",
        crate::agent_account::owner(s)?
    ))
}
pub(crate) fn get(s: &Store, project: &str) -> Result<Value, String> {
    let key = key(s, project)?;
    let mut scene: Value = match s.get_setting(&key)? {
        Some(raw) => serde_json::from_str(&raw).map_err(|e| e.to_string()),
        None => Ok(
            json!({"revision":0,"camera":{"longitude":108.9398,"latitude":34.3416,"height":16000000.,"heading":0.,"tilt":0.},"layers":{"boundaries":true,"graticule":false,"source":0},"overlays":[],"images":[],"models":[],"selection":null,"renderedRevision":null}),
        ),
    }?;
    let camera_key = format!("{key}:camera");
    if let Some(raw) = s.get_setting(&camera_key)? {
        let camera: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        for field in ["camera", "cameraRevision", "cameraCommandRevision", "renderedCameraCommandRevision"] { scene[field] = camera[field].clone(); }
    } else {
        scene["cameraRevision"] = json!(0);
        scene["cameraCommandRevision"] = json!(0);
        scene["renderedCameraCommandRevision"] = json!(0);
    }
    if scene.get("tilesets").is_none(){scene["tilesets"]=json!([]);}
    // Never reuse a receipt produced by a different browser window (or old versions).
    scene["renderedRevision"]=Value::Null;
    scene["renderedCameraCommandRevision"]=Value::Null;
    scene["renderedAt"]=Value::Null;
    scene["tilesetStatus"]=Value::Null;
    scene["windows"]=crate::agent_earth_receipts::views(s,project,&scene)?;
    Ok(scene)
}
// Metadata-only notifications: each subscriber checks current account/project before delivery.
#[derive(Clone)]
pub(crate) struct EarthChange { pub owner: String, pub project: String, pub value: Value }
pub(crate) fn changes() -> &'static tokio::sync::broadcast::Sender<EarthChange> {
    static CHANNEL: std::sync::OnceLock<tokio::sync::broadcast::Sender<EarthChange>> = std::sync::OnceLock::new();
    CHANNEL.get_or_init(|| tokio::sync::broadcast::channel(128).0)
}
fn notify(s: &Store, project: &str, scene: &Value) -> Result<(), String> {
    let _ = changes().send(EarthChange { owner: crate::agent_account::owner(s)?, project: project.into(), value: json!({"revision":scene["revision"],"cameraRevision":scene["cameraRevision"],"cameraCommandRevision":scene["cameraCommandRevision"]}) });
    Ok(())
}
fn save_camera(s: &Store, project: &str, args: &Value, command: bool) -> Result<Value, String> {
    let mut scene = get(s, project)?;
    if command {
        if args["expectedRevision"] != scene["revision"] { return Err("场景已变化，请刷新后重试".into()); }
    } else if args["expectedCameraCommandRevision"] != scene["cameraCommandRevision"] {
        return Err("定位命令已更新，请同步后重试".into());
    }
    scene["camera"] = args["camera"].clone();
    validate(&scene)?;
    scene["cameraRevision"] = json!(scene["cameraRevision"].as_u64().unwrap_or(0)+1);
    if command { scene["cameraCommandRevision"] = json!(scene["cameraCommandRevision"].as_u64().unwrap_or(0)+1); }
    let value = json!({"camera":scene["camera"],"cameraRevision":scene["cameraRevision"],"cameraCommandRevision":scene["cameraCommandRevision"],"renderedCameraCommandRevision":scene["renderedCameraCommandRevision"]});
    s.set_setting(&format!("{}:camera",key(s,project)?), &value.to_string())?;
    notify(s,project,&scene)?;
    Ok(scene)
}
pub(crate) fn validate(v: &Value) -> Result<(), String> {
    for (k, min, max) in [
        ("longitude", -180., 180.),
        ("latitude", -90., 90.),
        ("height", 20., 100_000_000.),
        ("heading", -360., 360.),
        ("tilt", 0., 75.),
    ] {
        if !v["camera"][k]
            .as_f64()
            .is_some_and(|n| n.is_finite() && n >= min && n <= max)
        {
            return Err(format!("地球相机参数 {k} 超出范围"));
        }
    }
    if !v["layers"]["boundaries"].is_boolean()
        || !v["layers"]["graticule"].is_boolean()
        || !v["layers"]["source"].as_u64().is_some_and(|n| n <= 1)
    {
        return Err("地球图层配置无效".into());
    }
    if !v["overlays"].as_array().is_some_and(|a| a.len() <= 8) || v.to_string().len() > 1_100_000 {
        return Err("最多8个业务图层，场景数据不能超过1 MiB".into());
    }
    if let Some(images)=v.get("images") {
        let images=images.as_array().ok_or("图片图层须为数组")?;
        if images.len()>8{return Err("最多8个图片图层".into());}
        for image in images{validate_polygon(&image["polygon"])?;if !image["id"].as_str().is_some_and(|id|uuid::Uuid::parse_str(id).is_ok()) || !image["visible"].is_boolean(){return Err("图片图层无效".into());}}
    }
    for layer in v["overlays"].as_array().unwrap() {
        if !layer["path"].as_str().is_some_and(|s| s.len() <= 200) || !layer["visible"].is_boolean()
        {
            return Err("业务图层字段无效".into());
        }
        validate_geojson(&layer["data"])?;
    }
    if v["selection"].to_string().len() > 20000 {
        return Err("选择信息过长".into());
    }
    Ok(())
}
pub(crate) fn save(s: &Store, project: &str, args: &Value) -> Result<Value, String> {
    let old = get(s, project)?;
    if args["expectedRevision"] != old["revision"] {
        return Err("场景已被其他页面或智能体修改，请刷新后重试".into());
    }
    let mut v = args["scene"].clone();
    validate(&v)?;
    crate::agent_earth_tiles::validate(&v["tilesets"])?;
    crate::agent_earth_models::validate(s,project,&v["models"])?;
    if let Some(images)=v["images"].as_array(){if !images.is_empty(){image_table(s)?;let owner=crate::agent_account::owner(s)?;for image in images{let valid:bool=s.conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_earth_images WHERE id=?1 AND owner=?2 AND project=?3)",rusqlite::params![image["id"].as_str(),owner,project],|r|r.get(0)).map_err(|e|e.to_string())?;if !valid{return Err("图片不属于当前账户和项目".into());}}}}
    v = json!({"camera":old["camera"],"layers":v["layers"],"overlays":v["overlays"],"images":v.get("images").cloned().unwrap_or(json!([])),"tilesets":v.get("tilesets").cloned().unwrap_or(json!([])),"models":v.get("models").cloned().unwrap_or(json!([])),"selection":v["selection"],"revision":old["revision"].as_u64().unwrap_or(0)+1,"renderedRevision":null});
    s.set_setting(&key(s, project)?, &v.to_string())?;
    let v = get(s, project)?;
    notify(s, project, &v)?;
    Ok(v)
}
pub(crate) fn invoke(s: &Store, command: &str, args: &Value) -> Result<Value, String> {
    let project = args["projectId"].as_str().ok_or("缺少项目")?;
    if (command != "agent_earth_get" || args.get("accountKey").is_some())
        && args["accountKey"].as_str() != Some(&crate::agent_account::owner(s)?)
    {
        return Err("账户已切换，请重新打开地球页面".into());
    }
    match command {
        "agent_earth_add_tileset" => {let mut scene=get(s,project)?;crate::agent_earth_tiles::add(&mut scene,args)?;save(s,project,&json!({"expectedRevision":args["expectedRevision"],"scene":scene}))}
        "agent_earth_tiles_status" => Err("请使用带窗口ID的图层报告".into()),
        "agent_earth_add_model" | "agent_earth_publish_model" | "agent_earth_import" | "agent_earth_publish_image" | "agent_earth_add_image" => {
            let directory: String = s
                .conn
                .query_row(
                    "SELECT directory FROM agent_projects WHERE id=?1",
                    [project],
                    |r| r.get(0),
                )
                .map_err(|_| "项目不存在")?;
            let root = cap_std::fs::Dir::open_ambient_dir(directory, cap_std::ambient_authority())
                .map_err(|_| "无法打开项目目录")?;
            let path = args["path"].as_str().ok_or("缺少路径")?;
            if command=="agent_earth_publish_model"{return crate::agent_earth_models::publish(s,project,&root,path);}
            if command=="agent_earth_publish_image"{return publish_image(s,project,&root,path);}
            let mut scene = get(s, project)?;
            if command=="agent_earth_add_model"{crate::agent_earth_models::add(s,project,&root,&mut scene,args)?;}else if command=="agent_earth_add_image"{add_image(s,project,&mut scene,&root,path,&args["polygon"])?;}else{add_layer(&mut scene, &root, path)?;}
            save(
                s,
                project,
                &json!({"expectedRevision":args["expectedRevision"],"scene":scene}),
            )
        }
        "agent_earth_fly_to" => {
            let mut scene = get(s, project)?;
            set_location(&mut scene, args)?;
            save_camera(s, project, &json!({"expectedRevision":args["expectedRevision"],"camera":scene["camera"]}), true)
        }
        "agent_earth_camera_save" => save_camera(s, project, args, false),
        "agent_earth_get" => get(s, project),
        "agent_earth_save" => save(s, project, args),
        "agent_earth_ack" => crate::agent_earth_receipts::report(s,project,args,&get(s,project)?),
        _ => Err("未知地球操作".into()),
    }
}
fn set_location(scene: &mut Value, args: &Value) -> Result<(), String> {
    scene["camera"]["longitude"] = args["longitude"].clone();
    scene["camera"]["latitude"] = args["latitude"].clone();
    if let Some(height) = args.get("height") { scene["camera"]["height"] = height.clone(); }
    validate(scene)
}
pub(crate) fn definitions() -> Vec<Value> {
    let n = |min: f64, max: f64| json!({"type":"number","minimum":min,"maximum":max});
    [("earth_search_datasets","查询已配置Cangling Map公开发布的3D Tiles数据集，按名称关键词或WGS84范围筛选。返回真实ID、版本、覆盖范围及URL。范围未知时不猜测；无结果不能编造数据集。",json!({"query":{"type":"string","maxLength":128},"bounds":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"number"}}}),json!([])),
    ("earth_load_dataset","按查询所得数据集ID加载当前发布版本到当前账户项目。重复调用同一版本复用图层；不自动定位，使用返回范围调用earth_fly_to，再用earth_get_view确认tilesetStatus。保存不代表绘制成功。",json!({"datasetId":{"type":"string","minLength":1,"maxLength":64}}),json!(["datasetId"])),
    ("earth_remove_tileset","按图层ID移除当前项目的3D Tiles场景引用，不删除服务端数据。",json!({"layer":{"type":"string","maxLength":64}}),json!(["layer"])),
    ("earth_add_tileset","添加原生3D Tiles图层。URL为允许CORS的公开HTTP(S) tileset.json（如Cangling Map发布地址）；支持显式瓦片树、GLB/b3dm，不支持隐式瓦片及点云。浏览器异步LOD加载；earth_get_view的windows逐窗口报告加载、绘制提交及错误，sceneApplied只代表场景已应用，不能据此声称瓦片已显示。",json!({"url":{"type":"string","maxLength":2048},"name":{"type":"string","maxLength":128},"maximumScreenSpaceError":n(2.,128.)}),json!(["url"])),
    ("earth_publish_model","发布项目内静态GLB下载URL，内嵌纹理/缓冲区，最多16MiB，当前账户本机可访问。",json!({"path":{"type":"string","maxLength":200}}),json!(["path"])),
    ("earth_add_model","将项目静态GLB加载到WGS84经纬度，height米默认0，altitudeMode默认relativeToGround离地高，absolute为椭球高；heading度默认0，scale默认1（米），GLB的Y轴朝上。保存后等地球页面确认。",json!({"path":{"type":"string","maxLength":200},"longitude":n(-180.,180.),"latitude":n(-90.,90.),"height":n(-10000.,100000.),"heading":n(-360.,360.),"scale":n(0.001,10000.),"altitudeMode":{"type":"string","enum":["relativeToGround","absolute"]}}),json!(["path","longitude","latitude"])),
    ("earth_get_view","读取已保存视图及各Earth窗口独立的应用和图层状态。windows按windowId区分；online和layerReportCurrent必须为true才能使用报告。sceneApplied不代表加载完成；draw_submitted只证明该窗口最近帧提交绘制，不保证屏幕像素可见。多个窗口必须指明哪个窗口，不得用一个窗口替其他窗口确认。",json!({"windowId":{"type":"string","description":"可选：指定Earth窗口ID；不指定时列出所有窗口，不推断成功"}}),json!([])),
    ("earth_fly_to","设置项目地球定位，经纬度为WGS84度，高度为米，可省略以保留当前高度；保存后由打开的地球页面应用。结果saved_not_rendered不是已完成渲染。",json!({"longitude":n(-180.,180.),"latitude":n(-90.,90.),"height":n(20.,100000000.)}),json!(["longitude","latitude"])),
    ("earth_set_layer_visibility","设置行政边界boundaries、经纬网graticule、已导入图层路径或图片/模型图层ID的可见性。",json!({"layer":{"type":"string","maxLength":200},"visible":{"type":"boolean"}}),json!(["layer","visible"])),
    ("earth_publish_image","将项目PNG/JPEG/WebP图片发布为当前账户可下载的本机URL快照，最多4MiB；不公开到互联网。",json!({"path":{"type":"string","maxLength":200}}),json!(["path"])),
    ("earth_add_image_layer","将项目PNG/JPEG/WebP图片叠加到WGS84多边形。polygon为3至32个不自交的经纬度顶点（可闭合），不跨日期变更线。图片北向上铺满外接矩形后按多边形裁剪，不是自动地理配准；最多8层。",json!({"path":{"type":"string","maxLength":200},"polygon":{"type":"array","minItems":3,"maxItems":33,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}}}}),json!(["path","polygon"])),
    ("earth_get_selection","读取地球页面最后选择的对象/坐标，不推断未选中的地理对象。",json!({}),json!([])),
    ("earth_add_geojson_layer","从项目相对路径加载WGS84 GeoJSON，支持点线及多边形轮廓。最多500要素、5000坐标、1MiB；不执行数据内的HTML或脚本。",json!({"path":{"type":"string","minLength":1,"maxLength":200}}),json!(["path"]))]
    .into_iter().map(|(name,description,properties,required)|json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})).collect()
}
pub(crate) fn validate_geojson(v: &Value) -> Result<(), String> {
    fn coords(v: &Value, depth: usize, count: &mut usize) -> Result<(), String> {
        let a = v.as_array().ok_or("GeoJSON 坐标必须为数组")?;
        if depth == 0 {
            if a.len() < 2 || a.len() > 3 {
                return Err("坐标须为经度纬度及可选高度".into());
            }
            for (i, item) in a.iter().enumerate() {
                let n = item
                    .as_f64()
                    .filter(|n| n.is_finite())
                    .ok_or("坐标须为有限数值")?;
                if (i == 0 && n.abs() > 180.)
                    || (i == 1 && n.abs() > 90.)
                    || (i == 2 && n.abs() > 100000.)
                {
                    return Err("坐标超出WGS84范围".into());
                }
            }
            *count += 1;
            if *count > 5000 {
                return Err("最多5000个坐标".into());
            }
        } else {
            if a.len() > 5000 {
                return Err("坐标列表过长".into());
            }
            for p in a {
                coords(p, depth - 1, count)?;
            }
        }
        Ok(())
    }
    if v.to_string().len() > 1_000_000 {
        return Err("GeoJSON 超过1 MiB".into());
    }
    if v.get("crs").is_some() {
        return Err("请先转换为无crs字段的WGS84 GeoJSON".into());
    }
    let features = if v["type"] == "FeatureCollection" {
        v["features"].as_array().ok_or("缺少features")?.clone()
    } else {
        vec![v.clone()]
    };
    if features.len() > 500 {
        return Err("最多500个要素".into());
    }
    let mut count = 0;
    for f in features {
        let g = if f["type"] == "Feature" {
            &f["geometry"]
        } else {
            &f
        };
        if f.get("crs").is_some() || g.get("crs").is_some() {
            return Err("GeoJSON 必须使用WGS84".into());
        }
        let depth = match g["type"].as_str() {
            Some("Point") => 0,
            Some("MultiPoint" | "LineString") => 1,
            Some("MultiLineString" | "Polygon") => 2,
            Some("MultiPolygon") => 3,
            _ => return Err("仅支持点线面GeoJSON，不支持GeometryCollection".into()),
        };
        coords(&g["coordinates"], depth, &mut count)?;
    }
    Ok(())
}
pub(crate) fn execute(
    s: &Store,
    session: &str,
    tool: &str,
    args: &Value,
    root: &cap_std::fs::Dir,
) -> Result<Value, String> {
    crate::agent_history::require_active(s, session)?;
    let project: String = s
        .conn
        .query_row(
            "SELECT project_id FROM agent_sessions WHERE id=?1",
            [session],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let mut scene = get(s, &project)?;
    match tool {
        "earth_get_view" => {
            return Ok(
                json!({"camera":scene["camera"],"layers":scene["layers"],"tilesetLayers":scene["tilesets"],"tilesetStatus":scene["tilesetStatus"],"modelLayers":scene.get("models").cloned().unwrap_or(json!([])),"imageLayers":scene.get("images").cloned().unwrap_or(json!([])),"businessLayers":scene["overlays"].as_array().unwrap().iter().map(|v|json!({"path":v["path"],"visible":v["visible"]})).collect::<Vec<_>>(),"windows":scene["windows"],"targetWindow":args["windowId"].as_str().and_then(|id|scene["windows"].as_array()?.iter().find(|v|v["windowId"]==id)),"evidence":"逐窗口核对在线、版本和图层状态；绘制提交不保证遮挡后像素可见，报告不含基础底图完整加载确认", "revision":scene["revision"],"renderedRevision":scene["renderedRevision"],"cameraRevision":scene["cameraRevision"],"cameraCommandRevision":scene["cameraCommandRevision"],"renderedCameraCommandRevision":scene["renderedCameraCommandRevision"],"renderedAt":scene["renderedAt"]}),
            );
        }
        "earth_get_selection" => {
            return Ok(json!({"selection":scene["selection"],"renderedAt":scene["renderedAt"]}));
        }
        "earth_publish_model" => {return crate::agent_earth_models::publish(s,&project,root,args["path"].as_str().ok_or("缺少路径")?);}
        "earth_load_dataset" => {crate::agent_earth_catalog::load(&mut scene,&args["_catalog"],args["datasetId"].as_str().ok_or("缺少数据集ID")?)?;}
        "earth_remove_tileset" => {let id=args["layer"].as_str().ok_or("缺少图层ID")?;let layers=scene["tilesets"].as_array_mut().ok_or("没有3D Tiles图层")?;if !layers.iter().any(|l|l["id"]==id){return Err("图层不存在".into());}layers.retain(|l|l["id"]!=id);}
        "earth_add_tileset" => {crate::agent_earth_tiles::add(&mut scene,args)?;}
        "earth_add_model" => {crate::agent_earth_models::add(s,&project,root,&mut scene,args)?;}
        "earth_publish_image" => {return publish_image(s,&project,root,args["path"].as_str().ok_or("缺少路径")?);}
        "earth_add_image_layer" => {add_image(s,&project,&mut scene,root,args["path"].as_str().ok_or("缺少路径")?,&args["polygon"])?;}
        "earth_fly_to" => {
            set_location(&mut scene, args)?;
        }
        "earth_set_layer_visibility" => {
            let layer = args["layer"].as_str().ok_or("缺少图层")?;
            if ["boundaries", "graticule"].contains(&layer) {
                scene["layers"][layer] = args["visible"].clone();
            } else if scene["tilesets"].as_array().is_some_and(|a|a.iter().any(|v|v["id"]==layer)){scene["tilesets"].as_array_mut().unwrap().iter_mut().find(|v|v["id"]==layer).unwrap()["visible"]=args["visible"].clone();
            } else if scene.get("models").and_then(Value::as_array).is_some_and(|a|a.iter().any(|v|v["id"]==layer)){scene["models"].as_array_mut().unwrap().iter_mut().find(|v|v["id"]==layer).unwrap()["visible"]=args["visible"].clone();
            } else if scene.get("images").and_then(Value::as_array).is_some_and(|images|images.iter().any(|v|v["id"]==layer)) {
                let image=scene["images"].as_array_mut().unwrap().iter_mut().find(|v|v["id"]==layer).unwrap();image["visible"]=args["visible"].clone();
            } else {
                let item = scene["overlays"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|v| v["path"] == layer)
                    .ok_or("图层不存在")?;
                item["visible"] = args["visible"].clone();
            }
        }
        "earth_add_geojson_layer" => {
            let path = args["path"].as_str().ok_or("缺少路径")?;
            add_layer(&mut scene, root, path)?;
        }
        _ => return Err("未知地球工具".into()),
    }
    let saved = if tool == "earth_fly_to" { save_camera(s, &project, &json!({"expectedRevision":scene["revision"],"camera":scene["camera"]}), true)? } else { save(
        s,
        &project,
        &json!({"expectedRevision":scene["revision"],"scene":scene}),
    )? };
    Ok(
        json!({"status":"saved_not_rendered","projectId":project,"tilesetLayers":saved["tilesets"],"revision":saved["revision"],"cameraCommandRevision":saved["cameraCommandRevision"],"message":"场景已保存；打开项目三维地球后应用。请用 earth_get_view 按 windowId 核对 online、sceneApplied、cameraApplied 和 layerReportCurrent，再逐图层核对状态；未确认不可声称已显示。"}),
    )
}

fn add_layer(scene: &mut Value, root: &cap_std::fs::Dir, path: &str) -> Result<(), String> {
    let text = crate::agent_files::editor_text(root, path)?;
    let data: Value = serde_json::from_str(text["content"].as_str().ok_or("文件不可读")?)
        .map_err(|_| "文件不是有效GeoJSON")?;
    validate_geojson(&data)?;
    let list = scene["overlays"].as_array_mut().ok_or("图层无效")?;
    list.retain(|v| v["path"] != path);
    list.push(json!({"path":path,"visible":true,"data":data}));
    Ok(())
}

fn image_table(s:&Store)->Result<(),String>{s.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_earth_images(id TEXT PRIMARY KEY,owner TEXT NOT NULL,project TEXT NOT NULL,name TEXT NOT NULL,mime TEXT NOT NULL,sha TEXT NOT NULL,data BLOB NOT NULL,UNIQUE(owner,project,sha));").map_err(|e|e.to_string())}
pub(crate) fn image_data(s:&Store,id:&str)->Result<(String,Vec<u8>),String>{
    image_table(s)?;let owner=crate::agent_account::owner(s)?;
    s.conn.query_row("SELECT mime,data FROM agent_earth_images WHERE id=?1 AND owner=?2",rusqlite::params![id,owner],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|"图片不存在或不属于当前账户".into())
}
fn publish_image(s:&Store,project:&str,root:&cap_std::fs::Dir,path:&str)->Result<Value,String>{
    let bytes=crate::agent_files::image_bytes(root,path)?;
    let name=std::path::Path::new(path).file_name().and_then(|n|n.to_str()).ok_or("无效文件名")?;
    let meta=crate::agent_attachments::validate(name,"application/octet-stream",&bytes)?;
    if meta["kind"]!="image"{return Err("仅支持PNG/JPEG/WebP，请先转换TIFF".into());}
    image_table(s)?;let owner=crate::agent_account::owner(s)?;let sha=crate::agent_actions::hash(&bytes);
    let old:Option<String>=s.conn.query_row("SELECT id FROM agent_earth_images WHERE owner=?1 AND project=?2 AND sha=?3",rusqlite::params![owner,project,sha],|r|r.get(0)).ok();
    let id=if let Some(id)=old{id}else{
        let total:i64=s.conn.query_row("SELECT COALESCE(SUM(length(data)),0) FROM agent_earth_images WHERE owner=?1",[&owner],|r|r.get(0)).map_err(|e|e.to_string())?;
        if total+bytes.len() as i64>128*1024*1024{return Err("当前账户图片快照超过128 MiB上限".into());}
        let id=uuid::Uuid::new_v4().to_string();s.conn.execute("INSERT INTO agent_earth_images VALUES (?1,?2,?3,?4,?5,?6,?7)",rusqlite::params![id,owner,project,name,meta["mimeType"].as_str().unwrap(),sha,bytes]).map_err(|e|e.to_string())?;id
    };
    Ok(json!({"id":id,"url":format!("/api/earth/images/{id}"),"downloadUrl":format!("/api/earth/images/{id}?download=1"),"name":name,"width":meta["width"],"height":meta["height"],"sha256":sha,"scope":"current_account_localhost"}))
}
fn validate_polygon(value:&Value)->Result<Vec<[f64;2]>,String>{
    let list=value.as_array().ok_or("polygon须为经纬度数组")?;
    if !(3..=33).contains(&list.len()){return Err("多边形须为3至32个顶点".into());}
    let mut pts=Vec::new();for p in list {let a=p.as_array().filter(|a|a.len()==2).ok_or("坐标须为经度、纬度")?;let x=a[0].as_f64().filter(|x|x.is_finite()&&x.abs()<=180.).ok_or("经度无效")?;let y=a[1].as_f64().filter(|y|y.is_finite()&&y.abs()<=90.).ok_or("纬度无效")?;pts.push([x,y]);}
    if pts.first()==pts.last(){pts.pop();}if !(3..=32).contains(&pts.len()){return Err("多边形顶点数无效".into());}
    let xmin=pts.iter().map(|p|p[0]).fold(f64::INFINITY,f64::min);let xmax=pts.iter().map(|p|p[0]).fold(f64::NEG_INFINITY,f64::max);
    if xmax-xmin>180.{return Err("暂不支持跨日期变更线的图片范围".into());}
    fn cross(a:[f64;2],b:[f64;2],c:[f64;2])->f64{(b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0])}
    let n=pts.len();let mut area=0.;for i in 0..n{let a=pts[i];let b=pts[(i+1)%n];if a==b{return Err("多边形含重复点".into());}area+=a[0]*b[1]-b[0]*a[1];for j in i+1..n{if j==i+1||(i==0&&j==n-1){continue;}let c=pts[j];let d=pts[(j+1)%n];if a[0].min(b[0])<=c[0].max(d[0]) && c[0].min(d[0])<=a[0].max(b[0]) && a[1].min(b[1])<=c[1].max(d[1]) && c[1].min(d[1])<=a[1].max(b[1]) && cross(a,b,c)*cross(a,b,d)<=0. && cross(c,d,a)*cross(c,d,b)<=0.{return Err("多边形自交或重叠".into());}}}
    if area.abs()<1e-12{return Err("多边形面积为零".into());}Ok(pts)
}
fn add_image(s:&Store,project:&str,scene:&mut Value,root:&cap_std::fs::Dir,path:&str,polygon:&Value)->Result<(),String>{
    let points=validate_polygon(polygon)?;
    if scene.get("images").is_none(){scene["images"]=json!([]);}
    let images=scene["images"].as_array_mut().ok_or("图片图层无效")?;
    let asset=publish_image(s,project,root,path)?;
    if images.len()>=8 && !images.iter().any(|v|v["id"]==asset["id"]){return Err("最多8个图片图层".into());}
    images.retain(|v|v["id"]!=asset["id"]);
    images.push(json!({"id":asset["id"],"name":asset["name"],"polygon":points,"visible":true}));Ok(())
}
