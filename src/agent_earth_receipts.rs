//! Per-viewer receipts. Legacy project-wide acknowledgements are never evidence.
use crate::store::Store;
use serde_json::{Value, json};
const LEASE_MS: i64 = 45_000;
fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
fn storage(s: &Store, project: &str) -> Result<String, String> {
    Ok(format!("{}:viewers", super::agent_earth::key(s, project)?))
}
fn records(s: &Store, project: &str) -> Result<Value, String> {
    s.get_setting(&storage(s, project)?)?
        .map(|v| serde_json::from_str(&v).map_err(|e| e.to_string()))
        .unwrap_or_else(|| Ok(json!({})))
}
pub(crate) fn views(s: &Store, project: &str, scene: &Value) -> Result<Value, String> {
    let values = records(s, project)?;
    let mut result = Vec::new();
    for (id, record) in values.as_object().ok_or("无效窗口记录")? {
        let mut v = record.clone();
        let fresh = v["closed"] != true && now() - v["lastSeen"].as_i64().unwrap_or(0) <= LEASE_MS;
        let applied = fresh && v["appliedRevision"] == scene["revision"];
        let camera = applied && v["appliedCameraCommandRevision"] == scene["cameraCommandRevision"];
        let report_current = applied
            && v["layerRevision"] == scene["revision"]
            && v["layerCameraCommandRevision"] == scene["cameraCommandRevision"]
            && now() - v["reportedAt"].as_i64().unwrap_or(0) <= LEASE_MS;
        v["windowId"] = json!(id);
        v["online"] = json!(fresh);
        v["sceneApplied"] = json!(applied);
        v["cameraApplied"] = json!(camera);
        v["layerReportCurrent"] = json!(report_current);
        v["status"] = json!(if !fresh {
            "offline"
        } else if !applied {
            "pending_apply"
        } else if !report_current {
            "scene_applied"
        } else {
            "layers_reported"
        });
        if !report_current {
            v["layers"] = json!([]);
        }
        result.push(v);
    }
    result.sort_by_key(|v| v["windowId"].as_str().unwrap_or("").to_owned());
    Ok(json!(result))
}
pub(crate) fn report(
    s: &Store,
    project: &str,
    args: &Value,
    scene: &Value,
) -> Result<Value, String> {
    let id = args["windowId"]
        .as_str()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .ok_or("缺少有效Earth窗口ID，请刷新地球页面")?;
    let phase = args["phase"].as_str().unwrap_or("applied");
    let mut all = records(s, project)?;
    let map = all.as_object_mut().ok_or("无效窗口记录")?;
    map.retain(|_, v| now() - v["lastSeen"].as_i64().unwrap_or(0) < 86_400_000);
    if phase == "open" {
        if map.contains_key(id) {
            return Err("窗口ID已使用，请重新打开地球页面".into());
        }
        if map.len() >= 32 {
            let oldest = map
                .iter()
                .min_by_key(|(_, v)| v["lastSeen"].as_i64().unwrap_or(0))
                .map(|(id, _)| id.clone())
                .unwrap();
            map.remove(&oldest);
        }
        map.insert(id.into(),json!({"lastSeen":now(),"closed":false,"appliedRevision":null,"appliedCameraCommandRevision":null,"layers":[]}));
    } else {
        let v = map.get_mut(id).ok_or("Earth窗口未注册或已过期")?;
        if v["closed"] == true {
            return Err("Earth窗口已关闭".into());
        }
        if phase == "closed" {
            v["closed"] = json!(true);
        } else {
            if args["revision"] != scene["revision"]
                || args["cameraCommandRevision"] != scene["cameraCommandRevision"]
            {
                return Err("场景或定位版本已变化".into());
            }
            match phase {
                "applied" => {
                    v["appliedRevision"] = scene["revision"].clone();
                    v["appliedCameraCommandRevision"] = scene["cameraCommandRevision"].clone();
                    v["appliedAt"] = json!(now());
                    v["layerRevision"] = Value::Null;
                    v["layers"] = json!([]);
                }
                "layers" => {
                    if v["appliedRevision"] != scene["revision"]
                        || v["appliedCameraCommandRevision"] != scene["cameraCommandRevision"]
                    {
                        return Err("当前窗口尚未应用该版本".into());
                    }
                    let layers = args["layers"]
                        .as_array()
                        .filter(|a| a.len() <= 32)
                        .ok_or("无效图层报告")?;
                    if args["layers"].to_string().len() > 32768 {
                        return Err("图层报告过大".into());
                    }
                    let mut clean = Vec::new();
                    let mut seen = std::collections::HashSet::new();
                    for layer in layers {
                        let kind = layer["kind"]
                            .as_str()
                            .filter(|k| ["images", "models", "overlays", "tilesets"].contains(k))
                            .ok_or("无效图层类型")?;
                        let id = layer["id"].as_str().ok_or("缺少图层ID")?;
                        if !seen.insert((kind, id)) {
                            return Err("重复图层报告".into());
                        }
                        let field = if kind == "overlays" { "path" } else { "id" };
                        let source = scene[kind]
                            .as_array()
                            .and_then(|a| a.iter().find(|v| v[field] == id))
                            .ok_or("未知图层")?;
                        let state = layer["state"]
                            .as_str()
                            .filter(|s| {
                                ["loading", "loaded", "draw_submitted", "error", "hidden"]
                                    .contains(s)
                            })
                            .ok_or("无效图层状态")?;
                        if (source["visible"] == false) != (state == "hidden") {
                            return Err("图层可见状态不匹配".into());
                        }
                        if state == "draw_submitted" && layer["drawing"].as_u64().unwrap_or(0) == 0
                        {
                            return Err("缺少绘制提交证据".into());
                        }
                        clean.push(json!({"kind":kind,"id":id,"state":state,"pending":layer["pending"].as_u64().unwrap_or(0),"drawing":layer["drawing"].as_u64().unwrap_or(0),"error":layer["error"].as_str().unwrap_or("").chars().take(500).collect::<String>()}));
                    }
                    v["layers"] = json!(clean);
                    v["layerRevision"] = scene["revision"].clone();
                    v["layerCameraCommandRevision"] = scene["cameraCommandRevision"].clone();
                    v["reportedAt"] = json!(now());
                }
                _ => return Err("未知窗口报告阶段".into()),
            }
        }
        v["lastSeen"] = json!(now());
    }
    let result = map[id].clone();
    s.set_setting(&storage(s, project)?, &all.to_string())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_windows_and_stale_receipts() {
        let path =
            std::env::temp_dir().join(format!("earth-receipts-{}.sqlite", uuid::Uuid::new_v4()));
        let s = Store::load(path.clone()).unwrap();
        crate::agent::initialize(&s).unwrap();
        let scene = json!({"revision":1,"cameraCommandRevision":0,"images":[{"id":"image","visible":true}]});
        let a = uuid::Uuid::new_v4().to_string();
        let b = uuid::Uuid::new_v4().to_string();
        for id in [&a, &b] {
            report(
                &s,
                "general",
                &json!({"windowId":id,"phase":"open"}),
                &scene,
            )
            .unwrap();
        }
        let applied =
            json!({"windowId":a,"phase":"applied","revision":1,"cameraCommandRevision":0});
        report(&s, "general", &applied, &scene).unwrap();
        let v = views(&s, "general", &scene).unwrap();
        assert_eq!(
            v.as_array()
                .unwrap()
                .iter()
                .find(|v| v["windowId"] == a)
                .unwrap()["sceneApplied"],
            true
        );
        assert_eq!(
            v.as_array()
                .unwrap()
                .iter()
                .find(|v| v["windowId"] == b)
                .unwrap()["sceneApplied"],
            false
        );
        assert!(
            v.as_array()
                .unwrap()
                .iter()
                .all(|v| v["layerReportCurrent"] == false)
        );
        let mut layers = json!({"windowId":a,"phase":"layers","revision":1,"cameraCommandRevision":0,"layers":[{"kind":"images","id":"image","state":"draw_submitted","drawing":1}]});
        report(&s, "general", &layers, &scene).unwrap();
        layers["windowId"] = json!(b);
        assert!(report(&s, "general", &layers, &scene).is_err());
        let changed = json!({"revision":2,"cameraCommandRevision":0});
        assert!(report(&s, "general", &applied, &changed).is_err());
        assert!(
            views(&s, "general", &changed)
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v["layerReportCurrent"] == false)
        );
        report(
            &s,
            "general",
            &json!({"windowId":a,"phase":"closed"}),
            &scene,
        )
        .unwrap();
        assert!(report(&s, "general", &applied, &scene).is_err());
        let mut all = records(&s, "general").unwrap();
        all[&b]["lastSeen"] = json!(now() - LEASE_MS - 1);
        s.set_setting(&storage(&s, "general").unwrap(), &all.to_string())
            .unwrap();
        assert!(
            views(&s, "general", &scene)
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v["online"] == false && v["layerReportCurrent"] == false)
        );
        s.set_setting("login_token", "receipt-test-only").unwrap();
        s.set_setting("login_username", "other").unwrap();
        s.set_setting("server_url", "http://localhost").unwrap();
        assert!(
            views(&s, "general", &scene)
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(report(&s, "general", &applied, &scene).is_err());
        drop(s);
        let _ = std::fs::remove_file(path);
    }
}
