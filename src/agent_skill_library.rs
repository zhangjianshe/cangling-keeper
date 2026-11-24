//! Versioned account/shared and agent/private skills, stored locally in SQLite.
//! Project skills stay on disk. Stable references prevent cross-project name collisions.
use crate::store::Store;
use rusqlite::params;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
pub(crate) fn init(s: &Store) -> Result<(), String> {
    s.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_skill_library(id TEXT PRIMARY KEY,owner TEXT NOT NULL,profile_id TEXT NOT NULL,name TEXT NOT NULL,description TEXT NOT NULL,revision TEXT NOT NULL); CREATE TABLE IF NOT EXISTS agent_skill_versions(skill_id TEXT NOT NULL,revision TEXT NOT NULL,files TEXT NOT NULL,PRIMARY KEY(skill_id,revision)); CREATE TABLE IF NOT EXISTS agent_skill_directories(skill_id TEXT NOT NULL,revision TEXT NOT NULL,directories TEXT NOT NULL,PRIMARY KEY(skill_id,revision)); CREATE INDEX IF NOT EXISTS agent_skill_owner ON agent_skill_library(owner,profile_id);").map_err(|e|e.to_string())
}
pub(crate) fn project(s: &Store, session: &str) -> Result<String, String> {
    crate::agent_history::require_active(s, session)?;
    s.conn
        .query_row(
            "SELECT project_id FROM agent_sessions WHERE id=?1",
            [session],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
}
pub(crate) fn normalize(p: &mut Value) {
    if let Some(project) = p["projectId"].as_str().map(str::to_owned) {
        if let Some(a) = p["skills"].as_array_mut() {
            for n in a {
                if let Some(name) = n.as_str().filter(|n| !n.contains(':')) {
                    *n = json!(format!("project:{project}:{name}"));
                }
            }
        }
    }
    p["scope"] = json!("account");
}
fn own_profile(s: &Store, id: &str) -> Result<(), String> {
    let owner = crate::agent_account::owner(s)?;
    if !s
        .conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE id=?1 AND owner=?2)",
            params![id, owner],
            |r| r.get::<_, bool>(0),
        )
        .map_err(|e| e.to_string())?
    {
        return Err("智能体不存在或不属于当前账户".into());
    }
    Ok(())
}
pub(crate) fn managed(s: &Store) -> Result<Vec<Value>, String> {
    init(s)?;
    let owner = crate::agent_account::owner(s)?;
    let mut q=s.conn.prepare("SELECT id,profile_id,name,description,revision FROM agent_skill_library WHERE owner=?1 ORDER BY rowid").map_err(|e|e.to_string())?;
    q.query_map([owner],|r|{let id:String=r.get(0)?;let profile:String=r.get(1)?;let scope=if profile.is_empty(){"account"}else{"agent"};Ok(json!({"id":id,"ref":format!("{scope}:{id}"),"scope":scope,"profileId":profile,"name":r.get::<_,String>(2)?,"description":r.get::<_,String>(3)?,"revision":r.get::<_,String>(4)?}))}).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}
const EARTH_SKILL:&str=include_str!("../skills/earth-workbench/SKILL.md");
fn builtin()->Value{json!({"id":"system-earth-workbench","ref":"system:earth-workbench","scope":"system","name":"earth-workbench","description":"地球定位、项目图片和GeoJSON原生叠加、选中要素、图片下载与渲染核验","revision":format!("{:x}",Sha256::digest(EARTH_SKILL.as_bytes())),"readonly":true})}
pub(crate) fn catalog(s: &Store, session: &str, guidance: &Value) -> Result<Vec<Value>, String> {
    let project = project(s, session)?;
    let mut out = vec![builtin()];
    out.extend(managed(s)?);
    for entry in guidance["skills"].as_array().into_iter().flatten() {
        let mut e = entry.clone();
        e["ref"] = json!(format!(
            "project:{}:{}",
            project,
            e["name"].as_str().unwrap_or("")
        ));
        e["scope"] = json!("project");
        e["projectId"] = json!(project);
        out.push(e);
    }
    Ok(out)
}
pub(crate) fn save(s: &Store, a: &Value) -> Result<Value, String> {
    init(s)?;
    let session = a["sessionId"].as_str().ok_or("缺少会话")?;
    project(s, session)?;
    let owner = crate::agent_account::owner(s)?;
    let profile = match a["scope"].as_str() {
        Some("account") => "",
        Some("agent") => {
            let id = a["profileId"]
                .as_str()
                .filter(|x| !x.is_empty())
                .ok_or("请先保存智能体，再创建专属技能")?;
            own_profile(s, id)?;
            id
        }
        _ => return Err("只能在技能库创建账户共享或智能体专属技能".into()),
    };
    let name = a["name"].as_str().ok_or("缺少技能名称")?;
    if !crate::agent_skills::valid_name(name) {
        return Err("技能名称须为1–64位小写字母、数字或单连字符".into());
    }
    let description = a["description"]
        .as_str()
        .filter(|x| !x.trim().is_empty() && x.len() <= 1024)
        .ok_or("技能简介须为1–1024字节")?;
    let files = a["files"]
        .as_object()
        .filter(|x| !x.is_empty() && x.len() <= 64)
        .ok_or("技能必须包含1–64个文本文件")?;
    let mut size = 0;
    for (path, value) in files {
        check_path(path)?;
        let text = value
            .as_str()
            .filter(|x| x.len() <= 65536 && !x.contains('\0'))
            .ok_or("每个文件须为不超过64 KiB的文本")?;
        size += text.len();
    }
    if size > 1048576 {
        return Err("技能内容不能超过1 MiB".into());
    }
    let directories = validate_directories(a.get("directories"), files)?;
    let body = files
        .get("SKILL.md")
        .and_then(Value::as_str)
        .ok_or("必须包含 SKILL.md")?;
    crate::agent_skills::validate_document(body, name)?;
    let supplied = a["id"].as_str().unwrap_or("");
    let id = if supplied.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        supplied.to_owned()
    };
    let tx = s.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    if !supplied.is_empty() {
        let (old, old_profile): (String, String) = s
            .conn
            .query_row(
                "SELECT revision,profile_id FROM agent_skill_library WHERE id=?1 AND owner=?2",
                params![id, owner],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| "技能不存在或无权编辑")?;
        if a["expectedRevision"] != old {
            return Err("技能已被修改，请重新打开后再保存".into());
        }
        if old_profile != profile {
            return Err("不能更改技能归属，请新建副本".into());
        }
    } else {
        let count: i64 = s
            .conn
            .query_row(
                "SELECT count(*) FROM agent_skill_library WHERE owner=?1",
                [&owner],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if count >= 256 {
            return Err("每个账户最多256个技能".into());
        }
    }
    let revision = uuid::Uuid::new_v4().to_string();
    s.conn
        .execute(
            "INSERT INTO agent_skill_versions VALUES (?1,?2,?3)",
            params![id, revision, json!(files).to_string()],
        )
        .map_err(|e| e.to_string())?;
    s.conn.execute("INSERT INTO agent_skill_library VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET name=excluded.name,description=excluded.description,revision=excluded.revision",params![id,owner,profile,name,description,revision]).map_err(|e|e.to_string())?;
    s.conn.execute("INSERT INTO agent_skill_directories VALUES (?1,?2,?3)",params![id,revision,json!(directories).to_string()]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    managed(s)?
        .into_iter()
        .find(|v| v["id"] == id)
        .ok_or("保存后技能不存在".into())
}
fn check_path(p: &str) -> Result<(), String> {
    if p.is_empty()
        || p.len() > 200
        || p.contains('\\')
        || p.chars().any(char::is_control)
        || p.starts_with('/')
        || p.split('/')
            .any(|c| c.is_empty() || c == "." || c == ".." || c.contains(':'))
    {
        return Err("技能文件须使用安全的相对路径".into());
    }
    if p != "SKILL.md"
        && !p.starts_with("scripts/")
        && !p.starts_with("references/")
        && !p.starts_with("assets/")
    {
        return Err("文件须位于 SKILL.md、scripts、references 或 assets 中".into());
    }
    Ok(())
}
fn files(s: &Store, entry: &Value, revision: &str) -> Result<Value, String> {
    if entry["scope"]=="system" {let current=builtin();if entry["ref"]!=current["ref"]||revision!=current["revision"].as_str().unwrap(){return Err("系统技能版本已变化，请发起新回复".into());}return Ok(json!({"SKILL.md":EARTH_SKILL}));}

    let raw: String = s
        .conn
        .query_row(
            "SELECT files FROM agent_skill_versions WHERE skill_id=?1 AND revision=?2",
            params![entry["id"].as_str().ok_or("技能标识无效")?, revision],
            |r| r.get(0),
        )
        .map_err(|_| "技能版本不存在")?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}
// Directory metadata is versioned with files, including empty directories.
fn validate_directories(value: Option<&Value>, files: &serde_json::Map<String, Value>) -> Result<Vec<String>, String> {
    let mut dirs = std::collections::BTreeSet::from(["scripts".to_owned(), "references".to_owned(), "assets".to_owned()]);
    if let Some(value) = value {
        let input = value.as_array().filter(|a| a.len() <= 128).ok_or("技能目录最多128个")?;
        for path in input {
            let path = path.as_str().ok_or("目录路径必须是文本")?;
            if path.len()>200 {return Err("目录路径过长".into());}
            if !["scripts", "references", "assets"].contains(&path) { check_path(path)?; }
            if path == "SKILL.md" { return Err("SKILL.md 必须是文件".into()); }
            dirs.insert(path.to_owned());
        }
    }
    for path in files.keys().chain(dirs.clone().iter()) {
        let mut parent = path.as_str();
        while let Some((head,_))=parent.rsplit_once('/') {dirs.insert(head.to_owned());parent=head;}
    }
    if dirs.len()>128 {return Err("技能目录最多128个".into());}
    if dirs.iter().any(|dir| files.contains_key(dir)) {return Err("文件与目录不能使用相同路径".into());}
    Ok(dirs.into_iter().collect())
}
pub(crate) fn read(s: &Store, a: &Value) -> Result<Value, String> {
    project(s, a["sessionId"].as_str().ok_or("缺少会话")?)?;
    let id = a["id"].as_str().ok_or("缺少技能")?;
    let mut e = managed(s)?
        .into_iter()
        .find(|x| x["id"] == id)
        .ok_or("技能不存在或无权读取")?;
    e["files"] = files(s, &e, e["revision"].as_str().unwrap())?;
    use rusqlite::OptionalExtension;
    let raw: Option<String> = s.conn.query_row("SELECT directories FROM agent_skill_directories WHERE skill_id=?1 AND revision=?2",params![e["id"].as_str().unwrap(),e["revision"].as_str().unwrap()],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
    let stored = raw.map(|v|serde_json::from_str::<Value>(&v)).transpose().map_err(|e|e.to_string())?;
    e["directories"] = json!(validate_directories(stored.as_ref(),e["files"].as_object().ok_or("技能文件无效")?)?);
    Ok(e)
}
pub(crate) fn prepare(
    s: &Store,
    session: &str,
    profile: &Value,
    guidance: &mut Value,
    previous: Option<&Value>,
) -> Result<Value, String> {
    let project = project(s, session)?;
    let all = catalog(s, session, guidance)?;
    // User-managed skill bindings remain frozen; built-in guidance is globally available.
    let previous_refs = previous.and_then(Value::as_array).map(|items| {
        items.iter().map(|entry| entry["ref"].clone()).collect::<Vec<_>>()
    });
    let refs = previous_refs.as_ref().or_else(|| profile["skills"].as_array());
    let mut selected = Vec::new();
    let mut warnings = guidance["warnings"].as_array().cloned().unwrap_or_default();
    if let Some(refs) = refs {
        for r in refs {
            let reference = r.as_str().ok_or("技能关联无效")?;
            if reference.starts_with("project:")
                && !reference.starts_with(&format!("project:{project}:"))
            {
                warnings.push(json!(format!("其他项目技能不适用于当前项目：{reference}")));
                continue;
            }
            let entry = all
                .iter()
                .find(|e| e["ref"] == *r)
                .ok_or("关联技能不存在，请编辑智能体配置")?;
            if entry["scope"] == "agent" && entry["profileId"] != profile["id"] {
                return Err("不能使用其他智能体的专属技能".into());
            }
            selected.push(entry.clone());
        }
    } else {
        selected = all.into_iter().filter(|e| e["scope"] != "agent").collect();
    }
    let system=builtin();
    if !selected.iter().any(|e|e["ref"]==system["ref"]){selected.insert(0,system);}
    if selected.len() > 64 {
        return Err("本轮技能超过64项，请使用智能体选择所需技能".into());
    }
    if let Some(old) = previous.and_then(Value::as_array) {
        for e in &mut selected {
            if let Some(saved) = old.iter().find(|v| v["ref"] == e["ref"]) {
                if saved["scope"] != e["scope"]
                    || saved["id"] != e["id"]
                    || saved["profileId"] != e["profileId"]
                {
                    return Err("历史技能来源不匹配".into());
                }
                if e["scope"] != "project" {
                    let revision = saved["revision"].as_str().ok_or("历史技能版本无效")?;
                    files(s, e, revision)?;
                }
                *e = saved.clone();
            }
        }
    }
    guidance["skills"] = json!(selected);
    guidance["warnings"] = json!(warnings);
    Ok(json!(selected))
}
pub(crate) fn definition() -> Value {
    json!({"type":"function","function":{"name":"read_skill_file","description":"读取本轮关联的账户共享或智能体专属技能的 scripts、references、assets 文本文件。仅返回数据，不执行脚本；执行需使用 run_command 并遵守审批。使用 load_skill 返回的文件列表选择 path。","parameters":{"type":"object","properties":{"name":{"type":"string","maxLength":200},"path":{"type":"string","maxLength":200},"offset":{"type":"integer","minimum":0},"expectedSha256":{"type":"string","maxLength":64}},"required":["name","path"],"additionalProperties":false}}})
}
pub(crate) fn load(
    s: &Store,
    response: &str,
    tool: &str,
    args: &Value,
    root: &cap_std::fs::Dir,
) -> Result<Value, String> {
    let (raw, session): (String, String) = s
        .conn
        .query_row(
            "SELECT model_metadata,session_id FROM agent_messages WHERE id=?1",
            [response],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    project(s, &session)?;
    let metadata: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let bindings = metadata["skillBindings"]
        .as_array()
        .ok_or("本轮没有技能快照")?;
    let obj = args.as_object().ok_or("技能参数须为对象")?;
    if obj
        .keys()
        .any(|k| !matches!(k.as_str(), "name" | "offset" | "expectedSha256" | "path"))
    {
        return Err("技能参数包含未知字段".into());
    }
    let name = args["name"].as_str().ok_or("缺少技能标识")?;
    let candidates: Vec<_> = bindings
        .iter()
        .filter(|e| e["ref"] == name || e["name"] == name)
        .collect();
    if candidates.len() != 1 {
        return Err("技能未关联或名称重复，请使用本轮目录中的完整技能标识".into());
    }
    let entry = candidates[0];
    if entry["scope"] == "project" {
        if tool != "load_skill" {
            return Err("项目技能资源请通过项目 read_file 读取".into());
        }
        if args.get("path").is_some() {
            return Err("load_skill 不接受 path".into());
        }
        let mut a = args.clone();
        a["name"] = entry["name"].clone();
        if a.get("expectedSha256")
            .is_some_and(|v| v != &entry["sha256"])
        {
            return Err("项目技能校验值与本轮快照不一致".into());
        }
        a["expectedSha256"] = entry["sha256"].clone();
        let mut result = crate::agent_skills::load(
            root,
            &a,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )?;
        result["name"] = entry["ref"].clone();
        result["scope"] = json!("project");
        return Ok(result);
    }
    let allowed = managed(s)?
        .into_iter().chain(std::iter::once(builtin()))
        .find(|e| e["id"] == entry["id"] && e["ref"] == entry["ref"])
        .ok_or("技能不属于当前账户")?;
    if allowed["scope"] == "agent" && allowed["profileId"] != metadata["agentProfile"]["id"] {
        return Err("技能不属于本轮智能体".into());
    }
    let all = files(
        s,
        &allowed,
        entry["revision"].as_str().ok_or("技能版本无效")?,
    )?;
    let path = if tool == "load_skill" {
        if args.get("path").is_some() {
            return Err("load_skill 不接受 path".into());
        }
        "SKILL.md"
    } else {
        args["path"].as_str().ok_or("缺少文件路径")?
    };
    check_path(path)?;
    let text = all[path].as_str().ok_or("技能文件不存在")?;
    let sha = format!("{:x}", Sha256::digest(text.as_bytes()));
    if args
        .get("expectedSha256")
        .is_some_and(|v| v.as_str() != Some(&sha))
    {
        return Err("技能文件校验值不一致".into());
    }
    let offset = match args.get("offset") {
        None => 0,
        Some(v) => v.as_u64().ok_or("offset 必须是非负整数")? as usize,
    };
    let total = text.chars().count();
    if offset > total {
        return Err("offset 超出文件范围".into());
    }
    let mut page = String::new();
    for ch in text.chars().skip(offset) {
        if page.len() + ch.len_utf8() > 2200 {
            break;
        }
        page.push(ch);
    }
    let next = offset + page.chars().count();
    let mut result = json!({"name":entry["ref"],"scope":entry["scope"],"path":path,"sha256":sha,"revision":entry["revision"],"offset":offset,"totalChars":total,"nextOffset":if next<total{Some(next)}else{None},"files":all.as_object().unwrap().keys().collect::<Vec<_>>()});
    result[if tool == "load_skill" {
        "instructions"
    } else {
        "content"
    }] = json!(page);
    Ok(result)
}
