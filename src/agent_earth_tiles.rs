//! Persisted 3D Tiles references; browser transports public CORS resources, never a server proxy.
use serde_json::{Value, json};
pub(crate) fn validate(v: &Value) -> Result<(), String> {
    if v.is_null() {
        return Ok(());
    }
    let layers = v
        .as_array()
        .filter(|a| a.len() <= 4)
        .ok_or("最多4个3D Tiles图层")?;
    let mut ids = std::collections::HashSet::new();
    for l in layers {
        let id = l["id"].as_str().ok_or("缺少图层ID")?;
        if uuid::Uuid::parse_str(id).is_err() || !ids.insert(id) {
            return Err("无效或重复的图层ID".into());
        }
        url(l["url"].as_str().ok_or("缺少tileset URL")?)?;
        if !l["visible"].is_boolean()
            || !l["maximumScreenSpaceError"]
                .as_f64()
                .is_some_and(|x| (2.0..=128.0).contains(&x))
            || !l["name"]
                .as_str()
                .is_some_and(|s| !s.is_empty() && s.chars().count() <= 128)
        {
            return Err("图层名称、可见性或屏幕误差无效".into());
        }
    }
    Ok(())
}
fn url(raw: &str) -> Result<(), String> {
    let u = reqwest::Url::parse(raw).map_err(|_| "tileset URL无效")?;
    if raw.len() > 2048
        || raw.chars().any(|c| c.is_control())
        || raw.contains('%')
        || raw.contains('\\')
        || raw.split('/').any(|s| s == ".." || s == ".")
        || !matches!(u.scheme(), "http" | "https")
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || !u.path().ends_with("/tileset.json")
    {
        return Err("使用无凭证、无查询参数的HTTP(S) tileset.json URL；资源服务须允许CORS".into());
    }
    Ok(())
}
pub(crate) fn add(scene: &mut Value, args: &Value) -> Result<(), String> {
    let u = args["url"].as_str().ok_or("缺少tileset URL")?;
    url(u)?;
    let mut layers = scene["tilesets"].as_array().cloned().unwrap_or_default();
    if layers.iter().any(|l| l["url"] == u) {
        return Err("该tileset已添加".into());
    }
    layers.push(json!({"id":uuid::Uuid::new_v4().to_string(),"url":u,"name":args["name"].as_str().unwrap_or("3D Tiles"),"visible":true,"maximumScreenSpaceError":args["maximumScreenSpaceError"].as_f64().unwrap_or(16.)}));
    let v = json!(layers);
    validate(&v)?;
    scene["tilesets"] = v;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_sources_and_limits() {
        for bad in [
            "file:///tmp/tileset.json",
            "https://u:p@host/tileset.json",
            "https://host/a/../tileset.json",
            "https://host/tileset.json?token=x",
        ] {
            assert!(url(bad).is_err());
        }
        let mut s = json!({});
        add(
            &mut s,
            &json!({"url":"http://127.0.0.1:8080/3dtiles/demo/tileset.json"}),
        )
        .unwrap();
        assert!(
            add(
                &mut s,
                &json!({"url":"http://127.0.0.1:8080/3dtiles/demo/tileset.json"})
            )
            .is_err()
        );
        assert_eq!(s["tilesets"][0]["maximumScreenSpaceError"], 16.);
    }
}
