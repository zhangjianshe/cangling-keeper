//! Account-scoped, one-time default MCP registration. User edits/deletion always win.
use crate::{AppState, store::Store};
use rusqlite::params;
use serde_json::{Value, json};
fn setting(owner: &str) -> String {
    format!("map_mcp_default_v1:{owner}")
}
fn endpoint() -> Result<Option<String>, String> {
    if let Ok(url) = std::env::var("KEEPER_MAP_MCP_URL") {
        if url.trim().is_empty() {
            return Ok(None);
        }
        crate::agent_mcp_http::validate_url(&url)?;
        return Ok(Some(url));
    }
    let base = std::env::var("KEEPER_EARTH_MAP_URL")
        .unwrap_or_else(|_| "https://train.cangling.cn/geoserver/".into());
    let url = format!("{}/mcp", base.trim_end_matches('/'));
    crate::agent_mcp_http::validate_url(&url)?;
    Ok(Some(url))
}
fn prepare(store: &Store, url: &str) -> Result<Option<(String, Value)>, String> {
    let owner = crate::agent_account::owner(store)?;
    let key = setting(&owner);
    if store.get_setting(&key)?.is_none() {
        let tx = store
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let existing = crate::agent_mcp::list(store)?
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["command"] == url && s["transport"] == "http")
            .cloned();
        let (id, pending) = if let Some(server) = existing {
            (server["id"].as_str().unwrap().to_owned(), false)
        } else {
            let id = uuid::Uuid::new_v4().simple().to_string();
            store.conn.execute("INSERT INTO agent_mcp_servers(id,owner_key,name,command,args,enabled,tools,transport) VALUES(?1,?2,'Cangling Map',?3,'[]',0,'[]','http')",params![id,owner,url]).map_err(|e|e.to_string())?;
            (id, true)
        };
        store.set_setting(
            &key,
            &json!({"id":id,"url":url,"pending":pending,"attemptedAt":0}).to_string(),
        )?;
        tx.commit().map_err(|e| e.to_string())?;
    }
    let mut marker: Value =
        serde_json::from_str(&store.get_setting(&key)?.unwrap()).map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    if marker["pending"] != true || now - marker["attemptedAt"].as_i64().unwrap_or(0) < 60 {
        return Ok(None);
    }
    let server = crate::agent_mcp::list(store)?
        .as_array()
        .unwrap()
        .iter()
        .find(|s| {
            s["id"] == marker["id"]
                && s["command"] == marker["url"]
                && s["enabled"] == false
                && s["tools"] == json!([])
        })
        .cloned();
    let Some(server) = server else {
        return Ok(None);
    };
    marker["attemptedAt"] = json!(now);
    store.set_setting(&key, &marker.to_string())?;
    Ok(Some((owner, server)))
}
/// Called before bootstrap/list; bounded discovery does not prevent opening Keeper when Map is offline.
pub(crate) async fn ensure(core: &AppState) -> Result<(), String> {
    let Some(url) = endpoint()? else {
        return Ok(());
    };
    let candidate = {
        let store = core.store.lock().map_err(|e| e.to_string())?;
        prepare(&store, &url)?
    };
    let Some((owner, server)) = candidate else {
        return Ok(());
    };
    let discovered = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        crate::agent_mcp::discover(&server),
    )
    .await
    .unwrap_or_else(|_| Err("默认Map MCP连接超时，可在模型工具设置中重新发现".into()));
    let store = core.store.lock().map_err(|e| e.to_string())?;
    if crate::agent_account::owner(&store)? != owner {
        return Ok(());
    }
    let key = setting(&owner);
    let mut marker: Value = serde_json::from_str(&store.get_setting(&key)?.unwrap_or("{}".into()))
        .map_err(|e| e.to_string())?;
    if marker["pending"] != true
        || !crate::agent_mcp::list(&store)?
            .as_array()
            .unwrap()
            .contains(&server)
    {
        return Ok(());
    }
    match discovered {
        Ok(tools) => {
            let tools = crate::agent_mcp::merge_selection(&server, tools);
            store
                .conn
                .execute(
                    "UPDATE agent_mcp_servers SET tools=?1,enabled=1 WHERE id=?2 AND owner_key=?3",
                    params![
                        json!(tools).to_string(),
                        server["id"].as_str().unwrap(),
                        owner
                    ],
                )
                .map_err(|e| e.to_string())?;
            marker["pending"] = json!(false);
            marker["error"] = Value::Null;
        }
        Err(e) => marker["error"] = json!(e),
    }
    store.set_setting(&key, &marker.to_string())?;
    Ok(())
}
pub(crate) fn user_changed(store: &Store, id: &str) -> Result<(), String> {
    let key = setting(&crate::agent_account::owner(store)?);
    if let Some(raw) = store.get_setting(&key)? {
        let mut marker: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        if marker["id"] == id {
            marker["pending"] = json!(false);
            marker["error"] = Value::Null;
            store.set_setting(&key, &marker.to_string())?;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_is_account_scoped_and_never_recreates_deleted_server() {
        let s = Store {
            conn: rusqlite::Connection::open_in_memory().unwrap(),
        };
        s.conn
            .execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL)")
            .unwrap();
        crate::agent_mcp::initialize(&s).unwrap();
        let (owner, server) = prepare(&s, "http://127.0.0.1:4310/mcp").unwrap().unwrap();
        assert_eq!(owner, "guest");
        assert!(prepare(&s, "http://127.0.0.1:4310/mcp").unwrap().is_none());
        let id = server["id"].as_str().unwrap();
        user_changed(&s, id).unwrap();
        s.conn
            .execute("DELETE FROM agent_mcp_servers WHERE id=?1", [id])
            .unwrap();
        assert!(prepare(&s, "http://127.0.0.1:4310/mcp").unwrap().is_none());
        assert_eq!(crate::agent_mcp::list(&s).unwrap(), json!([]));
        for (k, v) in [
            ("login_token", "fixture"),
            ("login_username", "other"),
            ("server_url", "http://localhost"),
        ] {
            s.set_setting(k, v).unwrap();
        }
        let (owner2, server2) = prepare(&s, "http://127.0.0.1:4310/mcp").unwrap().unwrap();
        assert_ne!(owner2, owner);
        assert_ne!(server2["id"], server["id"]);
    }
}
