//! Account-scoped immutable model downloads and persisted geographic placement.
use crate::store::Store;
use serde_json::{Value, json};
fn table(s: &Store) -> Result<(), String> {
    s.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_earth_models(id TEXT PRIMARY KEY,owner TEXT NOT NULL,project TEXT NOT NULL,name TEXT NOT NULL,sha TEXT NOT NULL,data BLOB NOT NULL,UNIQUE(owner,project,sha));").map_err(|e|e.to_string())
}
pub(crate) fn data(s: &Store, id: &str) -> Result<Vec<u8>, String> {
    table(s)?;
    let owner = crate::agent_account::owner(s)?;
    s.conn
        .query_row(
            "SELECT data FROM agent_earth_models WHERE id=?1 AND owner=?2",
            rusqlite::params![id, owner],
            |r| r.get(0),
        )
        .map_err(|_| "模型不存在或不属于当前账户".into())
}
pub(crate) fn placement(v: &Value) -> Result<(), String> {
    for (k, min, max) in [
        ("longitude", -180., 180.),
        ("latitude", -90., 90.),
        ("height", -10000., 100000.),
        ("heading", -360., 360.),
        ("scale", 0.001, 10000.),
    ] {
        if !v[k]
            .as_f64()
            .is_some_and(|n| n.is_finite() && n >= min && n <= max)
        {
            return Err(format!("模型参数 {k} 无效"));
        }
    }
    if !matches!(
        v["altitudeMode"].as_str(),
        Some("relativeToGround" | "absolute")
    ) {
        return Err("模型高度模式无效".into());
    }
    Ok(())
}
pub(crate) fn validate(s: &Store, project: &str, v: &Value) -> Result<(), String> {
    if v.is_null() {
        return Ok(());
    }
    let items = v.as_array().filter(|a| a.len() <= 8).ok_or("最多8个模型")?;
    table(s)?;
    let owner = crate::agent_account::owner(s)?;
    let mut ids = std::collections::HashSet::new();
    for item in items {
        placement(item)?;
        let id = item["id"].as_str().ok_or("模型ID无效")?;
        if !ids.insert(id) || !item["visible"].is_boolean() {
            return Err("模型图层无效".into());
        }
        let valid:bool=s.conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_earth_models WHERE id=?1 AND owner=?2 AND project=?3)",rusqlite::params![id,owner,project],|r|r.get(0)).map_err(|e|e.to_string())?;
        if !valid {
            return Err("模型不属于当前账户和项目".into());
        }
    }
    Ok(())
}
fn check_glb(bytes: &[u8]) -> Result<(), String> {
    let u = |i| {
        bytes
            .get(i..i + 4)
            .and_then(|v| v.try_into().ok())
            .map(u32::from_le_bytes)
    };
    if bytes.get(..4) != Some(b"glTF")
        || u(4) != Some(2)
        || u(8) != Some(bytes.len() as u32)
        || u(16) != Some(0x4e4f534a)
    {
        return Err("需要有效的glTF 2.0 GLB文件".into());
    }
    let len = u(12).ok_or("缺少JSON块")? as usize;
    let json: Value = serde_json::from_slice(bytes.get(20..20 + len).ok_or("GLB块长度错误")?)
        .map_err(|_| "GLB JSON无效")?;
    if u(24 + len) != Some(0x004e4942)
        || u(20 + len).map(|n| 28 + len + n as usize) != Some(bytes.len())
    {
        return Err("GLB须包含完整BIN块".into());
    }
    if ["animations", "skins", "extensionsRequired"]
        .iter()
        .any(|k| json[*k].as_array().is_some_and(|a| !a.is_empty()))
    {
        return Err("第一版仅支持静态模型，不支持动画、蒙皮及必需扩展".into());
    }
    for key in ["buffers", "images"] {
        if json[key]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.get("uri").is_some()))
        {
            return Err("请将纹理和缓冲区内嵌到GLB中".into());
        }
    }
    Ok(())
}
pub(crate) fn publish(
    s: &Store,
    project: &str,
    root: &cap_std::fs::Dir,
    path: &str,
) -> Result<Value, String> {
    if !path.to_lowercase().ends_with(".glb") {
        return Err("仅支持.glb文件".into());
    }
    let bytes = crate::agent_files::model_bytes(root, path)?;
    check_glb(&bytes)?;
    table(s)?;
    let owner = crate::agent_account::owner(s)?;
    let sha = crate::agent_actions::hash(&bytes);
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("无效文件名")?;
    let old: Option<String> = s
        .conn
        .query_row(
            "SELECT id FROM agent_earth_models WHERE owner=?1 AND project=?2 AND sha=?3",
            rusqlite::params![owner, project, sha],
            |r| r.get(0),
        )
        .ok();
    let id = if let Some(id) = old {
        id
    } else {
        let total: i64 = s
            .conn
            .query_row(
                "SELECT COALESCE(SUM(length(data)),0) FROM agent_earth_models WHERE owner=?1",
                [&owner],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if total + bytes.len() as i64 > 128 * 1024 * 1024 {
            return Err("模型快照超过128 MiB".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        s.conn
            .execute(
                "INSERT INTO agent_earth_models VALUES (?1,?2,?3,?4,?5,?6)",
                rusqlite::params![id, owner, project, name, sha, bytes],
            )
            .map_err(|e| e.to_string())?;
        id
    };
    Ok(
        json!({"id":id,"name":name,"sha256":sha,"url":format!("/api/earth/models/{id}"),"downloadUrl":format!("/api/earth/models/{id}?download=1"),"scope":"current_account_localhost"}),
    )
}
pub(crate) fn add(
    s: &Store,
    project: &str,
    root: &cap_std::fs::Dir,
    scene: &mut Value,
    args: &Value,
) -> Result<(), String> {
    let mut model = json!({"longitude":args["longitude"],"latitude":args["latitude"],"height":args.get("height").cloned().unwrap_or(json!(0)),"heading":args.get("heading").cloned().unwrap_or(json!(0)),"scale":args.get("scale").cloned().unwrap_or(json!(1)),"altitudeMode":args.get("altitudeMode").cloned().unwrap_or(json!("relativeToGround")),"visible":true});
    placement(&model)?;
    let asset = publish(s, project, root, args["path"].as_str().ok_or("缺少路径")?)?;
    model["id"] = asset["id"].clone();
    model["name"] = asset["name"].clone();
    if scene.get("models").is_none() {
        scene["models"] = json!([]);
    }
    let models = scene["models"].as_array_mut().ok_or("模型列表无效")?;
    if models.len() >= 8 && !models.iter().any(|m| m["id"] == model["id"]) {
        return Err("最多8个模型".into());
    }
    models.retain(|m| m["id"] != model["id"]);
    models.push(model);
    Ok(())
}
