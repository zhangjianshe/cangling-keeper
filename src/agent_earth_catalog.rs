//! Read-only catalog transport. The model cannot choose an arbitrary fetch destination.
use serde_json::{Value, json};

fn base_url() -> Result<reqwest::Url, String> {
    let raw = std::env::var("KEEPER_EARTH_MAP_URL")
        .unwrap_or_else(|_| "https://train.cangling.cn/geoserver/".into());
    let url = reqwest::Url::parse(&format!("{}/", raw.trim_end_matches('/')))
        .map_err(|_| "Map服务地址无效")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Map服务必须是无凭证HTTP(S)地址".into());
    }
    Ok(url)
}
pub(crate) async fn fetch(args: &Value) -> Result<Value, String> {
    let base = base_url()?;
    let mut url = base
        .join("api/3dtiles/discover")
        .map_err(|e| e.to_string())?;
    {
        let mut query = url.query_pairs_mut();
        for (input, key) in [("query", "q"), ("datasetId", "id")] {
            if let Some(s) = args[input].as_str() {
                query.append_pair(key, s);
            }
        }
        if let Some(b) = args["bounds"].as_array() {
            query.append_pair(
                "bbox",
                &b.iter().map(Value::to_string).collect::<Vec<_>>().join(","),
            );
        }
        query.append_pair("limit", "100");
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("查询Map数据集失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Map数据集目录返回HTTP {}；请确认服务已升级并配置KEEPER_EARTH_MAP_URL",
            response.status()
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err("数据集目录超过1MiB".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let data: Value = serde_json::from_slice(&bytes).map_err(|_| "Map数据集目录不是JSON")?;
    normalize(&base, &data)
}
fn normalize(base: &reqwest::Url, data: &Value) -> Result<Value, String> {
    let entries = data["datasets"]
        .as_array()
        .filter(|a| a.len() <= 100)
        .ok_or("无效的数据集目录")?;
    let mut result = Vec::new();
    for d in entries {
        let id = d["id"].as_str().ok_or("数据集缺少ID")?;
        let revision = d["version"].as_str().ok_or("数据集缺少版本")?;
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            || revision.len() != 64
            || !revision.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("无效的数据集ID或发布版本".into());
        }
        let name = d["name"]
            .as_str()
            .filter(|s| !s.is_empty() && s.chars().count() <= 128)
            .ok_or("无效的数据集名称")?;
        let bounds = if d["bounds"].is_null() {
            Value::Null
        } else {
            let a = d["bounds"]
                .as_array()
                .filter(|a| a.len() == 4)
                .ok_or("无效的数据集范围")?;
            let a = a
                .iter()
                .map(Value::as_f64)
                .collect::<Option<Vec<_>>>()
                .ok_or("无效的数据集范围")?;
            if !a.iter().all(|x| x.is_finite())
                || a[0] < -180.
                || a[2] > 180.
                || a[1] < -90.
                || a[3] > 90.
                || a[0] > a[2]
                || a[1] > a[3]
            {
                return Err("无效的数据集范围".into());
            }
            json!(a)
        };
        // Construct a trusted same-service URL; do not follow URLs supplied in metadata.
        let url = base
            .join(&format!("3dtiles/{id}/versions/{revision}/tileset.json"))
            .map_err(|e| e.to_string())?;
        result.push(json!({"id":id,"name":name,"version":revision,"type":"3dtiles","bounds":bounds,"tilesetUrl":url.as_str(),"updatedAt":d["updatedAt"],"source":d["metadata"]["source"].as_str().map(|s|s.chars().take(256).collect::<String>()),"description":d["metadata"]["description"].as_str().map(|s|s.chars().take(1000).collect::<String>())}));
    }
    Ok(json!({"datasets":result,"total":data["total"],"truncated":data["truncated"]}))
}
pub(crate) fn load(scene: &mut Value, catalog: &Value, id: &str) -> Result<Value, String> {
    let d = catalog["datasets"]
        .as_array()
        .and_then(|a| a.iter().find(|d| d["id"] == id))
        .ok_or("数据集不存在、未发布或已停用")?;
    if let Some(l) = scene["tilesets"]
        .as_array_mut()
        .and_then(|a| a.iter_mut().find(|l| l["url"] == d["tilesetUrl"]))
    {
        l["visible"] = json!(true);
        return Ok(l.clone());
    }
    crate::agent_earth_tiles::add(scene, &json!({"url":d["tilesetUrl"],"name":d["name"]}))?;
    let layer = scene["tilesets"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    layer["datasetId"] = json!(id);
    layer["datasetVersion"] = d["version"].clone();
    layer["bounds"] = d["bounds"].clone();
    Ok(layer.clone())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_and_idempotent_loading() {
        let base = reqwest::Url::parse("http://localhost:3000/geoserver/").unwrap();
        let mut raw = json!({"datasets":[{"id":"demo","name":"建筑","version":"a".repeat(64),"bounds":[113.,34.,114.,35.],"tilesetPath":"https://evil.invalid/x"}],"total":1,"truncated":false});
        let catalog = normalize(&base, &raw).unwrap();
        assert!(
            catalog["datasets"][0]["tilesetUrl"]
                .as_str()
                .unwrap()
                .starts_with(base.as_str())
        );
        let mut scene = json!({"tilesets":[]});
        let a = load(&mut scene, &catalog, "demo").unwrap();
        let b = load(&mut scene, &catalog, "demo").unwrap();
        assert_eq!(a["id"], b["id"]);
        assert_eq!(scene["tilesets"].as_array().unwrap().len(), 1);
        assert!(load(&mut scene, &catalog, "missing").is_err());
        raw["datasets"][0]["id"] = json!("../bad");
        assert!(normalize(&base, &raw).is_err());
    }
}
