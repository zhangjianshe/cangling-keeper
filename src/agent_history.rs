//! Account-scoped conversation listing and reversible lifecycle management.
use crate::store::Store;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

pub(crate) fn initialize(store: &Store) -> Result<(), String> {
    let columns = store
        .conn
        .prepare("PRAGMA table_info(agent_sessions)")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let tx = store
        .conn
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    for name in ["archived_at", "deleted_at"] {
        if !columns.iter().any(|c| c == name) {
            store
                .conn
                .execute_batch(&format!(
                    "ALTER TABLE agent_sessions ADD COLUMN {name} INTEGER NOT NULL DEFAULT 0;"
                ))
                .map_err(|e| e.to_string())?;
        }
    }
    store.conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_agent_sessions_lifecycle ON agent_sessions(owner_key,deleted_at,archived_at,updated_at DESC,id DESC);").map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}
fn sql<T>(r: rusqlite::Result<T>) -> Result<T, String> {
    r.map_err(|e| e.to_string())
}
fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(
        json!({"id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"updatedAt":r.get::<_,i64>(2)?,"projectId":r.get::<_,String>(3)?,"pinned":r.get::<_,bool>(4)?,"archivedAt":r.get::<_,i64>(5)?,"deletedAt":r.get::<_,i64>(6)?}),
    )
}
pub(crate) fn info(store: &Store, session: &str) -> Result<Value, String> {
    let owner = crate::agent_account::owner(store)?;
    sql(store.conn.query_row("SELECT id,title,updated_at,project_id,pinned,archived_at,deleted_at FROM agent_sessions WHERE id=?1 AND owner_key=?2",params![session,owner],from_row).optional())?
        .ok_or_else(||"会话不存在或不属于当前账号".into())
}
pub(crate) fn require_active(store: &Store, session: &str) -> Result<(), String> {
    let value = info(store, session)?;
    if value["deletedAt"].as_i64().unwrap_or(0) > 0 {
        return Err("会话已移入回收站，请先恢复。".into());
    }
    if value["archivedAt"].as_i64().unwrap_or(0) > 0 {
        return Err("会话已归档，请先取消归档。".into());
    }
    Ok(())
}
fn root_id(store: &Store, session: &str, message: &str) -> Result<String, String> {
    let mut current = message.to_owned();
    // Only follow earlier same-role messages in this session, as reply_groups does.
    for _ in 0..10000 {
        let parent:Option<String>=sql(store.conn.query_row("SELECT p.id FROM agent_messages m JOIN agent_messages p ON p.id=CASE WHEN m.role='user' THEN json_extract(m.model_metadata,'$.editOf') ELSE json_extract(m.model_metadata,'$.regenerateOf') END AND p.session_id=m.session_id AND p.role=m.role AND p.rowid<m.rowid WHERE m.id=?1 AND m.session_id=?2",params![current,session],|r|r.get(0)).optional())?;
        match parent {
            Some(parent) => current = parent,
            None => return Ok(current),
        }
    }
    Err("消息版本链过长".into())
}
fn snippet(content: &str, query: &str) -> String {
    let lowered = content.to_lowercase();
    let offset = lowered
        .find(&query.to_lowercase())
        .map(|n| lowered[..n].chars().count())
        .unwrap_or(0);
    let start = offset.saturating_sub(45);
    let chars: Vec<_> = content.chars().collect();
    let end = (start + 180).min(chars.len());
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        chars[start.min(chars.len())..end]
            .iter()
            .collect::<String>(),
        if end < chars.len() { "…" } else { "" }
    )
}
pub(crate) fn list(store: &Store, args: &Value) -> Result<Value, String> {
    let owner = crate::agent_account::owner(store)?;
    let state = args
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("active");
    if !matches!(state, "active" | "archived" | "deleted") {
        return Err("未知会话列表状态".into());
    }
    let query = args.get("q").and_then(Value::as_str).unwrap_or("").trim();
    if query.chars().count() > 256 || query.chars().any(char::is_control) {
        return Err("搜索内容最多256个字符，不能包含控制字符。".into());
    }
    let project = args.get("projectId").and_then(Value::as_str).unwrap_or("");
    if project.len() > 512 {
        return Err("项目 ID 无效".into());
    }
    let pinned = match args.get("pinned") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(v)) => Some(*v),
        _ => return Err("置顶筛选无效".into()),
    };
    let limit = match args.get("limit") {
        None => 30,
        Some(v) => v
            .as_i64()
            .filter(|n| *n > 0 && *n <= 100)
            .ok_or("分页大小必须在1到100之间")?,
    };
    let (cursor_time, cursor_id) = match args.get("cursor") {
        None | Some(Value::Null) => (None, None),
        Some(v) => {
            let time = v["updatedAt"]
                .as_i64()
                .filter(|n| *n >= 0)
                .ok_or("分页游标无效")?;
            let id = v["id"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 512)
                .ok_or("分页游标无效")?;
            (Some(time), Some(id))
        }
    };
    let state_filter = match state {
        "active" => "s.deleted_at=0 AND s.archived_at=0",
        "archived" => "s.deleted_at=0 AND s.archived_at>0",
        _ => "s.deleted_at>0",
    };
    let statement = format!(
        "SELECT s.id,s.title,s.updated_at,s.project_id,s.pinned,s.archived_at,s.deleted_at FROM agent_sessions s WHERE s.owner_key=?1 AND {state_filter} AND (?2='' OR s.project_id=?2) AND (?3 IS NULL OR s.pinned=?3) AND (?4='' OR instr(lower(s.title),lower(?4))>0 OR EXISTS(SELECT 1 FROM agent_messages m WHERE m.session_id=s.id AND instr(lower(m.content),lower(?4))>0)) AND (?5 IS NULL OR s.updated_at<?5 OR (s.updated_at=?5 AND s.id<?6)) ORDER BY s.updated_at DESC,s.id DESC LIMIT ?7"
    );
    let mut stmt = sql(store.conn.prepare(&statement))?;
    let mut items = sql(sql(stmt.query_map(
        params![
            owner,
            project,
            pinned,
            query,
            cursor_time,
            cursor_id,
            limit + 1
        ],
        from_row,
    ))?
    .collect::<rusqlite::Result<Vec<_>>>())?;
    let has_more = items.len() > limit as usize;
    items.truncate(limit as usize);
    if !query.is_empty() {
        for item in &mut items {
            let session = item["id"].as_str().unwrap();
            let found=sql(store.conn.query_row("SELECT id,role,content FROM agent_messages WHERE session_id=?1 AND instr(lower(content),lower(?2))>0 ORDER BY rowid LIMIT 1",params![session,query],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).optional())?;
            item["match"] = if let Some((message, role, content)) = found {
                json!({"rootId":root_id(store,session,&message)?,"messageId":message,"role":role,"snippet":snippet(&content,query)})
            } else {
                json!({"messageId":null,"rootId":null,"role":"title","snippet":snippet(item["title"].as_str().unwrap(),query)})
            };
        }
    }
    let cursor = if has_more {
        items
            .last()
            .map(|s| json!({"updatedAt":s["updatedAt"],"id":s["id"]}))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    Ok(json!({"items":items,"hasMore":has_more,"nextCursor":cursor}))
}
fn require_idle(store: &Store, session: &str) -> Result<(), String> {
    let busy:bool=sql(store.conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_model_pending WHERE session_id=?1) OR EXISTS(SELECT 1 FROM agent_runs WHERE session_id=?1 AND status IN ('running','approved','pending_approval')) OR EXISTS(SELECT 1 FROM agent_tasks WHERE session_id=?1 AND status IN ('running','model','tool','approval'))",[session],|r|r.get(0)))?;
    if busy {
        Err("当前会话仍在生成回复或执行工具，请先停止或等待完成。".into())
    } else {
        Ok(())
    }
}
pub(crate) fn mutate(store: &Store, command: &str, args: &Value) -> Result<Value, String> {
    let session = args["sessionId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("缺少参数 sessionId")?;
    let tx = sql(store.conn.unchecked_transaction())?;
    let before = info(store, session)?;
    require_idle(store, session)?;
    let now = chrono::Utc::now().timestamp_millis();
    match command {
        "agent_archive_session" => {
            if before["deletedAt"].as_i64().unwrap() > 0 {
                return Err("请先从回收站恢复会话。".into());
            }
            let archived = args["archived"].as_bool().ok_or("缺少归档状态")?;
            sql(store.conn.execute(
                "UPDATE agent_sessions SET archived_at=?2,updated_at=?3 WHERE id=?1",
                params![session, if archived { now } else { 0 }, now],
            ))?;
        }
        "agent_delete_session" => {
            sql(store.conn.execute(
                "UPDATE agent_sessions SET deleted_at=?2,updated_at=?2 WHERE id=?1",
                params![session, now],
            ))?;
        }
        "agent_restore_session" => {
            if before["deletedAt"].as_i64().unwrap() == 0 {
                return Err("会话不在回收站中。".into());
            }
            sql(store.conn.execute(
                "UPDATE agent_sessions SET deleted_at=0,updated_at=?2 WHERE id=?1",
                params![session, now],
            ))?;
        }
        "agent_purge_session" => {
            if args["confirm"] != true {
                return Err("永久删除需要确认。".into());
            }
            if before["deletedAt"].as_i64().unwrap() == 0 {
                return Err("只能永久删除回收站中的会话。".into());
            }
            sql(store.conn.execute("DELETE FROM agent_reviews WHERE run_id IN (SELECT id FROM agent_runs WHERE session_id=?1)",[session]))?;
            sql(store.conn.execute("DELETE FROM agent_model_contexts WHERE message_id IN (SELECT id FROM agent_messages WHERE session_id=?1)",[session]))?;
            for table in [
                "agent_reply_versions",
                "agent_runs",
                "agent_events",
                "agent_tasks",
                "agent_context_summaries",
                "agent_model_pending",
                "agent_messages",
            ] {
                sql(store.conn.execute(
                    &format!("DELETE FROM {table} WHERE session_id=?1"),
                    [session],
                ))?;
            }
            sql(store
                .conn
                .execute("DELETE FROM agent_sessions WHERE id=?1", [session]))?;
            sql(tx.commit())?;
            return Ok(json!({"id":session,"purged":true}));
        }
        _ => return Err("未知会话操作".into()),
    }
    let result = info(store, session)?;
    sql(tx.commit())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store() -> Store {
        let store = Store {
            conn: rusqlite::Connection::open_in_memory().unwrap(),
        };
        store.conn.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        CREATE TABLE agent_sessions(id TEXT PRIMARY KEY,title TEXT,updated_at INTEGER,project_id TEXT,pinned INTEGER,owner_key TEXT);
        CREATE TABLE agent_messages(id TEXT PRIMARY KEY,session_id TEXT,role TEXT,content TEXT,model_metadata TEXT);
        CREATE TABLE agent_model_pending(session_id TEXT,message_id TEXT);
        CREATE TABLE agent_runs(id TEXT,session_id TEXT,status TEXT);
        CREATE TABLE agent_tasks(session_id TEXT,status TEXT);
        CREATE TABLE agent_events(session_id TEXT);
        CREATE TABLE agent_context_summaries(session_id TEXT);
        CREATE TABLE agent_reviews(run_id TEXT);
        CREATE TABLE agent_model_contexts(message_id TEXT);
        CREATE TABLE agent_reply_versions(session_id TEXT);").unwrap();
        initialize(&store).unwrap();
        initialize(&store).unwrap();
        store
    }
    fn add(store: &Store, id: &str, owner: &str) {
        store.conn.execute("INSERT INTO agent_sessions(id,title,updated_at,project_id,pinned,owner_key) VALUES (?1,?1,100,'general',0,?2)",params![id,owner]).unwrap();
    }
    #[test]
    fn keyset_handles_ties_and_keeps_owners_separate() {
        let store = store();
        for id in ["a", "b", "c", "d"] {
            add(&store, id, "guest");
        }
        add(&store, "private", "other");
        let first = list(&store, &json!({"limit":2})).unwrap();
        assert_eq!(first["items"][0]["id"], "d");
        assert_eq!(first["items"][1]["id"], "c");
        assert_eq!(first["hasMore"], true);
        let second = list(&store, &json!({"limit":2,"cursor":first["nextCursor"]})).unwrap();
        assert_eq!(second["items"][0]["id"], "b");
        assert_eq!(second["items"][1]["id"], "a");
        assert_eq!(second["nextCursor"], Value::Null);
        assert!(info(&store, "private").is_err());
        assert!(list(&store, &json!({"limit":101})).is_err());
        assert!(list(&store, &json!({"cursor":{"id":"a"}})).is_err());
    }
    #[test]
    fn literal_search_finds_chinese_and_previous_versions() {
        let store = store();
        add(&store, "a", "guest");
        add(&store, "b", "guest");
        store.conn.execute_batch("INSERT INTO agent_messages VALUES ('root','a','assistant','old','{}');INSERT INTO agent_messages VALUES ('v2','a','assistant','中文 100%_ 完成','{\"regenerateOf\":\"root\"}');INSERT INTO agent_messages VALUES ('other','b','user','中文 100xx 完成','{}');").unwrap();
        let page = list(&store, &json!({"q":"%_"})).unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        assert_eq!(page["items"][0]["match"]["messageId"], "v2");
        assert_eq!(page["items"][0]["match"]["rootId"], "root");
        assert_eq!(
            list(&store, &json!({"q":"中文"})).unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn archive_delete_restore_and_inactive_guards() {
        let store = store();
        add(&store, "a", "guest");
        add(&store, "private", "other");
        assert!(
            mutate(
                &store,
                "agent_delete_session",
                &json!({"sessionId":"private"})
            )
            .is_err()
        );
        mutate(
            &store,
            "agent_archive_session",
            &json!({"sessionId":"a","archived":true}),
        )
        .unwrap();
        assert!(require_active(&store, "a").is_err());
        assert!(
            list(&store, &json!({})).unwrap()["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            list(&store, &json!({"state":"archived"})).unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        mutate(&store, "agent_delete_session", &json!({"sessionId":"a"})).unwrap();
        mutate(&store, "agent_restore_session", &json!({"sessionId":"a"})).unwrap();
        assert!(require_active(&store, "a").is_err());
        mutate(
            &store,
            "agent_archive_session",
            &json!({"sessionId":"a","archived":false}),
        )
        .unwrap();
        assert!(require_active(&store, "a").is_ok());
        store
            .conn
            .execute(
                "INSERT INTO agent_runs VALUES ('pending','a','pending_approval')",
                [],
            )
            .unwrap();
        assert!(mutate(&store, "agent_delete_session", &json!({"sessionId":"a"})).is_err());
    }
    #[test]
    fn purge_requires_confirmation_and_rolls_back_all_children() {
        let store = store();
        add(&store, "a", "guest");
        store.conn.execute_batch("INSERT INTO agent_messages VALUES ('m','a','user','keep','{}');INSERT INTO agent_model_contexts VALUES ('m');INSERT INTO agent_runs VALUES ('r','a','succeeded');INSERT INTO agent_reviews VALUES ('r');").unwrap();
        assert!(
            mutate(
                &store,
                "agent_purge_session",
                &json!({"sessionId":"a","confirm":true})
            )
            .is_err()
        );
        mutate(&store, "agent_delete_session", &json!({"sessionId":"a"})).unwrap();
        assert!(mutate(&store, "agent_purge_session", &json!({"sessionId":"a"})).is_err());
        store.conn.execute_batch("CREATE TRIGGER fail_delete BEFORE DELETE ON agent_sessions BEGIN SELECT RAISE(ABORT,'fixture');END;").unwrap();
        assert!(
            mutate(
                &store,
                "agent_purge_session",
                &json!({"sessionId":"a","confirm":true})
            )
            .is_err()
        );
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM agent_reviews", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        store
            .conn
            .execute_batch("DROP TRIGGER fail_delete")
            .unwrap();
        mutate(
            &store,
            "agent_purge_session",
            &json!({"sessionId":"a","confirm":true}),
        )
        .unwrap();
        for table in [
            "agent_sessions",
            "agent_messages",
            "agent_model_contexts",
            "agent_runs",
            "agent_reviews",
        ] {
            assert_eq!(
                store
                    .conn
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }
}
