//! Account-owned, project-scoped agent presets. Credentials remain in model/MCP stores.
use crate::store::Store;
use rusqlite::params;
use serde_json::{Value, json};
fn init(s: &Store) -> Result<(), String> {
    s.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_profiles(id TEXT PRIMARY KEY,owner TEXT NOT NULL,project_id TEXT NOT NULL,config TEXT NOT NULL);").map_err(|e|e.to_string())
}
fn project(s: &Store, session: &str) -> Result<String, String> {
    crate::agent_history::require_active(s, session)?;
    s.conn
        .query_row(
            "SELECT project_id FROM agent_sessions WHERE id=?1",
            [session],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
}
fn idle(s: &Store, session: &str) -> Result<(), String> {
    if s.conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_model_pending WHERE session_id=?1)",
            [session],
            |r| r.get::<_, bool>(0),
        )
        .map_err(|e| e.to_string())?
    {
        return Err("请等待当前回复结束后切换智能体".into());
    }
    Ok(())
}
pub(crate) fn list(s: &Store, session: &str) -> Result<Value, String> {
    init(s)?;
    let project = project(s, session)?;
    let owner = crate::agent_account::owner(s)?;
    let mut q = s
        .conn
        .prepare(
            "SELECT config FROM agent_profiles WHERE owner=?1 AND project_id=?2 ORDER BY rowid",
        )
        .map_err(|e| e.to_string())?;
    let rows = q
        .query_map(params![owner, project], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(json!(
        rows.into_iter()
            .map(|r| serde_json::from_str::<Value>(&r))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    ))
}
pub(crate) fn selected(s: &Store, session: &str) -> Result<Value, String> {
    let all = list(s, session)?;
    let id = s
        .get_setting(&format!("agent_profile:{session}"))?
        .unwrap_or_default();
    if id.is_empty() {
        return Ok(Value::Null);
    }
    all.as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .cloned()
        .ok_or("已选智能体不存在，请重新选择".into())
}
fn strings(v: &Value) -> Result<Vec<String>, String> {
    let a = v
        .as_array()
        .filter(|a| a.len() <= 64)
        .ok_or("Skills / MCP 必须是最多64项的列表")?;
    let mut out = vec![];
    for x in a {
        let x = x
            .as_str()
            .filter(|x| !x.is_empty() && x.len() <= 200)
            .ok_or("无效的关联项")?;
        if !out.iter().any(|s| s == x) {
            out.push(x.to_string());
        }
    }
    Ok(out)
}
pub(crate) fn options(s: &Store, session: &str) -> Result<Value, String> {
    let project = project(s, session)?;
    let directory: String = s
        .conn
        .query_row(
            "SELECT directory FROM agent_projects WHERE id=?1",
            [project],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let root = cap_std::fs::Dir::open_ambient_dir(directory, cap_std::ambient_authority())
        .map_err(|_| "无法读取项目目录")?;
    let guidance = crate::agent_skills::snapshot(
        &root,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )?;
    Ok(
        json!({"profiles":list(s,session)?,"selected":selected(s,session)?,"models":crate::model::config(s)?["servers"],"skills":guidance["skills"],"mcpTools":crate::agent_mcp::definitions_for(s,session)?}),
    )
}
pub(crate) fn save(s: &Store, args: &Value) -> Result<Value, String> {
    let session = args["sessionId"].as_str().ok_or("缺少会话")?;
    let all = list(s, session)?;
    let owner = crate::agent_account::owner(s)?;
    let project = project(s, session)?;
    let supplied = args["id"].as_str().unwrap_or("");
    if !supplied.is_empty() && !all.as_array().unwrap().iter().any(|p| p["id"] == supplied) {
        return Err("智能体不存在或无权修改".into());
    }
    if supplied.is_empty() && all.as_array().unwrap().len() >= 100 {
        return Err("每个项目最多100个智能体".into());
    }
    let name = args["name"].as_str().unwrap_or("").trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err("名称须为1–80字".into());
    }
    let prompt = args["systemPrompt"]
        .as_str()
        .ok_or("系统提示词必须为文本")?;
    if prompt.len() > 16384 {
        return Err("系统提示词不能超过16 KiB".into());
    }
    let model = args["modelId"].as_str().ok_or("请选择模型")?;
    let opts = options(s, session)?;
    if !opts["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"] == model && m["enabled"] == true && m["hasApiKey"] == true)
    {
        return Err("模型不可用，请重新选择".into());
    }
    let skills = strings(&args["skills"])?;
    let mcp = strings(&args["mcpTools"])?;
    if skills.iter().any(|n| {
        !opts["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["name"] == *n)
    }) {
        return Err("关联技能已不存在，请刷新列表".into());
    }
    if mcp.iter().any(|n| {
        !opts["mcpTools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["function"]["name"] == *n)
    }) {
        return Err("MCP 工具不可用，请先连接并启用".into());
    }
    let id = if supplied.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        supplied.into()
    };
    let value = json!({"id":id,"name":name,"systemPrompt":prompt,"modelId":model,"skills":skills,"mcpTools":mcp,"projectId":project,"revision":uuid::Uuid::new_v4().to_string()});
    s.conn.execute("INSERT INTO agent_profiles(id,owner,project_id,config) VALUES (?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET config=excluded.config",params![id,owner,project,value.to_string()]).map_err(|e|e.to_string())?;
    Ok(value)
}
pub(crate) fn choose(s: &Store, session: &str, id: &str) -> Result<(), String> {
    let all = list(s, session)?;
    idle(s, session)?;
    if !id.is_empty() && !all.as_array().unwrap().iter().any(|p| p["id"] == id) {
        return Err("智能体不存在或不属于当前项目".into());
    }
    s.set_setting(&format!("agent_profile:{session}"), id)
}
pub(crate) fn delete(s: &Store, session: &str, id: &str) -> Result<(), String> {
    let all = list(s, session)?;
    if !all.as_array().unwrap().iter().any(|p| p["id"] == id) {
        return Err("智能体不存在".into());
    }
    let owner = crate::agent_account::owner(s)?;
    let used:bool=s.conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_sessions a WHERE a.owner_key=?1 AND EXISTS(SELECT 1 FROM settings WHERE key='agent_profile:'||a.id AND value=?2))",params![owner,id],|r|r.get(0)).unwrap_or(true);
    if used {
        return Err("智能体仍被会话使用，请先切换为通用助手再删除".into());
    }
    s.conn
        .execute(
            "DELETE FROM agent_profiles WHERE id=?1 AND owner=?2",
            params![id, owner],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub(crate) fn includes(profile: &Value, field: &str, name: &str) -> bool {
    profile.is_null()
        || profile[field]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == name))
}
pub(crate) fn check_call(
    s: &Store,
    response: &str,
    name: &str,
    args: &Value,
) -> Result<(), String> {
    let raw: String = s
        .conn
        .query_row(
            "SELECT model_metadata FROM agent_messages WHERE id=?1",
            [response],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let meta: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let p = &meta["agentProfile"];
    if name.starts_with("mcp_") && !includes(p, "mcpTools", name) {
        return Err("此 MCP 工具未关联到本轮智能体，未执行".into());
    }
    if name == "load_skill" && !includes(p, "skills", args["name"].as_str().unwrap_or("")) {
        return Err("此技能未关联到本轮智能体，未加载".into());
    }
    Ok(())
}

pub(crate) fn validate_snapshot(p: &Value) -> Result<(), String> {
    if p.is_null() {
        return Ok(());
    }
    if !p.is_object()
        || p["name"].as_str().is_none_or(|s| s.chars().count() > 80)
        || p["systemPrompt"].as_str().is_none_or(|s| s.len() > 16384)
        || p["modelId"].as_str().is_none()
    {
        return Err("智能体快照无效".into());
    }
    strings(&p["skills"])?;
    strings(&p["mcpTools"])?;
    Ok(())
}
