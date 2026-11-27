//! Account-scoped CIS model catalog. Provider credentials stay on CIS.
use crate::{AppState, store::Store};
use serde_json::{Value, json};
use std::sync::Arc;
pub(crate) fn initialize(store: &Store) -> Result<(), String> {
    store.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_cis_models(owner_key TEXT NOT NULL,id TEXT NOT NULL,name TEXT NOT NULL,provider TEXT NOT NULL,model TEXT NOT NULL,context_limit INTEGER NOT NULL,revision TEXT NOT NULL DEFAULT '',PRIMARY KEY(owner_key,id));").map_err(|e|e.to_string())?;
    let columns = store
        .conn
        .prepare("PRAGMA table_info(agent_cis_models)")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if !columns.iter().any(|v| v == "revision") {
        store
            .conn
            .execute_batch(
                "ALTER TABLE agent_cis_models ADD COLUMN revision TEXT NOT NULL DEFAULT '';",
            )
            .map_err(|e| e.to_string())?;
    }
    if !columns.iter().any(|v|v=="supports_images") {store.conn.execute_batch("ALTER TABLE agent_cis_models ADD COLUMN supports_images INTEGER NOT NULL DEFAULT 0;").map_err(|e|e.to_string())?;}
    Ok(())
}
fn credentials(store: &Store) -> Result<(String, String, String), String> {
    let owner = crate::agent_account::owner(store)?;
    if owner == "guest" {
        return Err("请先登录 CIS Server".into());
    }
    let url = crate::sync::normalize_server_url(
        &store
            .get_setting(crate::sync::SETTING_SERVER_URL)?
            .ok_or("CIS 地址不可用")?,
    )?;
    let token = store
        .get_setting(crate::sync::SETTING_TOKEN)?
        .filter(|s| !s.is_empty())
        .ok_or("请先登录 CIS Server")?;
    Ok((owner, url, token))
}
fn client_with_timeout(seconds: u64) -> Result<reqwest::Client, String> {
    let ca =
        reqwest::Certificate::from_der(include_bytes!("../docs/certificates/cangling-root-ca.cer"))
            .map_err(|_| "CIS 根证书无效")?;
    reqwest::Client::builder()
        .add_root_certificate(ca)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(8))
        .timeout(std::time::Duration::from_secs(seconds))
        .build()
        .map_err(|_| "无法创建 CIS 连接".into())
}
fn client() -> Result<reqwest::Client, String> {
    client_with_timeout(20)
}
pub(crate) fn proxy_client() -> Result<reqwest::Client, String> {
    let ca=reqwest::Certificate::from_der(include_bytes!("../docs/certificates/cangling-root-ca.cer")).map_err(|_|"CIS 根证书无效")?;
    crate::model::streaming_client_builder().add_root_certificate(ca).build().map_err(|_|"无法创建 CIS 流式连接".into())
}
pub(crate) fn catalog(store: &Store) -> Result<Vec<Value>, String> {
    initialize(store)?;
    let owner = crate::agent_account::owner(store)?;
    if owner == "guest" {
        return Ok(Vec::new());
    }
    let mut stmt=store.conn.prepare("SELECT id,name,provider,model,context_limit,revision,supports_images FROM agent_cis_models WHERE owner_key=?1 AND revision<>'' ORDER BY name,id").map_err(|e|e.to_string())?;
    let rows=stmt.query_map([&owner],|r|{let id:String=r.get(0)?;Ok(json!({"id":format!("cis-model:{}:{}",owner,id),"remoteId":id,"name":r.get::<_,String>(1)?,"provider":"cis-proxy","upstreamProvider":r.get::<_,String>(2)?,"model":r.get::<_,String>(3)?,"contextLimit":r.get::<_,i64>(4)?,"revision":r.get::<_,String>(5)?,"source":"cis","enabled":true,"hasApiKey":true,"supportsImages":r.get::<_,bool>(6)?}))}).map_err(|e|e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}
pub(crate) fn proxy_credentials(
    store: &Store,
    id: &str,
) -> Result<(String, String, String, String), String> {
    let (owner, url, token) = credentials(store)?;
    let prefix = format!("cis-model:{}:", owner);
    let remote = id.strip_prefix(&prefix).ok_or("模型不属于当前 CIS 账号")?;
    if remote.is_empty()
        || !remote
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("服务端模型 ID 无效".into());
    }
    let model: String = store
        .conn
        .query_row(
            "SELECT model FROM agent_cis_models WHERE owner_key=?1 AND id=?2",
            rusqlite::params![owner, remote],
            |r| r.get(0),
        )
        .map_err(|_| "CIS 模型不可用，请刷新模型列表")?;
    Ok((
        format!("{url}/api/v1/keeper/models/{remote}"),
        model,
        token,
        "cis-proxy".into(),
    ))
}
pub(crate) async fn call(
    core: &Arc<AppState>,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let (owner, url, token) = {
        let store = core.store.lock().map_err(|e| e.to_string())?;
        credentials(&store)?
    };
    let mutation = method != reqwest::Method::GET;
    let mut req = client()?
        .request(method, format!("{url}/api/v1/keeper/models{path}"))
        .header("API-TOKEN", &token);
    if let Some(body) = body {
        req = req.json(&body);
    }
    let mut response = req.send().await.map_err(|_| "无法连接 CIS 模型服务")?;
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            401 => "CIS 登录已过期",
            403 => "需要 CIS 管理员权限",
            404 => "CIS 尚未提供模型代理接口",
            _ => "CIS 模型服务请求失败",
        }
        .into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "读取 CIS 模型列表失败")?
    {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err("CIS 模型响应过大".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let result: Value = serde_json::from_slice(&bytes).map_err(|_| "CIS 模型响应格式错误")?;
    if result["success"] != true || result["code"] != 200 {
        return Err(result["message"]
            .as_str()
            .unwrap_or("CIS 模型服务失败")
            .chars()
            .take(200)
            .collect());
    }
    {
        let store = core.store.lock().map_err(|e| e.to_string())?;
        let current = credentials(&store);
        if !matches!(current, Ok((now_owner, now_url, now_token)) if (&owner, &url, &token) == (&now_owner, &now_url, &now_token)) {
            return Err(if mutation { "登录账号已改变，请到原 CIS 服务器核实操作是否生效" } else { "登录账号已改变，已丢弃模型结果" }.into());
        }
    }
    Ok(result["data"].clone())
}
pub(crate) async fn refresh(core: &Arc<AppState>) -> Result<Value, String> {
    let owner = {
        let store = core.store.lock().map_err(|e| e.to_string())?;
        credentials(&store)?.0
    };
    let data = call(core, reqwest::Method::GET, "", None).await?;
    let models = data["models"].as_array().ok_or("CIS 模型列表格式错误")?;
    if models.len() > 256 {
        return Err("CIS 模型列表过大".into());
    }
    let mut parsed = Vec::new();
    for m in models {
        if m["enabled"] != true {
            continue;
        }
        let get = |k: &str, max: usize| {
            m[k].as_str()
                .filter(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
                .map(str::to_owned)
                .ok_or_else(|| format!("CIS 模型字段无效: {k}"))
        };
        let id = get("id", 64)?;
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err("CIS 模型 ID 无效".into());
        }
        let limit = m["contextLimit"]
            .as_i64()
            .filter(|x| (16384..=131072).contains(x))
            .ok_or("CIS 上下文预算无效")?;
        parsed.push((
            id,
            get("name", 240)?,
            get("provider", 32)?,
            get("model", 128)?,
            limit,
            get("revision", 80)?,
            m["supportsImages"].as_bool().unwrap_or(false),
        ));
    }
    let store = core.store.lock().map_err(|e| e.to_string())?;
    if credentials(&store)?.0 != owner {
        return Err("登录账号已改变，已丢弃模型列表".into());
    }
    let tx = store
        .conn
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    store
        .conn
        .execute("DELETE FROM agent_cis_models WHERE owner_key=?1", [&owner])
        .map_err(|e| e.to_string())?;
    for (id, name, provider, model, limit, revision, supports_images) in parsed {
        store
            .conn
            .execute(
                "INSERT INTO agent_cis_models(owner_key,id,name,provider,model,context_limit,revision,supports_images) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                rusqlite::params![owner, id, name, provider, model, limit, revision, supports_images],
            )
            .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    crate::model::config(&store)
}
