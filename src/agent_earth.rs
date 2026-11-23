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
    match s.get_setting(&key)? {
        Some(raw) => serde_json::from_str(&raw).map_err(|e| e.to_string()),
        None => Ok(
            json!({"revision":0,"camera":{"longitude":108.9398,"latitude":34.3416,"height":16000000.,"heading":0.,"tilt":0.},"layers":{"boundaries":true,"graticule":false,"source":0},"overlays":[],"selection":null,"renderedRevision":null}),
        ),
    }
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
    v = json!({"camera":v["camera"],"layers":v["layers"],"overlays":v["overlays"],"selection":v["selection"],"revision":old["revision"].as_u64().unwrap_or(0)+1,"renderedRevision":null});
    s.set_setting(&key(s, project)?, &v.to_string())?;
    Ok(v)
}
pub(crate) fn invoke(s: &Store, command: &str, args: &Value) -> Result<Value, String> {
    let project = args["projectId"].as_str().ok_or("缺少项目")?;
    if command != "agent_earth_get"
        && args["accountKey"].as_str() != Some(&crate::agent_account::owner(s)?)
    {
        return Err("账户已切换，请重新打开地球页面".into());
    }
    match command {
        "agent_earth_import" => {
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
            let mut scene = get(s, project)?;
            add_layer(&mut scene, &root, path)?;
            save(
                s,
                project,
                &json!({"expectedRevision":args["expectedRevision"],"scene":scene}),
            )
        }
        "agent_earth_get" => get(s, project),
        "agent_earth_save" => save(s, project, args),
        "agent_earth_ack" => {
            let mut v = get(s, project)?;
            if args["revision"] != v["revision"] {
                return Err("场景版本已变化".into());
            }
            v["renderedRevision"] = v["revision"].clone();
            v["renderedAt"] = json!(chrono::Utc::now().timestamp_millis());
            s.set_setting(&key(s, project)?, &v.to_string())?;
            Ok(v)
        }
        _ => Err("未知地球操作".into()),
    }
}
pub(crate) fn definitions() -> Vec<Value> {
    let n = |min: f64, max: f64| json!({"type":"number","minimum":min,"maximum":max});
    [("earth_get_view","读取当前项目已保存地球视图、图层及浏览器最后渲染确认时间。未确认不代表地球已显示。",json!({}),json!([])),
    ("earth_fly_to","设置项目地球定位，经纬度为WGS84度，高度为米；保存后由打开的地球页面应用。结果saved_not_rendered不是已完成渲染。",json!({"longitude":n(-180.,180.),"latitude":n(-90.,90.),"height":n(20.,100000000.)}),json!(["longitude","latitude","height"])),
    ("earth_set_layer_visibility","设置行政边界boundaries、经纬网graticule或已导入图层路径的可见性。",json!({"layer":{"type":"string","maxLength":200},"visible":{"type":"boolean"}}),json!(["layer","visible"])),
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
                json!({"camera":scene["camera"],"layers":scene["layers"],"businessLayers":scene["overlays"].as_array().unwrap().iter().map(|v|json!({"path":v["path"],"visible":v["visible"]})).collect::<Vec<_>>(),"revision":scene["revision"],"renderedRevision":scene["renderedRevision"],"renderedAt":scene["renderedAt"]}),
            );
        }
        "earth_get_selection" => {
            return Ok(json!({"selection":scene["selection"],"renderedAt":scene["renderedAt"]}));
        }
        "earth_fly_to" => {
            scene["camera"] = json!({"longitude":args["longitude"],"latitude":args["latitude"],"height":args["height"],"tilt":0.,"heading":0.});
        }
        "earth_set_layer_visibility" => {
            let layer = args["layer"].as_str().ok_or("缺少图层")?;
            if ["boundaries", "graticule"].contains(&layer) {
                scene["layers"][layer] = args["visible"].clone();
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
    let saved = save(
        s,
        &project,
        &json!({"expectedRevision":scene["revision"],"scene":scene}),
    )?;
    Ok(
        json!({"status":"saved_not_rendered","projectId":project,"revision":saved["revision"],"message":"场景已保存；打开项目三维地球后应用。请用 earth_get_view 核对 renderedRevision，不能声称已经显示。"}),
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
