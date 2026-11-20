//! Local conversation ownership: CIS origin + stable user ID (username fallback), or guest.
use crate::{AppState, store::Store};
use sha2::{Digest, Sha256};
pub(crate) fn owner(store: &Store) -> Result<String, String> {
    if store
        .get_setting(crate::sync::SETTING_TOKEN)?
        .unwrap_or_default()
        .is_empty()
    {
        return Ok("guest".into());
    }
    let server = crate::sync::normalize_server_url(
        &store
            .get_setting(crate::sync::SETTING_SERVER_URL)?
            .unwrap_or_else(|| crate::sync::DEFAULT_SERVER_URL.into()),
    )?;
    let id = store
        .get_setting(crate::sync::SETTING_USER_ID)?
        .unwrap_or_default();
    let user = if id.parse::<i64>().unwrap_or(0) > 0 {
        format!("id:{id}")
    } else {
        let name = store
            .get_setting(crate::sync::SETTING_USERNAME)?
            .unwrap_or_default();
        if name.is_empty() {
            return Err("登录身份缺少账号标识，请重新登录。".into());
        }
        format!("name:{name}")
    };
    Ok(format!(
        "cis:{:x}",
        Sha256::digest(format!("{server}\n{user}"))
    ))
}
pub(crate) fn initialize(store: &Store) -> Result<(), String> {
    let columns = store
        .conn
        .prepare("PRAGMA table_info(agent_sessions)")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if !columns.iter().any(|c| c == "owner_key") {
        let owner = owner(store)?;
        let tx = store
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        store.conn.execute_batch("ALTER TABLE agent_sessions ADD COLUMN owner_key TEXT NOT NULL DEFAULT 'guest'; CREATE INDEX IF NOT EXISTS idx_agent_sessions_owner ON agent_sessions(owner_key,updated_at);").map_err(|e|e.to_string())?;
        store
            .conn
            .execute("UPDATE agent_sessions SET owner_key=?1", [owner])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub(crate) fn owns(store: &Store, session: &str) -> bool {
    let Ok(owner) = owner(store) else {
        return false;
    };
    store
        .conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE id=?1 AND owner_key=?2)",
            rusqlite::params![session, owner],
            |r| r.get::<_, bool>(0),
        )
        .unwrap_or(false)
}
pub(crate) fn stop_tasks(core: &AppState, store: &Store) -> Result<(), String> {
    store
        .conn
        .execute("UPDATE agent_model_pending SET stop_requested=1", [])
        .map_err(|e| e.to_string())?;
    for (_, cancel) in core
        .model_cancellations
        .lock()
        .map_err(|e| e.to_string())?
        .drain()
    {
        let _ = cancel.send(());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_is_server_scoped_and_token_independent() {
        let store = Store {
            conn: rusqlite::Connection::open_in_memory().unwrap(),
        };
        store
            .conn
            .execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);")
            .unwrap();
        assert_eq!(owner(&store).unwrap(), "guest");
        store
            .set_setting(crate::sync::SETTING_TOKEN, "test")
            .unwrap();
        store
            .set_setting(crate::sync::SETTING_USER_ID, "5")
            .unwrap();
        store
            .set_setting(crate::sync::SETTING_SERVER_URL, "https://one.example/ib/")
            .unwrap();
        let a = owner(&store).unwrap();
        store
            .set_setting(crate::sync::SETTING_TOKEN, "rotated")
            .unwrap();
        assert_eq!(owner(&store).unwrap(), a);
        store
            .set_setting(crate::sync::SETTING_SERVER_URL, "https://two.example/ib")
            .unwrap();
        assert_ne!(owner(&store).unwrap(), a);
    }
}
