//! Portable account-scoped conversation snapshots. Credentials and executable
//! runtime state are deliberately outside this format; restore only adds rows.
use crate::{AppState, store::Store};
use base64::{Engine, engine::general_purpose::STANDARD};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
const FORMAT: &str = "cangling-keeper-conversations";
const MAX_BYTES: usize = 24 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Archive {
    snapshot: Snapshot,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    format: String,
    version: u32,
    created_at: i64,
    source_account: String,
    projects: Vec<Project>,
    sessions: Vec<Session>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Project {
    id: String,
    name: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Session {
    id: String,
    project_id: String,
    title: String,
    created_at: i64,
    updated_at: i64,
    pinned: bool,
    model_id: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    archived_at: i64,
    #[serde(default, skip_serializing_if = "is_zero")]
    deleted_at: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    attachments: Vec<Attachment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mcp_images: Vec<Value>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    draft: String,
    messages: Vec<Message>,
    versions: Vec<Version>,
    runs: Vec<Run>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Message {
    id: String,
    role: String,
    content: String,
    created_at: i64,
    metadata: Value,
    context: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    attachment_ids: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Attachment {
    id: String,
    name: String,
    mime_type: String,
    kind: String,
    size: u64,
    sha256: String,
    created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
    data: String,
}
fn attachment_bytes(attachment: &Attachment) -> Result<Vec<u8>, String> {
    if attachment.data.len() > (4usize * 1024 * 1024).div_ceil(3) * 4 {
        return Err("备份中的图片附件超过4 MiB".into());
    }
    STANDARD
        .decode(&attachment.data)
        .map_err(|_| "备份附件不是有效的 base64".into())
}
fn attachment_metadata(attachment: &Attachment) -> Result<Value, String> {
    let bytes = attachment_bytes(attachment)?;
    let mut meta =
        crate::agent_attachments::validate(&attachment.name, &attachment.mime_type, &bytes)?;
    if meta["kind"] != attachment.kind
        || meta["mimeType"] != attachment.mime_type
        || meta["size"] != attachment.size
        || meta["sha256"] != attachment.sha256
        || meta.get("width").and_then(Value::as_u64) != attachment.width.map(u64::from)
        || meta.get("height").and_then(Value::as_u64) != attachment.height.map(u64::from)
    {
        return Err("备份附件的类型、大小、校验值或尺寸与内容不一致".into());
    }
    meta["id"] = json!(attachment.id);
    if attachment.kind == "text" {
        meta["text"] = json!(std::str::from_utf8(&bytes).map_err(|_| "文本附件编码无效")?);
    }
    Ok(meta)
}
// Stored contexts contain canonical references and strings, never provider image_url parts.
fn validate_context(context: &[Value], attachments: &HashMap<&str, Value>) -> Result<(), String> {
    for (index, item) in context.iter().enumerate() {
        if !item.is_object()
            || (!matches!(item["role"].as_str(), Some("user" | "assistant" | "tool"))
                && !(index == 0 && item["role"] == "system"))
            || !matches!(
                item.get("content"),
                Some(Value::String(_)) | Some(Value::Null)
            )
            || (item["role"] == "user" && !item["content"].is_string())
            || item.as_object().is_some_and(|o| {
                o.keys().any(|k| {
                    !matches!(
                        k.as_str(),
                        "role"
                            | "content"
                            | "name"
                            | "tool_calls"
                            | "tool_call_id"
                            | "keeperAttachments"
                    )
                })
            })
        {
            return Err("历史上下文须为纯文本与本机会话附件引用".into());
        }
        if let Some(refs) = item.get("keeperAttachments") {
            let refs = refs
                .as_array()
                .filter(|v| v.len() <= crate::agent_attachments::MAX_ATTACHMENTS)
                .ok_or("历史上下文附件格式无效")?;
            if item["role"] != "user" {
                return Err("只有用户上下文消息可以关联附件".into());
            }
            let mut seen = HashSet::new();
            for reference in refs {
                let id = reference["id"].as_str().ok_or("历史上下文附件缺少ID")?;
                if !seen.insert(id) || attachments.get(id) != Some(reference) {
                    return Err("历史上下文附件不属于会话或元数据被修改".into());
                }
            }
        }
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Version {
    root_id: String,
    selected_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Run {
    id: String,
    tool: String,
    arguments: Value,
    status: String,
    output: String,
    created_at: i64,
    updated_at: i64,
    response_id: String,
    tool_call_id: String,
    step: i64,
    review: Option<Value>,
}
fn is_zero(value: &i64) -> bool {
    *value == 0
}
fn digest(snapshot: &Snapshot) -> Result<String, String> {
    Ok(crate::agent_actions::hash(
        serde_json::to_value(snapshot)
            .map_err(|_| "备份序列化失败")?
            .to_string()
            .as_bytes(),
    ))
}
fn sql<T>(result: rusqlite::Result<T>) -> Result<T, String> {
    result.map_err(|e| e.to_string())
}
fn parse(value: &str) -> Result<Value, String> {
    serde_json::from_str(value).map_err(|_| "历史记录中的 JSON 无效，无法备份".into())
}
fn counts(s: &Snapshot) -> Value {
    json!({"projects":s.projects.len(),"sessions":s.sessions.len(),"messages":s.sessions.iter().map(|s|s.messages.len()).sum::<usize>(),"runs":s.sessions.iter().map(|s|s.runs.len()).sum::<usize>(),"attachments":s.sessions.iter().map(|s|s.attachments.len()).sum::<usize>(),"mcpImages":s.sessions.iter().map(|s|s.mcp_images.len()).sum::<usize>(),"createdAt":s.created_at})
}
pub(crate) fn export(store: &Store) -> Result<Value, String> {
    let tx = sql(store.conn.unchecked_transaction())?;
    let owner = crate::agent_account::owner(store)?;
    let mut snapshot = Snapshot {
        format: FORMAT.into(),
        version: 1,
        created_at: chrono::Utc::now().timestamp_millis(),
        source_account: owner.clone(),
        projects: Vec::new(),
        sessions: Vec::new(),
    };
    let mut stmt=sql(store.conn.prepare("SELECT id,project_id,title,created_at,updated_at,pinned,server_id,archived_at,deleted_at FROM agent_sessions WHERE owner_key=?1 ORDER BY created_at,rowid"))?;
    let rows = sql(stmt.query_map([owner], |r| {
        Ok(Session {
            id: r.get(0)?,
            project_id: r.get(1)?,
            title: r.get(2)?,
            created_at: r.get(3)?,
            updated_at: r.get(4)?,
            pinned: r.get(5)?,
            model_id: r.get(6)?,
            archived_at: r.get(7)?,
            deleted_at: r.get(8)?,
            attachments: Vec::new(),
            mcp_images: Vec::new(),
            draft: String::new(),
            messages: Vec::new(),
            versions: Vec::new(),
            runs: Vec::new(),
        })
    }))?;
    let mut bytes = 0;
    for row in rows {
        let mut session = sql(row)?;
        session.mcp_images=crate::agent_mcp_images::export(store,&session.id)?;
        if !session.mcp_images.is_empty(){snapshot.version=snapshot.version.max(3);}
        session.draft=sql(store.conn.query_row("SELECT content FROM agent_drafts WHERE session_id=?1",[&session.id],|r|r.get(0)).optional())?.unwrap_or_default();
        if !session.draft.is_empty(){snapshot.version=4;}
        let mut attachment_stmt=sql(store.conn.prepare("SELECT id,name,mime_type,kind,data,sha256,created_at FROM agent_attachments WHERE session_id=?1 AND owner_key=?2 ORDER BY created_at,rowid"))?;
        let attachment_rows = sql(attachment_stmt.query_map(
            params![session.id, snapshot.source_account],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Vec<u8>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            },
        ))?;
        for raw in attachment_rows {
            let (id, name, mime, kind, data, hash, created) = sql(raw)?;
            let metadata = crate::agent_attachments::validate(&name, &mime, &data)?;
            session.attachments.push(Attachment {
                id,
                name,
                mime_type: mime,
                kind,
                size: data.len() as u64,
                sha256: hash,
                created_at: created,
                width: metadata["width"].as_u64().map(|v| v as u32),
                height: metadata["height"].as_u64().map(|v| v as u32),
                data: STANDARD.encode(data),
            });
        }
        if !session.attachments.is_empty() {
            snapshot.version = snapshot.version.max(2);
        }
        let mut messages=sql(store.conn.prepare("SELECT m.id,m.role,m.content,m.created_at,m.model_metadata,c.messages FROM agent_messages m LEFT JOIN agent_model_contexts c ON c.message_id=m.id WHERE m.session_id=?1 ORDER BY m.rowid"))?;
        let raw = sql(messages.query_map([&session.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        }))?;
        for row in raw {
            let (id, role, content, created_at, metadata, context) = sql(row)?;
            let linked = crate::agent_attachments::message_ids(store, &session.id, &id)?
                .as_array()
                .ok_or("附件关联记录无效")?
                .iter()
                .map(|v| v.as_str().map(str::to_owned).ok_or("附件关联记录无效"))
                .collect::<Result<Vec<_>, _>>()?;
            session.messages.push(Message {
                id,
                role,
                content,
                created_at,
                metadata: parse(&metadata)?,
                context: context
                    .map(|s| serde_json::from_str(&s).map_err(|_| "历史上下文无效".to_string()))
                    .transpose()?,
                attachment_ids: linked,
            });
        }
        let mut versions=sql(store.conn.prepare("SELECT root_id,selected_id FROM agent_reply_versions WHERE session_id=?1 ORDER BY rowid"))?;
        session.versions = sql(sql(versions.query_map([&session.id], |r| {
            Ok(Version {
                root_id: r.get(0)?,
                selected_id: r.get(1)?,
            })
        }))?
        .collect::<rusqlite::Result<Vec<_>>>())?;
        let mut runs=sql(store.conn.prepare("SELECT id,tool,arguments,status,output,created_at,updated_at,response_id,tool_call_id,step FROM agent_runs WHERE session_id=?1 ORDER BY rowid"))?;
        let raw = sql(runs.query_map([&session.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, i64>(9)?,
            ))
        }))?;
        for row in raw {
            let (
                id,
                tool,
                arguments,
                status,
                output,
                created_at,
                updated_at,
                response_id,
                tool_call_id,
                step,
            ) = sql(row)?;
            let review = crate::agent_runtime::review(store, &id)?;
            session.runs.push(Run {
                id,
                tool,
                arguments: parse(&arguments)?,
                status,
                output,
                created_at,
                updated_at,
                response_id,
                tool_call_id,
                step,
                review: if review.is_null() { None } else { Some(review) },
            });
        }
        bytes += serde_json::to_vec(&session)
            .map_err(|_| "备份序列化失败")?
            .len();
        if bytes > MAX_BYTES {
            return Err("当前账户历史及附件超过 24 MiB 备份上限，请清理回收站或不需要的附件；归档不会减小备份".into());
        }
        snapshot.sessions.push(session);
    }
    let mut projects=sql(store.conn.prepare("SELECT DISTINCT p.id,p.name FROM agent_projects p JOIN agent_sessions s ON s.project_id=p.id WHERE s.owner_key=?1 ORDER BY p.created_at,p.id"))?;
    snapshot.projects = sql(sql(projects.query_map([&snapshot.source_account], |r| {
        Ok(Project {
            id: r.get(0)?,
            name: r.get(1)?,
        })
    }))?
    .collect::<rusqlite::Result<Vec<_>>>())?;
    let archive = Archive {
        sha256: digest(&snapshot)?,
        snapshot,
    };
    let value = serde_json::to_value(archive).map_err(|_| "备份序列化失败")?;
    if value.to_string().len() > MAX_BYTES {
        return Err("备份超过 24 MiB 上限".into());
    }
    validate(&serde_json::from_value::<Archive>(value.clone()).map_err(|_| "备份格式无效")?)?;
    sql(tx.commit())?;
    Ok(value)
}
fn checked_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 128 || id.contains('\0') {
        Err("备份中的记录 ID 无效".into())
    } else {
        Ok(())
    }
}
fn checked_time(time: i64) -> Result<(), String> {
    if !(0..=253402300799000).contains(&time) {
        Err("备份中的时间无效".into())
    } else {
        Ok(())
    }
}
fn validate(archive: &Archive) -> Result<(), String> {
    let s = &archive.snapshot;
    if s.format != FORMAT || !matches!(s.version, 1 | 2 | 3 | 4) {
        return Err("不支持的 Keeper 备份格式或版本".into());
    }
    if archive.sha256 != digest(s)? {
        return Err("备份校验失败，文件可能已损坏或被修改".into());
    }
    checked_time(s.created_at)?;
    if s.source_account.is_empty() || s.source_account.len() > 512 {
        return Err("备份来源账号无效".into());
    }
    if s.sessions.len() > 10000
        || s.projects.len() > 10000
        || s.sessions
            .iter()
            .map(|s| s.messages.len() + s.runs.len() + s.attachments.len())
            .sum::<usize>()
            > 200000
    {
        return Err("备份中的记录数量超过限制".into());
    }
    let mut projects = HashSet::new();
    let mut ids = HashSet::new();
    for p in &s.projects {
        checked_id(&p.id)?;
        if !projects.insert(p.id.as_str()) || p.name.is_empty() || p.name.chars().count() > 80 {
            return Err("项目记录无效或重复".into());
        }
    }
    for session in &s.sessions {
        checked_id(&session.id)?;
        checked_time(session.created_at)?;
        checked_time(session.updated_at)?;
        checked_time(session.archived_at)?;
        checked_time(session.deleted_at)?;
        if !ids.insert(session.id.as_str())
            || !projects.contains(session.project_id.as_str())
            || session.title.chars().count() > 128
            || session.model_id.len() > 512
        {
            return Err("会话记录无效或重复".into());
        }
        if s.version == 1
            && (!session.attachments.is_empty()
                || session
                    .messages
                    .iter()
                    .any(|m| !m.attachment_ids.is_empty()))
        {
            return Err("附件需要第2版备份格式".into());
        }
        if session.draft.len()>32000 || (s.version<4 && !session.draft.is_empty()){return Err("草稿超出长度限制或备份版本不支持草稿".into());}
        if s.version<3 && !session.mcp_images.is_empty(){return Err("MCP 图片需要版本3备份".into());}
        let mut image_refs=HashMap::new();
        for image in &session.mcp_images {
            let checked=crate::agent_mcp_images::validate(image)?;
            if checked!=*image || image_refs.insert(image["sha256"].as_str().unwrap(),crate::agent_mcp_images::metadata(image)).is_some(){return Err("备份 MCP 图片校验失败或重复".into());}
        }
        let check_images=|content:&str|->Result<(),String>{for reference in crate::agent_mcp_images::references(content){if reference["sha256"].as_str().and_then(|h|image_refs.get(h))!=Some(&reference){return Err("备份 MCP 图片引用无效".into());}}Ok(())};
        for run in &session.runs{check_images(&run.output)?;}
        for message in &session.messages{if let Some(context)=&message.context{for item in context{if item["role"]=="tool"{check_images(item["content"].as_str().unwrap_or(""))?;}}}}
        let mut attachments = HashMap::new();
        for attachment in &session.attachments {
            checked_id(&attachment.id)?;
            checked_time(attachment.created_at)?;
            if !ids.insert(&attachment.id) {
                return Err("附件ID重复".into());
            }
            attachments.insert(attachment.id.as_str(), attachment_metadata(attachment)?);
        }
        let mut bound = HashSet::new();
        let mut messages = HashMap::<&str, (&str, &str)>::new();
        for m in &session.messages {
            checked_id(&m.id)?;
            checked_time(m.created_at)?;
            if !ids.insert(m.id.as_str())
                || !matches!(m.role.as_str(), "user" | "assistant" | "error")
                || !m.metadata.is_object()
            {
                return Err("消息记录无效或重复".into());
            }
            let parent = m.metadata.get(if m.role == "user" {
                "editOf"
            } else {
                "regenerateOf"
            });
            let root = if let Some(parent) = parent {
                let parent = parent.as_str().ok_or("消息版本引用无效")?;
                let (role, root) = messages
                    .get(parent)
                    .ok_or("消息版本引用了不存在或后续的消息")?;
                if *role != m.role {
                    return Err("消息版本角色不匹配".into());
                }
                *root
            } else {
                m.id.as_str()
            };
            if !m.attachment_ids.is_empty() && m.role != "user" {
                return Err("只有用户消息可以关联附件".into());
            }
            if m.attachment_ids.len() > crate::agent_attachments::MAX_ATTACHMENTS {
                return Err("每条消息最多4个附件".into());
            }
            let mut selected = HashSet::new();
            for id in &m.attachment_ids {
                if !selected.insert(id) || !attachments.contains_key(id.as_str()) {
                    return Err("消息附件重复或不属于当前会话".into());
                }
                bound.insert(id.as_str());
            }
            if let Some(context) = &m.context {
                validate_context(context, &attachments)?;
            }
            messages.insert(&m.id, (&m.role, root));
        }
        if attachments
            .keys()
            .filter(|id| !bound.contains(**id))
            .count()
            > crate::agent_attachments::MAX_ATTACHMENTS
        {
            return Err("每个会话最多4个待发送附件".into());
        }
        for m in &session.messages {
            for field in ["editOf", "regenerateOf", "promptMessageId"] {
                if let Some(value) = m.metadata.get(field) {
                    if !value.as_str().is_some_and(|id| messages.contains_key(id)) {
                        return Err("消息关联引用无效".into());
                    }
                    if (field == "editOf" && m.role != "user")
                        || (field != "editOf" && m.role != "assistant")
                    {
                        return Err("消息关联角色无效".into());
                    }
                }
            }
            if let Some(prompt) = m.metadata.get("promptMessageId") {
                if !prompt
                    .as_str()
                    .is_some_and(|id| messages.get(id).is_some_and(|(role, _)| *role == "user"))
                {
                    return Err("回复关联的用户消息不存在".into());
                }
            }
        }
        let mut roots = HashSet::new();
        for v in &session.versions {
            if !roots.insert(&v.root_id)
                || !messages
                    .get(v.root_id.as_str())
                    .is_some_and(|(_, r)| *r == v.root_id)
                || !messages
                    .get(v.selected_id.as_str())
                    .is_some_and(|(_, r)| *r == v.root_id)
            {
                return Err("选中的消息版本关系无效".into());
            }
        }
        for run in &session.runs {
            checked_id(&run.id)?;
            checked_time(run.created_at)?;
            checked_time(run.updated_at)?;
            if !ids.insert(run.id.as_str())
                || run.tool.is_empty()
                || run.tool.len() > 256
                || !run.arguments.is_object()
                || (!run.response_id.is_empty()
                    && !messages
                        .get(run.response_id.as_str())
                        .is_some_and(|(role, _)| *role == "assistant"))
                || run.step < 0
                || run.step > 100000
                || !matches!(
                    run.status.as_str(),
                    "running"
                        | "pending_approval"
                        | "approved"
                        | "succeeded"
                        | "failed"
                        | "rejected"
                        | "stopped"
                        | "interrupted"
                )
                || run.review.as_ref().is_some_and(|v| !v.is_object())
            {
                return Err("工具执行记录无效".into());
            }
        }
    }
    Ok(())
}
pub(crate) fn restore(core: &AppState, args: &Value) -> Result<Value, String> {
    let value = args.get("backup").ok_or("请选择备份文件")?;
    if value.to_string().len() > MAX_BYTES {
        return Err("备份文件不能超过 24 MiB".into());
    }
    let archive: Archive =
        serde_json::from_value(value.clone()).map_err(|_| "Keeper 备份结构无效")?;
    validate(&archive)?;
    let store = core.store.lock().map_err(|e| e.to_string())?;
    let owner = crate::agent_account::owner(&store)?;
    let mut summary = counts(&archive.snapshot);
    summary["accountKey"] = json!(owner);
    summary["differentAccount"] = json!(archive.snapshot.source_account != owner);
    if args["mode"] == "preview" {
        return Ok(summary);
    }
    if args["mode"] != "restore" || args["confirmed"] != true || args["expectedAccount"] != owner {
        return Err("请预览备份并确认恢复；账号发生变化时需要重新预览".into());
    }
    let busy:bool=sql(store.conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_model_pending p JOIN agent_sessions s ON s.id=p.session_id WHERE s.owner_key=?1)",[&owner],|r|r.get(0)))?;
    if busy {
        return Err("请先停止当前账户正在生成的回复，再恢复备份".into());
    }
    let models = crate::model::config(&store)?;
    let default = models["activeServerId"].as_str().unwrap_or("").to_owned();
    let mut created_dirs = Vec::new();
    let result = (|| -> Result<Value, String> {
        let tx = sql(store.conn.unchecked_transaction())?;
        let mut project_ids = HashMap::new();
        let mut session_ids = Vec::new();
        for p in &archive.snapshot.projects {
            let existing = sql(store
                .conn
                .query_row(
                    "SELECT id FROM agent_projects WHERE id=?1 AND name=?2",
                    params![p.id, p.name],
                    |r| r.get::<_, String>(0),
                )
                .optional())?;
            let id = if let Some(id) = existing {
                id
            } else {
                let id = uuid::Uuid::new_v4().to_string();
                let directory = crate::platform_dirs::home_dir()
                    .ok_or("无法确定用户目录")?
                    .join("cangling/projects")
                    .join(&id);
                std::fs::create_dir_all(&directory).map_err(|_| "无法创建恢复项目的目录")?;
                created_dirs.push(directory.clone());
                sql(store.conn.execute(
                    "INSERT INTO agent_projects(id,name,directory,created_at) VALUES (?1,?2,?3,?4)",
                    params![
                        id,
                        p.name,
                        directory.to_string_lossy(),
                        chrono::Utc::now().timestamp_millis()
                    ],
                ))?;
                id
            };
            project_ids.insert(p.id.clone(), id);
        }
        for session in &archive.snapshot.sessions {
            let id = uuid::Uuid::new_v4().to_string();
            session_ids.push(id.clone());
            let ids = session
                .messages
                .iter()
                .map(|m| (m.id.clone(), uuid::Uuid::new_v4().to_string()))
                .collect::<HashMap<_, _>>();
            let model = if models["servers"]
                .as_array()
                .is_some_and(|servers| servers.iter().any(|s| s["id"] == session.model_id))
            {
                session.model_id.as_str()
            } else {
                default.as_str()
            };
            sql(store.conn.execute("INSERT INTO agent_sessions(id,title,created_at,updated_at,project_id,pinned,title_custom,server_id,security_mode,owner_key,archived_at,deleted_at) VALUES (?1,?2,?3,?4,?5,?6,1,?7,'ask',?8,?9,?10)",params![id,session.title,session.created_at,session.updated_at,project_ids[&session.project_id],session.pinned,model,owner,session.archived_at,session.deleted_at]))?;
            crate::agent_mcp_images::save(&store,&id,&session.mcp_images)?;
            if !session.draft.is_empty(){sql(store.conn.execute("INSERT INTO agent_drafts(session_id,content,revision) VALUES (?1,?2,1)",params![id,session.draft]))?;}
            let mut attachment_ids = HashMap::new();
            for attachment in &session.attachments {
                let attachment_id = uuid::Uuid::new_v4().to_string();
                let bytes = attachment_bytes(attachment)?;
                sql(store.conn.execute("INSERT INTO agent_attachments(id,owner_key,session_id,name,mime_type,kind,data,sha256,created_at,size,width,height) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![attachment_id,owner,id,attachment.name,attachment.mime_type,attachment.kind,bytes,attachment.sha256,attachment.created_at,attachment.size as i64,attachment.width,attachment.height]))?;
                attachment_ids.insert(attachment.id.clone(), attachment_id);
            }
            for m in &session.messages {
                let mut metadata = m.metadata.clone();
                for field in ["editOf", "regenerateOf", "promptMessageId"] {
                    if let Some(old) = metadata[field].as_str().map(str::to_owned) {
                        metadata[field] = json!(ids.get(&old).ok_or("消息关联引用无效")?);
                    }
                }
                if metadata["generationStatus"] == "streaming" {
                    metadata["generationStatus"] = json!("interrupted");
                    metadata["context"] = json!({"status":"interrupted"});
                }
                sql(store.conn.execute("INSERT INTO agent_messages(id,session_id,role,content,created_at,model_metadata) VALUES (?1,?2,?3,?4,?5,?6)",params![ids[&m.id],id,m.role,m.content,m.created_at,metadata.to_string()]))?;
                for (position, attachment) in m.attachment_ids.iter().enumerate() {
                    sql(store.conn.execute("INSERT INTO agent_message_attachments(message_id,attachment_id,position) VALUES (?1,?2,?3)",params![ids[&m.id],attachment_ids[attachment],position as i64]))?;
                }
                if let Some(context) = &m.context {
                    let mut context = context.clone();
                    for item in &mut context {
                        if let Some(refs) = item
                            .get_mut("keeperAttachments")
                            .and_then(Value::as_array_mut)
                        {
                            for reference in refs {
                                let old = reference["id"].as_str().ok_or("上下文附件缺少ID")?;
                                reference["id"] =
                                    json!(attachment_ids.get(old).ok_or("上下文附件引用无效")?);
                            }
                        }
                    }
                    crate::model::validate_attachment_context(&store, &id, &context)?;
                    sql(store.conn.execute(
                        "INSERT INTO agent_model_contexts(message_id,messages) VALUES (?1,?2)",
                        params![ids[&m.id], json!(context).to_string()],
                    ))?;
                }
            }
            for v in &session.versions {
                sql(store.conn.execute("INSERT INTO agent_reply_versions(session_id,root_id,selected_id) VALUES (?1,?2,?3)",params![id,ids[&v.root_id],ids[&v.selected_id]]))?;
            }
            for run in &session.runs {
                let run_id = uuid::Uuid::new_v4().to_string();
                let active = matches!(
                    run.status.as_str(),
                    "running" | "pending_approval" | "approved"
                );
                let status = if active { "interrupted" } else { &run.status };
                let output = if active {
                    format!("{}\n从备份恢复，原操作已中断，不会重新执行。", run.output)
                } else {
                    run.output.clone()
                };
                let response = if run.response_id.is_empty() {
                    String::new()
                } else {
                    ids[&run.response_id].clone()
                };
                sql(store.conn.execute("INSERT INTO agent_runs(id,session_id,tool,arguments,status,output,created_at,updated_at,response_id,tool_call_id,step) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![run_id,id,run.tool,run.arguments.to_string(),status,output,run.created_at,run.updated_at,response,run.tool_call_id,run.step]))?;
                if let Some(review) = &run.review {
                    let mut review = review.clone();
                    review.as_object_mut().unwrap().remove("hash");
                    // Imported history is not proof of writes in the current filesystem.
                    review.as_object_mut().unwrap().remove("rollbackVersion");
                    let raw = review.to_string();
                    let hash = crate::agent_actions::hash(raw.as_bytes());
                    sql(store.conn.execute(
                        "INSERT INTO agent_reviews(run_id,review,hash) VALUES (?1,?2,?3)",
                        params![run_id, raw, hash],
                    ))?;
                }
            }
        }
        sql(tx.commit())?;
        summary["sessionIds"] = json!(session_ids);
        Ok(summary)
    })();
    if result.is_err() {
        for directory in created_dirs.iter().rev() {
            let _ = std::fs::remove_dir(directory);
        }
    }
    result
}
