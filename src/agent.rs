//! Persistent conversations, model tools and host-enforced approval policies.
//! Conversations and immutable approval requests survive browser refreshes in SQLite.
use crate::{AppState, store::Store};
use rusqlite::params;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) fn initialize(store: &Store) -> Result<(), String> {
    store.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_sessions (
        id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS agent_messages (
        id TEXT PRIMARY KEY, session_id TEXT NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, created_at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS idx_agent_messages_session ON agent_messages(session_id,created_at);
        CREATE TABLE IF NOT EXISTS agent_runs (
        id TEXT PRIMARY KEY, session_id TEXT NOT NULL, tool TEXT NOT NULL, arguments TEXT NOT NULL,
        status TEXT NOT NULL, output TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS idx_agent_runs_session ON agent_runs(session_id,created_at);
        UPDATE agent_runs SET status='interrupted',output='Keeper 已重启；执行结果未知，请核对远端状态后再决定是否重新执行。' WHERE status='running';")
        .map_err(|e|e.to_string())?;
    store.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_projects (
        id TEXT PRIMARY KEY, name TEXT NOT NULL, directory TEXT NOT NULL, created_at INTEGER NOT NULL);") .map_err(|e|e.to_string())?;
    let has_project: bool = store
        .conn
        .prepare("PRAGMA table_info(agent_sessions)")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
        .iter()
        .any(|name| name == "project_id");
    if !has_project {
        store
            .conn
            .execute_batch(
                "ALTER TABLE agent_sessions ADD COLUMN project_id TEXT NOT NULL DEFAULT 'general';",
            )
            .map_err(|e| e.to_string())?;
    }
    let columns=store.conn.prepare("PRAGMA table_info(agent_sessions)").map_err(|e|e.to_string())?
        .query_map([],|r|r.get::<_,String>(1)).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    if !columns.iter().any(|c|c=="pinned") {store.conn.execute_batch("ALTER TABLE agent_sessions ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;").map_err(|e|e.to_string())?;}
    if !columns.iter().any(|c|c=="title_custom") {store.conn.execute_batch("ALTER TABLE agent_sessions ADD COLUMN title_custom INTEGER NOT NULL DEFAULT 0;").map_err(|e|e.to_string())?;}
    let directory = project_directory("general")?;
    store
        .conn
        .execute(
            "INSERT OR IGNORE INTO agent_projects VALUES ('general','通用项目',?1,?2)",
            params![directory, now()],
        )
        .map_err(|e| e.to_string())?;
    store.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_model_pending (session_id TEXT PRIMARY KEY, message_id TEXT NOT NULL DEFAULT '');").map_err(|e|e.to_string())?;
    let columns=store.conn.prepare("PRAGMA table_info(agent_model_pending)").map_err(|e|e.to_string())?.query_map([],|r|r.get::<_,String>(1)).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    if !columns.iter().any(|c|c=="message_id"){store.conn.execute_batch("ALTER TABLE agent_model_pending ADD COLUMN message_id TEXT NOT NULL DEFAULT '';").map_err(|e|e.to_string())?;}

    let message_columns=store.conn.prepare("PRAGMA table_info(agent_messages)").map_err(|e|e.to_string())?.query_map([],|r|r.get::<_,String>(1)).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    if !message_columns.iter().any(|c|c=="model_metadata"){store.conn.execute_batch("ALTER TABLE agent_messages ADD COLUMN model_metadata TEXT NOT NULL DEFAULT '{}';").map_err(|e|e.to_string())?;}
    if !columns.iter().any(|c|c=="stop_requested"){store.conn.execute_batch("ALTER TABLE agent_model_pending ADD COLUMN stop_requested INTEGER NOT NULL DEFAULT 0;").map_err(|e|e.to_string())?;}
    store.conn.execute_batch("UPDATE agent_messages SET model_metadata=json_set(model_metadata,'$.generationStatus','interrupted','$.context.status','interrupted') WHERE id IN (SELECT message_id FROM agent_model_pending); INSERT INTO agent_messages(id,session_id,role,content,created_at) SELECT lower(hex(randomblob(16))),session_id,'error','Keeper 已重启，模型请求已中断；已生成的回复已保留，请重新发送消息。',CAST(strftime('%s','now') AS INTEGER)*1000 FROM agent_model_pending; DELETE FROM agent_model_pending;").map_err(|e|e.to_string())?;
    store.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_reply_versions (session_id TEXT NOT NULL,root_id TEXT NOT NULL,selected_id TEXT NOT NULL,PRIMARY KEY(session_id,root_id)); CREATE TABLE IF NOT EXISTS agent_model_contexts (message_id TEXT PRIMARY KEY,messages TEXT NOT NULL);").map_err(|e|e.to_string())?;
    crate::agent_runtime::initialize(store)?;
    crate::agent_security::initialize(store)?;
    crate::agent_account::initialize(store)?;
    crate::model::initialize(store)?;
    let tx=store.conn.unchecked_transaction().map_err(|e|e.to_string())?;
    let columns=store.conn.prepare("PRAGMA table_info(agent_sessions)").map_err(|e|e.to_string())?.query_map([],|r|r.get::<_,String>(1)).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    if !columns.iter().any(|c|c=="server_id"){
        store.conn.execute_batch("ALTER TABLE agent_sessions ADD COLUMN server_id TEXT NOT NULL DEFAULT '';").map_err(|e|e.to_string())?;
        let default=store.get_setting("agent_llm_active")?.unwrap_or_default();
        store.conn.execute("UPDATE agent_sessions SET server_id=?1",[default]).map_err(|e|e.to_string())?;
    }
    tx.commit().map_err(|e|e.to_string())?;
    Ok(())
}
fn project_directory(project: &str) -> Result<String, String> {
    let path = crate::platform_dirs::home_dir()
        .ok_or("无法确定用户主目录")?
        .join("cangling/projects")
        .join(project);
    std::fs::create_dir_all(&path).map_err(|e| format!("创建项目目录失败：{e}"))?;
    Ok(path.to_string_lossy().into_owned())
}
fn projects(store: &Store) -> Result<Value, String> {
    let mut stmt=store.conn.prepare("SELECT id,name,directory FROM agent_projects ORDER BY CASE WHEN id='general' THEN 0 ELSE 1 END,created_at,rowid").map_err(|e|e.to_string())?;
    let rows=stmt.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"directory":r.get::<_,String>(2)?}))).map_err(|e|e.to_string())?;
    Ok(json!(
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    ))
}
fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn text(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("缺少参数 {key}"))
}
fn require_session(store:&Store,session:&str)->Result<(),String>{if crate::agent_account::owns(store,session){Ok(())}else{Err("会话不存在或不属于当前账号".into())}}

fn message(store: &Store, session: &str, role: &str, content: &str) -> Result<(), String> {
    store
        .conn
        .execute(
            "INSERT INTO agent_messages(id,session_id,role,content,created_at,model_metadata) VALUES (?1,?2,?3,?4,?5,?6)",
            params![id(), session, role, content, now(),if role=="assistant"{json!({"source":"local"}).to_string()}else{"{}".to_owned()}],
        )
        .map_err(|e| e.to_string())?;
    store
        .conn
        .execute(
            "UPDATE agent_sessions SET updated_at=?2 WHERE id=?1",
            params![session, now()],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn tools() -> Value {
    let mut registry = json!([
        {"name":"list_hosts","title":"查看主机","description":"仅返回主机名称、地址和端口，不返回密码或私钥。","approval":false},
        {"name":"list_tunnels","title":"查看隧道","description":"查看配置的本地隧道。","approval":false},
        {"name":"proxy_status","title":"代理状态","description":"查看 Keeper 当前代理状态。","approval":false},
        {"name":"repository_status","title":"仓库状态","description":"查看本地软件仓库同步状态。","approval":false},
        {"name":"root_certificate_status","title":"根证书状态","description":"只读检查内嵌根证书的安装状态。","approval":false},
        {"name":"ssh_execute","title":"远程命令","description":"在指定主机执行命令；每次都必须确认目标及完整命令。","approval":true,
        "inputSchema":{"type":"object","required":["hostId","command"],"properties":{"hostId":{"type":"string"},"command":{"type":"string"}}}}
    ]);
    for tool in registry.as_array_mut().unwrap() {
        if tool.get("inputSchema").is_none() {
            tool["inputSchema"] =
                json!({"type":"object","properties":{},"additionalProperties":false});
        }
    }
    registry
}
fn sessions(store: &Store) -> Result<Value, String> {
    let mut stmt = store
        .conn
        .prepare(
            "SELECT id,title,updated_at,project_id,pinned FROM agent_sessions WHERE owner_key=?1 ORDER BY updated_at DESC,rowid DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows=stmt.query_map([crate::agent_account::owner(store)?],|r|Ok(json!({"id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"updatedAt":r.get::<_,i64>(2)?,"projectId":r.get::<_,String>(3)?,"pinned":r.get::<_,bool>(4)?}))).map_err(|e|e.to_string())?;
    Ok(Value::Array(
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?,
    ))
}
// Keep the physical rows immutable; resolve alternative replies at the original position.
fn reply_messages(store: &Store, session: &str, boundary: Option<&str>) -> Result<Vec<Value>, String> {
    let mut stmt=store.conn.prepare("SELECT id,role,content,created_at,model_metadata FROM agent_messages WHERE session_id=?1 AND (?2 IS NULL OR rowid<(SELECT rowid FROM agent_messages WHERE id=?2 AND session_id=?1)) ORDER BY rowid").map_err(|e|e.to_string())?;
    stmt.query_map(params![session,boundary],|r|Ok(json!({"id":r.get::<_,String>(0)?,"role":r.get::<_,String>(1)?,"content":r.get::<_,String>(2)?,"createdAt":r.get::<_,i64>(3)?,"modelMetadata":serde_json::from_str::<Value>(&r.get::<_,String>(4)?).unwrap_or(json!({}))}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}
fn reply_groups(store: &Store, session: &str, messages: &[Value]) -> Result<Vec<Value>, String> {
    let mut roots=std::collections::HashMap::<String,String>::new();
    let mut groups=Vec::<Value>::new();
    for message in messages.iter().filter(|m|matches!(m["role"].as_str(),Some("assistant"|"user"))) {
        let id=message["id"].as_str().unwrap();
        // A parent must be an earlier assistant in this session, which also prevents cycles.
        let root=message["modelMetadata"][if message["role"]=="user"{"editOf"}else{"regenerateOf"}].as_str().and_then(|p|roots.get(p)).cloned().unwrap_or_else(||id.to_owned());
        roots.insert(id.to_owned(),root.clone());
        if let Some(group)=groups.iter_mut().find(|g|g["rootId"]==root){group["versionIds"].as_array_mut().unwrap().push(json!(id));}
        else{groups.push(json!({"rootId":root,"versionIds":[id]}));}
    }
    for group in &mut groups {
        let selected=store.conn.query_row("SELECT selected_id FROM agent_reply_versions WHERE session_id=?1 AND root_id=?2",params![session,group["rootId"].as_str().unwrap()],|r|r.get::<_,String>(0)).ok();
        let ids=group["versionIds"].as_array().unwrap();
        group["selectedId"]=selected.filter(|id|ids.iter().any(|v|v.as_str()==Some(id))).map(Value::String).unwrap_or_else(||ids.last().unwrap().clone());
    }
    let user_selections=groups.iter().filter(|g|messages.iter().any(|m|m["id"]==g["rootId"] && m["role"]=="user")).map(|g|(g["rootId"].clone(),g["selectedId"].clone(),g["versionIds"].clone())).collect::<Vec<_>>();
    for group in &mut groups {
        let Some(root)=messages.iter().position(|m|m["id"]==group["rootId"] && m["role"]=="assistant") else {continue};
        let Some(user)=messages[..root].iter().rev().find(|m|m["role"]=="user") else {continue};
        let Some((_,selected,ids))=user_selections.iter().find(|(_,_,ids)|ids.as_array().unwrap().contains(&user["id"])) else {continue};
        let eligible=group["versionIds"].as_array().unwrap().iter().filter(|id|{
            let m=messages.iter().find(|m|&m["id"]==*id).unwrap();
            let prompt=m["modelMetadata"].get("promptMessageId").unwrap_or(&user["id"]);
            prompt==selected || (!ids.as_array().unwrap().contains(prompt) && selected==&user["id"])
        }).cloned().collect::<Vec<_>>();
        group["allVersionIds"]=group["versionIds"].clone();
        if !eligible.contains(&group["selectedId"]){group["selectedId"]=eligible.last().cloned().unwrap_or(Value::Null);}
        group["versionIds"]=json!(eligible);
    }
    Ok(groups)
}
pub(crate) fn context_messages(store: &Store, session: &str, boundary: &str) -> Result<Vec<Value>, String> {
    let messages=reply_messages(store,session,Some(boundary))?;
    let groups=reply_groups(store,session,&messages)?;
    let mut result=Vec::new();
    for message in &messages {
        let chosen=if matches!(message["role"].as_str(),Some("assistant"|"user")) {
            let Some(group)=groups.iter().find(|g|g["rootId"]==message["id"]) else {continue};
            match messages.iter().find(|m|m["id"]==group["selectedId"]){Some(m)=>m,None=>continue}
        } else {message};
        if matches!(chosen["role"].as_str(),Some("user"|"assistant")) && !chosen["content"].as_str().unwrap_or("").is_empty(){result.push(json!({"role":chosen["role"],"content":chosen["content"]}));}
    }
    Ok(result)
}
fn detail(store: &Store, session: &str) -> Result<Value, String> {
    require_session(store, session)?;
    let messages = reply_messages(store, session, None)?;
    let groups = reply_groups(store, session, &messages)?;
    let mut stmt=store.conn.prepare("SELECT id,tool,arguments,status,output,created_at,response_id,tool_call_id,step,updated_at FROM agent_runs WHERE session_id=?1 ORDER BY rowid").map_err(|e|e.to_string())?;
    let rows=stmt.query_map([session],|r|Ok(json!({"id":r.get::<_,String>(0)?,"tool":r.get::<_,String>(1)?,"arguments":serde_json::from_str::<Value>(&r.get::<_,String>(2)?).unwrap_or(Value::Null),"status":r.get::<_,String>(3)?,"output":r.get::<_,String>(4)?,"createdAt":r.get::<_,i64>(5)?,"responseId":r.get::<_,String>(6)?,"toolCallId":r.get::<_,String>(7)?,"step":r.get::<_,i64>(8)?,"updatedAt":r.get::<_,i64>(9)?}))).map_err(|e|e.to_string())?;
    let runs = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut runs=runs;for run in &mut runs {run["review"]=crate::agent_runtime::review(store,run["id"].as_str().unwrap())?;}
    let pending:bool=store.conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_model_pending WHERE session_id=?1)",[session],|r|r.get(0)).map_err(|e|e.to_string())?;
    let model=crate::model::session_config(store,session)?;
    Ok(json!({"sessionId":session,"securityMode":crate::agent_security::mode(store,session),"model":model,"messages":messages,"replyGroups":groups,"runs":runs,"tasks":crate::agent_runtime::tasks(store,session)?,"modelConnected":model["enabled"]==true && model["hasApiKey"]==true,"modelPending":pending,"stopRequested":store.conn.query_row("SELECT stop_requested FROM agent_model_pending WHERE session_id=?1",[session],|r|r.get::<_,bool>(0)).unwrap_or(false),"streamingMessageId":store.conn.query_row("SELECT message_id FROM agent_model_pending WHERE session_id=?1",[session],|r|r.get::<_,String>(0)).unwrap_or_default()}))
}
fn host_snapshot(store: &Store, host_id: &str) -> Result<Value, String> {
    let h = store.get_host(host_id)?;
    Ok(json!({"id":h.id,"name":h.name,"hostname":h.hostname,"port":h.port,"username":h.username}))
}
fn create_run(
    store: &Store,
    session: &str,
    tool: &str,
    mut arguments: Value,
) -> Result<Value, String> {
    require_session(store, session)?;
    let registry = tools();
    let spec = registry
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == tool)
        .ok_or("未知工具")?;
    let approval = spec["approval"].as_bool().unwrap_or(true);
    if !arguments.is_object() {
        return Err("工具参数必须为对象".into());
    }
    if tool == "ssh_execute" {
        let host_id = text(&arguments, "hostId")?;
        let command = text(&arguments, "command")?;
        if command.len() > 8192 || command.contains('\0') {
            return Err("命令过长或包含空字符".into());
        }
        arguments =
            json!({"hostId":host_id,"command":command,"target":host_snapshot(store,&host_id)?});
    } else {
        arguments = json!({});
    }
    let run = id();
    let status = if approval {
        "pending_approval"
    } else {
        "running"
    };
    store.conn.execute("INSERT INTO agent_runs (id,session_id,tool,arguments,status,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?6)",params![run,session,tool,arguments.to_string(),status,now()]).map_err(|e|e.to_string())?;
    store.conn.execute("UPDATE agent_sessions SET updated_at=?2,title=CASE WHEN title='新会话' AND title_custom=0 THEN ?3 ELSE title END WHERE id=?1",params![session,now(),spec["title"].as_str().unwrap_or(tool)]).map_err(|e|e.to_string())?;
    Ok(json!({"id":run,"status":status}))
}
fn decide(store: &Store, run: &str, approved: bool) -> Result<(), String> {
    let status = if approved { "running" } else { "rejected" };
    let count=store.conn.execute("UPDATE agent_runs SET status=?2,updated_at=?3 WHERE id=?1 AND status='pending_approval' AND response_id=''",params![run,status,now()]).map_err(|e|e.to_string())?;
    if count != 1 {
        return Err("工具请求已处理或不存在，不能重复执行".into());
    }
    Ok(())
}
async fn execute(core: &Arc<AppState>, tool: &str, args: &Value) -> Result<Value, String> {
    match tool {
        "list_hosts" => {
            let store = core.store.lock().map_err(|e| e.to_string())?;
            let rows=store.list_hosts()?.into_iter().map(|h|json!({"id":h.id,"name":h.name,"hostname":h.hostname,"port":h.port,"username":h.username,"catalog":h.catalog})).collect::<Vec<_>>();
            Ok(json!(rows))
        }
        "list_tunnels" => {
            let store = core.store.lock().map_err(|e| e.to_string())?;
            Ok(json!(store.list_tunnels()?.into_iter().map(|t|json!({"id":t.id,"name":t.name,"localPort":t.local_port,"remoteHost":t.remote_host,"remotePort":t.remote_port,"type":t.tunnel_type})).collect::<Vec<_>>()))
        }
        "proxy_status" => {
            serde_json::to_value(crate::current_proxy_status(core)?).map_err(|e| e.to_string())
        }
        "repository_status" => {
            serde_json::to_value(crate::repo::web_repo_status(core)).map_err(|e| e.to_string())
        }
        "root_certificate_status" => serde_json::to_value(
            tokio::task::spawn_blocking(crate::root_certificate::status)
                .await
                .map_err(|e| e.to_string())??,
        )
        .map_err(|e| e.to_string()),
        "ssh_execute" => {
            let host_id = text(args, "hostId")?;
            let command = text(args, "command")?;
            let (host, auth) = {
                let store = core.store.lock().map_err(|e| e.to_string())?;
                if host_snapshot(&store, &host_id)? != args["target"] {
                    return Err("主机配置已改变，请重新创建命令请求并确认".into());
                }
                let h = store.get_host(&host_id)?;
                let auth = crate::resolve_auth(&store, &h.auth, &core.data_dir)?;
                (h, auth)
            };
            let output = tokio::time::timeout(
                std::time::Duration::from_secs(60),
                crate::ssh::execute(&host, &command, &auth),
            )
            .await
            .map_err(|_| {
                "等待远程命令超时；远端命令可能仍在运行，请核对后再执行。".to_string()
            })??;
            Ok(json!(output))
        }
        _ => Err("未知工具".into()),
    }
}
fn launch(core: Arc<AppState>, run: String) {
    tokio::spawn(async move {
        let request = (|| -> Result<(String, Value), String> {
            let store = core.store.lock().map_err(|e| e.to_string())?;
            store
                .conn
                .query_row(
                    "SELECT tool,arguments FROM agent_runs WHERE id=?1 AND status='running'",
                    [&run],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .map_err(|e| e.to_string())
                .and_then(|(tool, args)| {
                    Ok((
                        tool,
                        serde_json::from_str(&args).map_err(|e| e.to_string())?,
                    ))
                })
        })();
        let result = match request {
            Ok((tool, args)) => execute(&core, &tool, &args).await,
            Err(e) => Err(e),
        };
        let (status, output) = match result {
            Ok(v) => (
                if v.get("exit_status")
                    .and_then(Value::as_i64)
                    .is_some_and(|code| code != 0)
                {
                    "failed"
                } else {
                    "succeeded"
                },
                serde_json::to_string_pretty(&v).unwrap_or_default(),
            ),
            Err(e) => ("failed", e),
        };
        let output: String = output.chars().take(65536).collect();
        if let Ok(store) = core.store.lock() {
            let _=store.conn.execute("UPDATE agent_runs SET status=?2,output=?3,updated_at=?4 WHERE id=?1 AND status='running'",params![run,status,output,now()]);
        }
    });
}
fn launch_model(core:Arc<AppState>,session:String,response_id:String,cancel_rx:tokio::sync::oneshot::Receiver<()>,sink:Arc<dyn crate::EventSink>){
                let model_core=core;let model_session=session;
                tokio::spawn(async move{
                    let progress_sink=sink.clone();
                    let response=crate::model::reply(&model_core,&model_session,&response_id,cancel_rx,sink.clone(),|content|{
                        let store=model_core.store.lock().map_err(|e|e.to_string())?;
                        store.conn.execute("UPDATE agent_messages SET content=?2 WHERE id=?1 AND session_id=?3",params![response_id,content,model_session]).map_err(|e|e.to_string())?;
                        progress_sink.emit_json("agent-model-stream",json!({"sessionId":model_session,"messageId":response_id,"content":content}));Ok(())
                    }).await;
                    if let Ok(store)=model_core.store.lock(){
                        if let Ok(tx)=store.conn.unchecked_transaction(){
                            let status=match &response{Ok(crate::model::ReplyFinish::Complete)=>"complete",Ok(crate::model::ReplyFinish::Stopped)=>"stopped",Err(_)=>"failed"};
                            let _=store.conn.execute("UPDATE agent_messages SET model_metadata=json_set(model_metadata,'$.generationStatus',?2) WHERE id=?1",params![response_id,status]);
                            let _=crate::agent_runtime::finish(&store,&model_session,&response_id,status);
                            let result=match response{Ok(_)=>Ok(()),Err(error)=>message(&store,&model_session,"error",&error)};
                            if result.is_ok(){let _=store.conn.execute("DELETE FROM agent_model_pending WHERE session_id=?1",[&model_session]);let _=tx.commit();}
                        }
                    }
                    if let Ok(mut active)=model_core.model_cancellations.lock(){active.remove(&response_id);}
                    sink.emit_json("agent-model-finished",json!({"sessionId":model_session}));
                });
}
pub(crate) async fn dispatch(
    core: Arc<AppState>,
    command: &str,
    args: Value,
    sink: Arc<dyn crate::EventSink>,
) -> Result<Value, String> {
    match command {
        "agent_cis_usage" => crate::cis_models::call(&core,reqwest::Method::GET,if args["all"]==true {"/admin/usage"} else {"/usage"},None).await,
        "agent_cis_models_admin" => crate::cis_models::call(&core,reqwest::Method::GET,"/admin",None).await,
        "agent_cis_model_save" => {let saved=crate::cis_models::call(&core,reqwest::Method::POST,"/admin/save",Some(args)).await?;let refreshed=crate::cis_models::refresh(&core).await;Ok(json!({"id":saved["id"],"refreshError":refreshed.err()}))},
        "agent_cis_model_delete" => {let id=text(&args,"id")?;if id.len()>64 || !id.chars().all(|c|c.is_ascii_alphanumeric()||c=='-'||c=='_'){return Err("模型 ID 无效".into());}crate::cis_models::call(&core,reqwest::Method::POST,&format!("/admin/{id}/delete"),Some(json!({}))).await?;let refreshed=crate::cis_models::refresh(&core).await;Ok(json!({"refreshError":refreshed.err()}))},
        "agent_cis_models_refresh" => crate::cis_models::refresh(&core).await,
        "agent_model_config" => {let store=core.store.lock().map_err(|e|e.to_string())?;crate::model::config(&store)}
        "agent_model_save" => {let store=core.store.lock().map_err(|e|e.to_string())?;crate::model::save(&store,&args)}
        "agent_model_delete" => {let store=core.store.lock().map_err(|e|e.to_string())?;crate::model::delete(&store,&text(&args,"id")?)}
        "agent_model_test" => crate::model::test(&core,args["id"].as_str()).await,
        "agent_session_model" => {
            let store=core.store.lock().map_err(|e|e.to_string())?;
            let session=text(&args,"sessionId")?;require_session(&store,&session)?;let id=text(&args,"id")?;
            let pending:bool=store.conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_model_pending WHERE session_id=?1)",[&session],|r|r.get(0)).map_err(|e|e.to_string())?;
            if pending{return Err("当前会话正在生成回复，完成后才能切换模型".into());}
            let config=crate::model::config(&store)?;
            if !config["servers"].as_array().unwrap().iter().any(|s|s["id"]==id && s["enabled"]==true && s["hasApiKey"]==true){return Err("该服务未启用或未配置 API Key，请重新选择".into());}
            store.conn.execute("UPDATE agent_sessions SET server_id=?2 WHERE id=?1",params![session,id]).map_err(|e|e.to_string())?;detail(&store,&session)
        }
        "agent_model_select" => {let store=core.store.lock().map_err(|e|e.to_string())?;crate::model::select(&store,&text(&args,"id")?)}
        "agent_bootstrap" => {
            let store = core.store.lock().map_err(|e| e.to_string())?;
            Ok(
                json!({"accountKey":crate::agent_account::owner(&store)?,"projects":projects(&store)?,"sessions":sessions(&store)?,"tools":tools(),"modelConnected":crate::model::connected(&store),"model":crate::model::config(&store)?,"hosts":store.list_hosts()?.iter().map(|h|json!({"id":h.id,"name":h.name,"hostname":h.hostname})).collect::<Vec<_>>()}),
            )
        }
        "agent_pin_session" => {
            let store=core.store.lock().map_err(|e|e.to_string())?;
            let session=text(&args,"sessionId")?;require_session(&store,&session)?;
            let pinned=args.get("pinned").and_then(Value::as_bool).ok_or("缺少置顶状态")?;
            store.conn.execute("UPDATE agent_sessions SET pinned=?2 WHERE id=?1",params![session,pinned]).map_err(|e|e.to_string())?;
            Ok(json!({"pinned":pinned}))
        }
        "agent_new_project" => {
            let name = text(&args, "name")?.trim().to_owned();
            if name.chars().count() > 80 {
                return Err("项目名称不能超过80字".into());
            }
            let project = id();
            let directory = project_directory(&project)?;
            let store = core.store.lock().map_err(|e| e.to_string())?;
            if let Err(error) = store.conn.execute(
                "INSERT INTO agent_projects VALUES (?1,?2,?3,?4)",
                params![project, name, directory, now()],
            ) {
                let _ = std::fs::remove_dir(&directory);
                return Err(error.to_string());
            }
            Ok(json!({"id":project,"name":name,"directory":directory}))
        }
        "agent_rename_session" => {
            let session=text(&args,"sessionId")?;let title=text(&args,"title")?.trim().to_owned();
            if title.chars().count()>128 || title.chars().any(char::is_control){return Err("标题最多 128 个字符，不能包含换行或控制字符。".into());}
            let store=core.store.lock().map_err(|e|e.to_string())?;require_session(&store,&session)?;
            store.conn.execute("UPDATE agent_sessions SET title=?2,title_custom=1,updated_at=?3 WHERE id=?1",params![session,title,now()]).map_err(|e|e.to_string())?;
            Ok(json!({"id":session,"title":title}))
        }
        "agent_security_mode" => {
            let session=text(&args,"sessionId")?;
            let store=core.store.lock().map_err(|e|e.to_string())?;require_session(&store,&session)?;
            crate::agent_security::set(&store,&session,&args)?;detail(&store,&session)
        }
        "agent_new_session" => {
            let store = core.store.lock().map_err(|e| e.to_string())?;
            let project = args
                .get("projectId")
                .and_then(Value::as_str)
                .unwrap_or("general");
            let exists: bool = store
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM agent_projects WHERE id=?1)",
                    [project],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !exists {
                return Err("项目不存在".into());
            }
            let session = id();
            store
                .conn
                .execute(
                    "INSERT INTO agent_sessions (id,title,created_at,updated_at,project_id,server_id,owner_key) VALUES (?1,'新会话',?2,?2,?3,?4,?5)",
                    params![session, now(),project,crate::model::config(&store)?["activeServerId"].as_str().unwrap_or(""),crate::agent_account::owner(&store)?],
                )
                .map_err(|e| e.to_string())?;
            Ok(json!({"id":session}))
        }
        "agent_session" => {
            let store = core.store.lock().map_err(|e| e.to_string())?;
            detail(&store, &text(&args, "sessionId")?)
        }
        "agent_stop_generation" => {
            let session=text(&args,"sessionId")?;let expected=text(&args,"messageId")?;
            let store=core.store.lock().map_err(|e|e.to_string())?;require_session(&store,&session)?;
            let current=store.conn.query_row("SELECT message_id FROM agent_model_pending WHERE session_id=?1",[&session],|r|r.get::<_,String>(0)).unwrap_or_default();
            if current.is_empty(){return detail(&store,&session);}
            if current!=expected{return Err("当前请求已变化，未停止新的回复，请刷新后重试。".into());}
            store.conn.execute("UPDATE agent_model_pending SET stop_requested=1 WHERE session_id=?1",[&session]).map_err(|e|e.to_string())?;
            if let Some(cancel)=core.model_cancellations.lock().map_err(|e|e.to_string())?.remove(&current){let _=cancel.send(());}
            detail(&store,&session)
        }
        "agent_reply_version" => {
            let session=text(&args,"sessionId")?;let root=text(&args,"rootId")?;let selected=text(&args,"messageId")?;
            let store=core.store.lock().map_err(|e|e.to_string())?;require_session(&store,&session)?;
            let busy:bool=store.conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_model_pending WHERE session_id=?1)",[&session],|r|r.get(0)).map_err(|e|e.to_string())?;
            if busy{return Err("请先停止生成或等待完成，再切换回复版本。".into());}
            let messages=reply_messages(&store,&session,None)?;let groups=reply_groups(&store,&session,&messages)?;
            if !groups.iter().any(|g|g["rootId"]==root && g["versionIds"].as_array().unwrap().iter().any(|id|id==&json!(selected))){return Err("回复版本不存在或不属于该会话。".into());}
            store.conn.execute("INSERT INTO agent_reply_versions(session_id,root_id,selected_id) VALUES (?1,?2,?3) ON CONFLICT(session_id,root_id) DO UPDATE SET selected_id=excluded.selected_id",params![session,root,selected]).map_err(|e|e.to_string())?;
            detail(&store,&session)
        }
        "agent_regenerate" | "agent_edit_message" => {
            let session=text(&args,"sessionId")?;let mut original=text(&args,"messageId")?;
            let editing=command=="agent_edit_message";
            let edit_content=if editing{let value=text(&args,"content")?;if value.len()>32000{return Err("消息过长".into());}Some(value)}else{None};
            let response_id=id();let (cancel_tx,cancel_rx)=tokio::sync::oneshot::channel();
            {
                let store=core.store.lock().map_err(|e|e.to_string())?;require_session(&store,&session)?;
                let tx=store.conn.unchecked_transaction().map_err(|e|e.to_string())?;
                let busy:bool=store.conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_model_pending WHERE session_id=?1)",[&session],|r|r.get(0)).map_err(|e|e.to_string())?;
                if busy{return Err("当前会话正在生成回复，请先停止或等待完成。".into());}
                let messages=reply_messages(&store,&session,None)?;let groups=reply_groups(&store,&session,&messages)?;
                let mut edited_user=None;
                let mut edited_context=None;
                if editing {
                    let user=messages.iter().find(|m|m["id"]==original && m["role"]=="user").ok_or("用户消息不存在或不属于该会话")?;
                    let user_group=groups.iter().find(|g|g["versionIds"].as_array().unwrap().contains(&user["id"])).ok_or("用户消息版本不存在")?;
                    if user_group["selectedId"]!=user["id"]{return Err("请先切换到要编辑的消息版本。".into());}
                    if user["content"]==json!(edit_content){return Err("消息内容未修改。".into());}
                    let root_position=messages.iter().position(|m|m["id"]==user_group["rootId"]).unwrap();
                    let reply=messages[root_position+1..].iter().take_while(|m|m["role"]!="user").find(|m|m["role"]=="assistant").ok_or("该消息尚无可重新生成的回复")?;
                    let reply_group=groups.iter().find(|g|g["rootId"]==reply["id"]).ok_or("原回复不存在")?;
                    original=reply_group["selectedId"].as_str().ok_or("当前消息版本没有回复")?.to_owned();
                    let mut context=store.conn.query_row("SELECT messages FROM agent_model_contexts WHERE message_id=?1",[&original],|r|r.get::<_,String>(0)).ok().and_then(|v|serde_json::from_str::<Vec<Value>>(&v).ok()).unwrap_or(context_messages(&store,&session,reply["id"].as_str().unwrap())?);
                    let last=context.iter().rposition(|m|m["role"]=="user").ok_or("原始问题上下文不存在")?;context.truncate(last+1);context[last]["content"]=json!(edit_content.as_ref().unwrap());
                    if context.first().is_none_or(|m|m["role"]!="system"){context.insert(0,json!({"role":"system","content":crate::model::SYSTEM_PROMPT}));}
                    let user_id=id();
                    store.conn.execute("INSERT INTO agent_messages(id,session_id,role,content,created_at,model_metadata) VALUES (?1,?2,'user',?3,?4,?5)",params![user_id,session,edit_content.as_ref().unwrap(),now(),json!({"editOf":user["id"]}).to_string()]).map_err(|e|e.to_string())?;
                    store.conn.execute("INSERT INTO agent_reply_versions(session_id,root_id,selected_id) VALUES (?1,?2,?3) ON CONFLICT(session_id,root_id) DO UPDATE SET selected_id=excluded.selected_id",params![session,user_group["rootId"].as_str().unwrap(),user_id]).map_err(|e|e.to_string())?;
                    edited_user=Some(user_id);edited_context=Some(context);
                }
                let raw:String=store.conn.query_row("SELECT model_metadata FROM agent_messages WHERE id=?1 AND session_id=?2 AND role='assistant'",params![original,session],|r|r.get(0)).map_err(|_|"原回复不存在或不属于该会话")?;
                let mut metadata:Value=serde_json::from_str(&raw).map_err(|_|"原回复的模型信息无效")?;
                let server=text(&metadata,"serverId")?;
                if metadata["source"]!="llm" || metadata["requestedModel"].as_str().unwrap_or("").is_empty(){return Err("该回复未记录原模型，无法重新生成。".into());}
                let config=crate::model::config(&store)?;
                let current=config["servers"].as_array().unwrap().iter().find(|s|s["id"]==server && s["enabled"]==true && s["hasApiKey"]==true).ok_or("原模型服务已停用或密钥不可用，请先在模型服务中恢复。")?;
                let (url,model,_,provider)=crate::model::credentials_for(&store,Some(&server))?;
                if provider=="cis-proxy" && (metadata["requestedModel"].as_str()!=Some(model.as_str()) || metadata["cisRevision"]!=current["revision"]){return Err("CIS 模型已变更，无法使用原模型重新生成。".into());}
                if metadata.get("baseUrl").is_some_and(|v|v!=&json!(url)) || metadata.get("provider").is_some_and(|v|v!=&json!(provider)){return Err("原模型服务的地址或 API 类型已变更，请恢复原配置后重试。".into());}

                let root=groups.iter().find(|g|g["versionIds"].as_array().unwrap().iter().any(|id|id==&json!(original))).ok_or("原回复不存在")?["rootId"].as_str().unwrap().to_owned();
                store.conn.execute("INSERT INTO agent_reply_versions(session_id,root_id,selected_id) VALUES (?1,?2,?3) ON CONFLICT(session_id,root_id) DO UPDATE SET selected_id=excluded.selected_id",params![session,root,response_id]).map_err(|e|e.to_string())?;
                metadata["generationStatus"]=json!("streaming");metadata["regenerateOf"]=json!(original);
                if let Some(user)=edited_user{metadata["promptMessageId"]=json!(user);}
                if let Some(context)=edited_context{store.conn.execute("INSERT INTO agent_model_contexts(message_id,messages) VALUES (?1,?2)",params![response_id,json!(context).to_string()]).map_err(|e|e.to_string())?;}

                store.conn.execute("INSERT INTO agent_messages(id,session_id,role,content,created_at,model_metadata) VALUES (?1,?2,'assistant','',?3,?4)",params![response_id,session,now(),metadata.to_string()]).map_err(|e|e.to_string())?;
                store.conn.execute("INSERT INTO agent_model_pending(session_id,message_id) VALUES (?1,?2)",params![session,response_id]).map_err(|e|e.to_string())?;
                store.conn.execute("UPDATE agent_sessions SET updated_at=?2 WHERE id=?1",params![session,now()]).map_err(|e|e.to_string())?;
                core.model_cancellations.lock().map_err(|e|e.to_string())?.insert(response_id.clone(),cancel_tx);tx.commit().map_err(|e|e.to_string())?;
            }
            launch_model(core.clone(),session.clone(),response_id,cancel_rx,sink);
            let store=core.store.lock().map_err(|e|e.to_string())?;detail(&store,&session)
        }
        "agent_send_message" => {
            let session = text(&args, "sessionId")?;
            let content = text(&args, "content")?;
            if content.len() > 32000 {
                return Err("消息过长".into());
            }
            let response_id=id();
            let (cancel_tx,cancel_rx)=tokio::sync::oneshot::channel();
            let use_model={
                let store=core.store.lock().map_err(|e|e.to_string())?;require_session(&store,&session)?;
                let tx=store.conn.unchecked_transaction().map_err(|e|e.to_string())?;
                let busy:bool=store.conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_model_pending WHERE session_id=?1)",[&session],|r|r.get(0)).map_err(|e|e.to_string())?;
                if busy{return Err("当前会话正在等待模型回复".into());}
                let selection=crate::model::session_config(&store,&session)?;
                let enabled=selection["enabled"]==true && selection["hasApiKey"]==true;
                if !enabled && !selection["activeServerId"].as_str().unwrap_or("").is_empty(){return Err("该会话选择的模型已停用或密钥不可用，请重新选择模型。".into());}
                if enabled{store.conn.execute("INSERT INTO agent_model_pending(session_id,message_id) VALUES (?1,?2)",params![session,response_id]).map_err(|e|e.to_string())?;}
                message(&store,&session,"user",&content)?;
                store.conn.execute("UPDATE agent_sessions SET title=?2 WHERE id=?1 AND title_custom=0 AND (SELECT count(*) FROM agent_messages WHERE session_id=?1 AND role='user')=1",params![session,content.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(128).collect::<String>()]).map_err(|e|e.to_string())?;
                if enabled{store.conn.execute("INSERT INTO agent_messages(id,session_id,role,content,created_at,model_metadata) VALUES (?1,?2,'assistant','',?3,?4)",params![response_id,session,now(),json!({"source":"llm","generationStatus":"streaming","serverId":selection["activeServerId"],"serverName":selection["name"],"requestedModel":selection["model"],"model":selection["model"]}).to_string()]).map_err(|e|e.to_string())?;}
                if !enabled{message(&store,&session,"assistant","消息已保存。尚未启用模型，可到账户页配置 DeepSeek，或通过工具入口手动操作。")?;}
                if enabled{core.model_cancellations.lock().map_err(|e|e.to_string())?.insert(response_id.clone(),cancel_tx);}
                tx.commit().map_err(|e|e.to_string())?;enabled
            };
            if use_model{launch_model(core.clone(),session.clone(),response_id,cancel_rx,sink);}
            let store=core.store.lock().map_err(|e|e.to_string())?;detail(&store,&session)
        }

        "agent_run_tool" => {
            let run = {
                let store = core.store.lock().map_err(|e| e.to_string())?;
                create_run(
                    &store,
                    &text(&args, "sessionId")?,
                    &text(&args, "tool")?,
                    args.get("arguments").cloned().unwrap_or(json!({})),
                )?
            };
            if run["status"] == "running" {
                launch(core, run["id"].as_str().unwrap().into());
            }
            Ok(run)
        }
        "agent_review_tool" => {
            let session=text(&args,"sessionId")?;let run=text(&args,"runId")?;let hash=text(&args,"reviewHash")?;let approved=args["approved"].as_bool().ok_or("缺少确认决定")?;
            let store=core.store.lock().map_err(|e|e.to_string())?;require_session(&store,&session)?;crate::agent_runtime::decide(&store,&session,&run,&hash,approved)?;detail(&store,&session)
        }
        "agent_decide_tool" => {
            let run = text(&args, "runId")?;
            let approved = args
                .get("approved")
                .and_then(Value::as_bool)
                .ok_or("缺少确认决定")?;
            {
                let store = core.store.lock().map_err(|e| e.to_string())?;
                let session:String=store.conn.query_row("SELECT session_id FROM agent_runs WHERE id=?1",[&run],|r|r.get(0)).map_err(|_|"操作不存在")?;require_session(&store,&session)?;
                decide(&store, &run, approved)?;
            }
            if approved {
                launch(core, run);
            }
            Ok(json!({"ok":true}))
        }
        _ => Err("未知智能体操作".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("keeper-agent-{}", id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn migrates_legacy_sessions_into_general_project() {
        let temp=Temp::new();
        let store=Store::load(temp.0.join("data.sql")).unwrap();
        store.conn.execute_batch("CREATE TABLE agent_sessions (id TEXT PRIMARY KEY,title TEXT NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL); INSERT INTO agent_sessions VALUES ('legacy','旧会话',1,2);").unwrap();
        store.conn.execute_batch("CREATE TABLE agent_messages (id TEXT PRIMARY KEY,session_id TEXT NOT NULL,role TEXT NOT NULL,content TEXT NOT NULL,created_at INTEGER NOT NULL); INSERT INTO agent_messages VALUES ('legacy-reply','legacy','assistant','旧回复',1);").unwrap();
        store.set_setting("agent_deepseek_config",&json!({"enabled":true,"baseUrl":"https://api.deepseek.com","model":"deepseek-flash"}).to_string()).unwrap();
        store.set_setting("agent_deepseek_api_key","migration-only-test-key").unwrap();
        initialize(&store).unwrap();initialize(&store).unwrap();
        assert_eq!(detail(&store,"legacy").unwrap()["model"]["activeServerId"],"legacy-deepseek");
        let old=detail(&store,"legacy").unwrap();assert_eq!(old["messages"][0]["id"],"legacy-reply");assert_eq!(old["messages"][0]["modelMetadata"],json!({}));
        let rows=sessions(&store).unwrap();
        assert_eq!(rows[0]["id"],"legacy");assert_eq!(rows[0]["projectId"],"general");
        let projects=projects(&store).unwrap();assert_eq!(projects.as_array().unwrap().len(),1);
        assert_eq!(projects[0]["name"],"通用项目");
        assert!(std::path::Path::new(projects[0]["directory"].as_str().unwrap()).is_dir());
    }
    fn session(store: &Store) -> String {
        let session = id();
        store
            .conn
            .execute(
                "INSERT INTO agent_sessions (id,title,created_at,updated_at) VALUES (?1,'新会话',1,1)",
                [&session],
            )
            .unwrap();
        session
    }
    #[test]
    fn reply_versions_keep_original_order_and_only_selected_context_after_restart() {
        let temp=Temp::new();let path=temp.0.join("data.sql");
        let store=Store::load(path.clone()).unwrap();initialize(&store).unwrap();let session=session(&store);
        let add=|role:&str,content:&str|{message(&store,&session,role,content).unwrap();store.conn.query_row("SELECT id FROM agent_messages WHERE session_id=?1 ORDER BY rowid DESC LIMIT 1",[&session],|r|r.get::<_,String>(0)).unwrap()};
        let user=add("user","question");
        let first=add("assistant","first");
        message(&store,&session,"user","follow up").unwrap();
        let second=add("assistant","second");
        let third=add("assistant","third");
        for (id,parent) in [(&second,&first),(&third,&second)] {
            store.conn.execute("UPDATE agent_messages SET model_metadata=?2 WHERE id=?1",params![id,json!({"regenerateOf":parent}).to_string()]).unwrap();
        }
        let boundary=add("assistant","");
        let groups=detail(&store,&session).unwrap()["replyGroups"].clone();
        let group=groups.as_array().unwrap().iter().find(|g|g["rootId"]==first).unwrap();
        assert_eq!(group["versionIds"],json!([first,second,third]));assert_eq!(group["selectedId"],third);
        assert_eq!(context_messages(&store,&session,&boundary).unwrap(),json!([{"role":"user","content":"question"},{"role":"assistant","content":"third"},{"role":"user","content":"follow up"}]).as_array().unwrap().clone());
        store.conn.execute("INSERT INTO agent_reply_versions VALUES (?1,?2,?3)",params![session,first,second]).unwrap();
        drop(store);let store=Store::load(path).unwrap();initialize(&store).unwrap();
        assert_eq!(detail(&store,&session).unwrap()["replyGroups"].as_array().unwrap().iter().find(|g|g["rootId"]==first).unwrap()["selectedId"],second);
        assert_eq!(context_messages(&store,&session,&boundary).unwrap()[1]["content"],"second");
        assert_eq!(context_messages(&store,&session,&first).unwrap().len(),1);
        assert_eq!(reply_messages(&store,&session,None).unwrap()[0]["id"],user);
    }
    fn host(store: &Store) -> crate::host::Host {
        let host:crate::host::Host=serde_json::from_value(json!({"id":"local-test","name":"test","hostname":"192.0.2.10","port":22,"username":"root","auth":{"method":"password","password":"never-expose-this-password"}})).unwrap();
        store.add_host(&host).unwrap();
        host
    }
    #[test]
    fn approval_is_required_and_cannot_be_replayed_or_replaced() {
        let temp = Temp::new();
        let store = Store::load(temp.0.join("data.sql")).unwrap();
        initialize(&store).unwrap();
        host(&store);
        let session = session(&store);
        let run = create_run(
            &store,
            &session,
            "ssh_execute",
            json!({"hostId":"local-test","command":"uname -a","target":{"hostname":"attacker"}}),
        )
        .unwrap();
        assert_eq!(run["status"], "pending_approval");
        let detail = detail(&store, &session).unwrap();
        assert_eq!(
            detail["runs"][0]["arguments"]["target"]["hostname"],
            "192.0.2.10"
        );
        assert!(!detail.to_string().contains("never-expose-this-password"));
        let run = run["id"].as_str().unwrap();
        decide(&store, run, false).unwrap();
        assert!(decide(&store, run, true).is_err());
        assert_eq!(detail_of_status(&store, &session), "rejected");
        assert!(create_run(&store, &session, "unknown", json!({})).is_err());
        assert!(create_run(&store, "missing", "list_hosts", json!({})).is_err());
    }
    fn detail_of_status(store: &Store, session: &str) -> Value {
        detail(store, session).unwrap()["runs"][0]["status"].clone()
    }
    #[test]
    fn restart_preserves_messages_and_approvals_but_interrupts_running_tools() {
        let temp = Temp::new();
        let path = temp.0.join("data.sql");
        let session_id;
        {
            let store = Store::load(path.clone()).unwrap();
            initialize(&store).unwrap();
            host(&store);
            session_id = session(&store);
            message(&store, &session_id, "user", "保存会话").unwrap();
            create_run(&store, &session_id, "list_hosts", json!({})).unwrap();
            create_run(
                &store,
                &session_id,
                "ssh_execute",
                json!({"hostId":"local-test","command":"uptime"}),
            )
            .unwrap();
        }
        let store = Store::load(path).unwrap();
        initialize(&store).unwrap();
        let data = detail(&store, &session_id).unwrap();
        assert_eq!(data["messages"][0]["content"], "保存会话");
        assert_eq!(data["runs"][0]["status"], "interrupted");
        assert_eq!(data["runs"][1]["status"], "pending_approval");
    }
    #[tokio::test]
    async fn tools_redact_credentials_and_pin_approval_to_target() {
        let temp = Temp::new();
        let core = Arc::new(crate::create_app_state(temp.0.clone()).unwrap());
        {
            let store = core.store.lock().unwrap();
            host(&store);
        }
        let hosts = execute(&core, "list_hosts", &json!({})).await.unwrap();
        assert_eq!(hosts[0]["hostname"], "192.0.2.10");
        assert!(!hosts.to_string().contains("password"));
        let args = {
            let store = core.store.lock().unwrap();
            let session = session(&store);
            create_run(
                &store,
                &session,
                "ssh_execute",
                json!({"hostId":"local-test","command":"uptime"}),
            )
            .unwrap();
            detail(&store, &session).unwrap()["runs"][0]["arguments"].clone()
        };
        {
            let store = core.store.lock().unwrap();
            let mut h = store.get_host("local-test").unwrap();
            h.hostname = "192.0.2.20".into();
            store.update_host(&h).unwrap();
        }
        assert!(
            execute(&core, "ssh_execute", &args)
                .await
                .unwrap_err()
                .contains("配置已改变")
        );
    }
}
