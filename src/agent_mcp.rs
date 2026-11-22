//! Local MCP stdio connections and persisted, user-configured server metadata.
use crate::store::Store;
#[path = "agent_mcp_settings.rs"]
mod settings;
use rusqlite::params;
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

pub(crate) fn initialize(store: &Store) -> Result<(), String> {
    store.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_mcp_servers (id TEXT PRIMARY KEY, owner_key TEXT NOT NULL, name TEXT NOT NULL, command TEXT NOT NULL, args TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 0, tools TEXT NOT NULL DEFAULT '[]');").map_err(|e|e.to_string())?;
    let columns = store
        .conn
        .prepare("PRAGMA table_info(agent_mcp_servers)")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if !columns.iter().any(|c| c == "transport") {
        store
            .conn
            .execute_batch(
                "ALTER TABLE agent_mcp_servers ADD COLUMN transport TEXT NOT NULL DEFAULT 'stdio'",
            )
            .map_err(|e| e.to_string())?;
    }
    let tx=store.conn.unchecked_transaction().map_err(|e|e.to_string())?;
    for (name,statement) in [
        ("plugin_manifest","ALTER TABLE agent_mcp_servers ADD COLUMN plugin_manifest TEXT NOT NULL DEFAULT '{}';"),
        ("environment","ALTER TABLE agent_mcp_servers ADD COLUMN environment TEXT NOT NULL DEFAULT '{}';"),
        ("environment_revision","ALTER TABLE agent_mcp_servers ADD COLUMN environment_revision TEXT NOT NULL DEFAULT '';"),
        ("project_ids","ALTER TABLE agent_mcp_servers ADD COLUMN project_ids TEXT NOT NULL DEFAULT 'null';"),
    ] {
        if !columns.iter().any(|column|column==name){store.conn.execute_batch(statement).map_err(|e|e.to_string())?;}
    }
    tx.commit().map_err(|e|e.to_string())?;
    Ok(())
}
fn environment(store:&Store,id:&str,owner:&str)->Result<Value,String>{
    let raw:String=store.conn.query_row("SELECT environment FROM agent_mcp_servers WHERE id=?1 AND owner_key=?2",params![id,owner],|r|r.get(0)).map_err(|_|"MCP 环境配置不存在或无法读取")?;
    let value:Value=serde_json::from_str(&raw).map_err(|_|"MCP 环境配置无效")?;
    settings::environment(&value,None)
}
pub(crate) fn list(store: &Store) -> Result<Value, String> {
    let owner = crate::agent_account::owner(store)?;
    let mut stmt=store.conn.prepare("SELECT id,name,command,args,enabled,tools,transport,environment,environment_revision,project_ids,plugin_manifest FROM agent_mcp_servers WHERE owner_key=?1 ORDER BY rowid").map_err(|e|e.to_string())?;
    let rows=stmt.query_map([owner],|r|Ok((json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"command":r.get::<_,String>(2)?,"args":serde_json::from_str::<Value>(&r.get::<_,String>(3)?).unwrap_or(json!([])),"enabled":r.get::<_,bool>(4)?,"tools":serde_json::from_str::<Value>(&r.get::<_,String>(5)?).unwrap_or(json!([])),"transport":r.get::<_,String>(6)?,"plugin":serde_json::from_str::<Value>(&r.get::<_,String>(10)?).unwrap_or(json!({})),"environmentRevision":r.get::<_,String>(8)?}),r.get::<_,String>(7)?,r.get::<_,String>(9)?))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    let mut result=Vec::new();
    for (mut item,raw_environment,raw_projects) in rows {
        let values:Value=serde_json::from_str(&raw_environment).map_err(|_|"MCP 环境配置无效")?;
        let validated=settings::environment(&values,None)?;
        item["environmentKeys"]=json!(validated.as_object().ok_or("MCP 环境配置无效")?.keys().cloned().collect::<Vec<_>>());
        item["projectIds"]=serde_json::from_str::<Value>(&raw_projects).map_err(|_|"MCP 项目范围配置无效")?;
        result.push(item);
    }
    Ok(json!(result))
}
/// Build the private process configuration only at the launch boundary.
/// Values must never enter public lists, approvals, model definitions or cache fingerprints.
pub(crate) fn launch_config(store:&Store,server:&Value)->Result<Value,String>{
    let owner=crate::agent_account::owner(store)?;
    let public=list(store)?.as_array().unwrap().iter().find(|s|s["id"]==server["id"] && *s==server).cloned().ok_or("MCP 服务配置已变更，请重试")?;
    let mut launch=public;
    if launch["transport"]=="stdio" {
        launch["_keeperEnvironment"]=environment(store,launch["id"].as_str().ok_or("MCP 服务无效")?,&owner)?;
    }
    Ok(launch)
}

pub(crate) fn save(store: &Store, args: &Value) -> Result<Value, String> {
    let owner = crate::agent_account::owner(store)?;
    let name = args["name"].as_str().unwrap_or("").trim();
    let command = args["command"].as_str().unwrap_or("").trim();
    let transport = args["transport"].as_str().unwrap_or("stdio");
    if name.is_empty()
        || name.len() > 80
        || command.is_empty()
        || command.len() > 2048
        || command.contains('\0')
        || !matches!(transport, "stdio" | "sse" | "http")
    {
        return Err("请填写有效的服务名称、连接类型和地址".into());
    }
    let argv = args["args"].as_array().ok_or("参数必须是字符串数组")?;
    if argv.len() > 64
        || argv.iter().any(|a| {
            a.as_str()
                .is_none_or(|s| s.len() > 4096 || s.contains('\0'))
        })
    {
        return Err("MCP 参数无效或过长".into());
    }
    if transport != "stdio" {
        crate::agent_mcp_http::validate_url(command)?;
        crate::agent_mcp_http::headers(argv)?;
    }
    let id = args["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    let existing = list(store)?
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == id)
        .cloned();
    if args["id"].as_str().is_some_and(|s| !s.is_empty()) && existing.is_none() {
        return Err("MCP 服务不存在或不属于当前账号".into());
    }
    let previous_environment=if existing.is_some(){environment(store,&id,&owner)?}else{json!({})};
    if transport!="stdio" && args.get("environmentChanges").is_some_and(|value|!value.as_object().is_some_and(|object|object.is_empty())) {
        return Err("只有本机 stdio MCP 服务可以设置环境变量".into());
    }
    let next_environment=if transport=="stdio" {settings::environment(&previous_environment,args.get("environmentChanges"))?}else{json!({})};
    let environment_changed=previous_environment!=next_environment;
    let environment_revision=if environment_changed{uuid::Uuid::new_v4().to_string()}else{existing.as_ref().and_then(|s|s["environmentRevision"].as_str()).unwrap_or("").to_owned()};
    let previous_projects=existing.as_ref().map(|s|s["projectIds"].clone()).unwrap_or(Value::Null);
    let projects=settings::project_ids(store,args.get("projectIds"),&previous_projects)?;
    let unchanged = existing.as_ref().is_some_and(|v| {
        v["command"] == command && v["args"] == args["args"] && v["transport"] == transport && !environment_changed
    });
    let tools = if unchanged {existing.as_ref().unwrap()["tools"].to_string()} else {"[]".to_string()};
    let requested_enabled=match args.get("enabled") {None=>existing.as_ref().is_some_and(|s|s["enabled"]==true),Some(v)=>v.as_bool().ok_or("MCP 启用状态必须是布尔值")?};
    let enabled = unchanged && requested_enabled;
    store.conn.execute("INSERT INTO agent_mcp_servers(id,owner_key,name,command,args,enabled,tools,transport,environment,environment_revision,project_ids) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(id) DO UPDATE SET name=excluded.name,command=excluded.command,args=excluded.args,enabled=excluded.enabled,tools=excluded.tools,transport=excluded.transport,environment=excluded.environment,environment_revision=excluded.environment_revision,project_ids=excluded.project_ids WHERE owner_key=excluded.owner_key",params![id,owner,name,command,json!(argv).to_string(),enabled,tools,transport,next_environment.to_string(),environment_revision,projects.to_string()]).map_err(|e|e.to_string())?;
    Ok(json!({"id":id,"servers":list(store)?}))
}
pub(crate) fn delete(store: &Store, id: &str) -> Result<Value, String> {
    let owner = crate::agent_account::owner(store)?;
    if store
        .conn
        .execute(
            "DELETE FROM agent_mcp_servers WHERE id=?1 AND owner_key=?2",
            params![id, owner],
        )
        .map_err(|e| e.to_string())?
        != 1
    {
        return Err("MCP 服务不存在".into());
    }
    list(store)
}

struct StdioClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    capabilities: Value,
}
impl StdioClient {
    async fn start(server: &Value) -> Result<Self, String> {
        let command = server["command"].as_str().ok_or("缺少 MCP 命令")?;
        let args = server["args"]
            .as_array()
            .ok_or("MCP 参数无效")?
            .iter()
            .map(|x| x.as_str().unwrap_or(""))
            .collect::<Vec<_>>();
        let mut process = Command::new(command);
        process.args(args).env_clear();
        for key in [
            "PATH",
            "HOME",
            "USER",
            "USERPROFILE",
            "SYSTEMROOT",
            "WINDIR",
            "TEMP",
            "TMP",
            "TMPDIR",
        ] {
            if let Some(value) = std::env::var_os(key) {
                process.env(key, value);
            }
        }
        if let Some(values)=server.get("_keeperEnvironment") {
            let environment=settings::environment(values,None)?;
            for (name,value) in environment.as_object().ok_or("MCP 环境配置无效")? {
                process.env(name,value.as_str().ok_or("MCP 环境配置无效")?);
            }
        }
        #[cfg(unix)]
        process.process_group(0);
        let mut child = process
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "无法启动 MCP Server，请检查命令")?;
        let stdin = child.stdin.take().ok_or("MCP stdin 不可用")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("MCP stdout 不可用")?);
        let mut c = Self {
            child,
            stdin,
            stdout,
            capabilities: Value::Null,
        };
        let reply=c.request(1,"initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"cangling-keeper","version":env!("CARGO_PKG_VERSION")}})).await?;
        c.capabilities = supported_capabilities(&reply)?;
        c.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await?;
        Ok(c)
    }
    async fn send(&mut self, v: Value) -> Result<(), String> {
        let mut line = serde_json::to_vec(&v).map_err(|_| "MCP 请求序列化失败")?;
        line.push(b'\n');
        self.stdin
            .write_all(&line)
            .await
            .map_err(|_| "MCP Server 已关闭".to_owned())
    }
    async fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, String> {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        for _ in 0..64 {
            let mut line = Vec::new();
            let n = (&mut self.stdout)
                .take(6291457)
                .read_until(b'\n', &mut line)
                .await
                .map_err(|_| "读取 MCP 响应失败")?;
            if n == 0 || line.len() > 6291456 || !line.ends_with(b"\n") {
                return Err("MCP 响应过大或连接已关闭".into());
            }
            let v: Value = serde_json::from_slice(&line).map_err(|_| "MCP 响应不是有效 JSON")?;
            if v["id"] != id {
                continue;
            }
            if v.get("error").is_some() {
                return Err("MCP Server 返回错误".into());
            }
            return v.get("result").cloned().ok_or("MCP 响应缺少结果".into());
        }
        Err("MCP 响应消息过多".into())
    }
}
impl Drop for StdioClient {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.child.id() {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        let _ = self.child.start_kill();
    }
}
pub(crate) fn supported_capabilities(initialized: &Value) -> Result<Value, String> {
    let capabilities=initialized.get("capabilities").filter(|v|v.is_object()).ok_or("MCP 初始化缺少有效能力声明")?;
    if !["tools","resources","prompts"].iter().any(|kind|capabilities[*kind].is_object()) {
        return Err("MCP Server 未声明工具、资料或提示模板能力".into());
    }
    Ok(capabilities.clone())
}
enum Client {
    Stdio(StdioClient),
    Http(crate::agent_mcp_http::Client),
}
impl Client {
    async fn start(server: &Value) -> Result<Self, String> {
        if server["transport"] == "stdio" || server["transport"].is_null() {
            Ok(Self::Stdio(StdioClient::start(server).await?))
        } else {
            Ok(Self::Http(
                crate::agent_mcp_http::Client::start(server).await?,
            ))
        }
    }
    fn capabilities(&self) -> &Value {
        match self { Self::Stdio(c)=>&c.capabilities, Self::Http(c)=>c.capabilities() }
    }
    async fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, String> {
        match self {
            Self::Stdio(c) => c.request(id, method, params).await,
            Self::Http(c) => c.request(id, method, params).await,
        }
    }
}
pub(crate) async fn discover(server: &Value) -> Result<Vec<Value>, String> {
    tokio::time::timeout(Duration::from_secs(15),async {
        let mut client=Client::start(server).await?;
        let mut tools=Vec::new();
        if client.capabilities()["tools"].is_object() {
            let mut cursor=None::<String>;
            let mut previous=std::collections::HashSet::new();
            for id in 2..12 {
                let result=client.request(id,"tools/list",cursor.as_ref().map_or(json!({}),|s|json!({"cursor":s}))).await?;
                for tool in result["tools"].as_array().ok_or("MCP 工具列表无效")? {
                    let name=tool["name"].as_str().ok_or("MCP 工具名称无效")?;
                    if name.is_empty() || name.len()>128 || name.starts_with("__keeper_mcp_") || tool["inputSchema"]["type"]!="object" || tool.to_string().len()>32768 {return Err("MCP 工具声明无效、使用保留名称或过大".into());}
                    if tools.iter().any(|t:&Value|t["name"]==name){return Err("MCP 工具名称重复".into());}
                    // External tools cannot supply Keeper's native operation kind or flags.
                    tools.push(json!({"name":name,"description":tool["description"].as_str().unwrap_or(""),"inputSchema":tool["inputSchema"]}));
                    if tools.len()>64{return Err("MCP 工具数量超过限制".into());}
                }
                cursor=match result.get("nextCursor") {None | Some(Value::Null)=>None,Some(v)=>Some(v.as_str().filter(|c|!c.is_empty() && c.len()<=2048).ok_or("MCP 工具分页游标无效")?.to_owned())};
                if let Some(cursor)=&cursor {if !previous.insert(cursor.clone()){return Err("MCP 工具分页游标重复".into());}}
                if cursor.is_none(){break;}
                if id==11{return Err("MCP 工具分页超过限制".into());}
            }
        }
        // Native catalogs and their bodies remain lazy and go through tool approvals.
        tools.extend(crate::agent_mcp_content::wrappers(client.capabilities()));
        Ok(tools)
    }).await.map_err(|_|"MCP Server 连接超时".to_owned())?
}

fn session_project(store:&Store,session:&str)->Result<String,String>{
    crate::agent_history::require_active(store,session)?;
    store.conn.query_row("SELECT project_id FROM agent_sessions WHERE id=?1",[session],|r|r.get(0)).map_err(|_|"MCP 会话项目不存在".to_owned())
}
fn require_scope(store:&Store,session:&str,server:&Value)->Result<String,String>{
    let project=session_project(store,session)?;
    if !settings::applies(&server["projectIds"],&project){return Err("当前项目未启用此 MCP 服务".into());}
    Ok(project)
}
fn definitions_from(servers:Value,project:Option<&str>)->Vec<Value>{
    let mut result = Vec::new();
    for server in servers.as_array().unwrap().iter().filter(|s|s["enabled"]==true && project.is_none_or(|project|settings::applies(&s["projectIds"],project))) {
        for (index, tool) in server["tools"].as_array().unwrap().iter().enumerate() {
            if !selected(tool,index){continue;}
            let title=tool["title"].as_str().unwrap_or_else(||tool["name"].as_str().unwrap_or(""));
            result.push(json!({"type":"function","function":{"name":tool_id(server,tool,index),"description":format!("MCP 服务 {} / {}: {}",server["name"].as_str().unwrap_or(""),title,tool["description"].as_str().unwrap_or("")),"parameters":tool["inputSchema"]}}));
        }
    }
    result
}
// Kept for internal compatibility; model requests must use definitions_for.
pub(crate) fn definitions(store:&Store)->Result<Vec<Value>,String>{Ok(definitions_from(list(store)?,None))}
pub(crate) fn definitions_for(store:&Store,session:&str)->Result<Vec<Value>,String>{
    let project=session_project(store,session)?;
    Ok(definitions_from(list(store)?,Some(&project)))
}

fn selected(tool: &Value, index: usize) -> bool {
    tool["enabled"].as_bool().unwrap_or(tool.get("kind").is_none() && index < 8)
}
pub(crate) fn selection(store: &Store, id: &str, names: &Value) -> Result<Value, String> {
    let owner = crate::agent_account::owner(store)?;
    let names = names.as_array().ok_or("请选择 MCP 工具")?;
    if names.len() > 64 || names.iter().any(|v| v.as_str().is_none()) {
        return Err("MCP 工具选择无效".into());
    }
    let server = list(store)?
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .cloned()
        .ok_or("MCP 服务不存在")?;
    let original = server["tools"].to_string();
    let mut tools = server["tools"]
        .as_array()
        .cloned()
        .ok_or("MCP 工具列表无效")?;
    for name in names {
        if !tools.iter().any(|t| t["name"] == *name) {
            return Err("所选 MCP 工具已变更，请刷新".into());
        }
    }
    for tool in &mut tools {
        tool["enabled"] = json!(names.contains(&tool["name"]));
    }
    let changed = store
        .conn
        .execute(
            "UPDATE agent_mcp_servers SET tools=?1 WHERE id=?2 AND owner_key=?3 AND tools=?4",
            params![json!(tools).to_string(), id, owner, original],
        )
        .map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("MCP 工具列表已变更，请刷新".into());
    }
    list(store)
}
pub(crate) fn merge_selection(server: &Value, mut tools: Vec<Value>) -> Vec<Value> {
    let previous = server["tools"].as_array();
    let has_selection = previous.is_some_and(|p| p.iter().any(|t| t["enabled"].is_boolean()));
    let preferred: Vec<usize> = tools
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            t["name"].as_str().is_some_and(|n| {
                ["get_", "list_", "read_", "search_"]
                    .iter()
                    .any(|prefix| n.starts_with(prefix))
            })
        })
        .map(|(i, _)| i)
        .take(8)
        .collect();
    for (i, tool) in tools.iter_mut().enumerate() {
        let enabled = if tool["kind"].is_string() {
            previous.and_then(|items|items.iter().find(|p|p["name"]==tool["name"] && p["kind"]==tool["kind"]))
                .and_then(|p|p["enabled"].as_bool()).unwrap_or(false)
        } else if has_selection {
            previous
                .unwrap()
                .iter()
                .find(|p| p["name"] == tool["name"] && p.get("kind").is_none())
                .and_then(|p| p["enabled"].as_bool())
                .unwrap_or(false)
        } else if preferred.is_empty() {
            i < 8
        } else {
            preferred.contains(&i)
        };
        tool["enabled"] = json!(enabled);
    }
    tools
}
fn tool_id(server: &Value, tool: &Value, index: usize) -> String {
    let hash =
        crate::agent_actions::hash(json!({"server":server,"tool":tool}).to_string().as_bytes());
    format!(
        "mcp_{}_{}_{}",
        server["id"].as_str().unwrap(),
        index,
        &hash[..12]
    )
}
pub(crate) fn resolve(store: &Store, name: &str) -> Result<(Value, Value), String> {
    let rest = name.strip_prefix("mcp_").ok_or("不是 MCP 工具")?;
    let (id_index, _hash) = rest.rsplit_once('_').ok_or("MCP 工具名称无效")?;
    let (id, index) = id_index.rsplit_once('_').ok_or("MCP 工具名称无效")?;
    let index = index.parse::<usize>().map_err(|_| "MCP 工具名称无效")?;
    let server = list(store)?
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id && s["enabled"] == true)
        .cloned()
        .ok_or("MCP 服务不可用")?;
    let tool = server["tools"]
        .as_array()
        .and_then(|a| a.get(index))
        .cloned()
        .ok_or("MCP 工具已变更")?;
    if !selected(&tool, index) || tool_id(&server, &tool, index) != name {
        return Err("MCP 工具配置已变更，请重试".into());
    }
    Ok((server, tool))
}
pub(crate) fn resolve_for(store:&Store,session:&str,name:&str)->Result<(Value,Value),String>{
    let (server,tool)=resolve(store,name)?;
    require_scope(store,session,&server)?;
    Ok((server,tool))
}
// Only idle connections live in the pool. A checked-out connection is owned by a
// lease, so dropping an interrupted request always drops its transport too.
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_CONNECTIONS: usize = 32;
#[derive(Clone, Hash, Eq, PartialEq)]
struct ConnectionKey {
    owner: String,
    session: String,
    server: String,
    fingerprint: String,
}
struct Connection {
    client: Client,
    next_id: u64,
}
struct IdleConnection {
    connection: Connection,
    last_used: std::time::Instant,
}
#[derive(Default)]
struct PoolState {
    generation: u64,
    idle: std::collections::HashMap<ConnectionKey, IdleConnection>,
    active: std::collections::HashMap<ConnectionKey, tokio::sync::oneshot::Sender<()>>,
}
#[derive(Default)]
pub(crate) struct ConnectionPool {
    state: std::sync::Mutex<PoolState>,
    sweeping: std::sync::atomic::AtomicBool,
}
impl PoolState {
    fn prune(&mut self, now: std::time::Instant) {
        self.idle
            .retain(|_, value| now.saturating_duration_since(value.last_used) < IDLE_TIMEOUT);
    }
}
impl ConnectionPool {
    pub(crate) fn clear(&self) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.generation = state.generation.wrapping_add(1);
        state.idle.clear();
        for (_, cancel) in state.active.drain() {
            let _ = cancel.send(());
        }
        Ok(())
    }
    pub(crate) fn annotate(&self, owner: &str, mut servers: Value) -> Result<Value, String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.prune(std::time::Instant::now());
        for server in servers.as_array_mut().ok_or("MCP 列表无效")? {
            let count = state
                .idle
                .keys()
                .chain(state.active.keys())
                .filter(|k| k.owner == owner && json!(k.server) == server["id"])
                .count();
            server["connectedSessions"] = json!(count);
        }
        Ok(servers)
    }
    fn checkout(self: &std::sync::Arc<Self>, key: ConnectionKey) -> Result<Lease, String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.prune(std::time::Instant::now());
        if state.active.contains_key(&key) {
            return Err("同一会话的 MCP 工具正在执行，请等待完成".into());
        }
        let connection = state.idle.remove(&key).map(|v| v.connection);
        if connection.is_none() && state.idle.len() + state.active.len() >= MAX_CONNECTIONS {
            if let Some(oldest) = state
                .idle
                .iter()
                .min_by_key(|(_, v)| v.last_used)
                .map(|(k, _)| k.clone())
            {
                state.idle.remove(&oldest);
            } else {
                return Err("MCP 连接繁忙，请稍后重试".into());
            }
        }
        let (cancel, invalidated) = tokio::sync::oneshot::channel();
        state.active.insert(key.clone(), cancel);
        let lease = Lease {
            pool: self.clone(),
            key,
            generation: state.generation,
            connection,
            invalidated,
        };
        drop(state);
        if !self
            .sweeping
            .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            let weak = std::sync::Arc::downgrade(self);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    let Some(pool) = weak.upgrade() else { break };
                    if let Ok(mut state) = pool.state.lock() {
                        state.prune(std::time::Instant::now());
                    }
                }
            });
        }
        Ok(lease)
    }
}
struct Lease {
    pool: std::sync::Arc<ConnectionPool>,
    key: ConnectionKey,
    generation: u64,
    connection: Option<Connection>,
    invalidated: tokio::sync::oneshot::Receiver<()>,
}
impl Lease {
    fn retain(mut self) -> Result<(), String> {
        let mut state = self.pool.state.lock().map_err(|e| e.to_string())?;
        if state.generation != self.generation {
            return Err("MCP 连接配置已变更，连接已关闭".into());
        }
        state.active.remove(&self.key);
        if let Some(connection) = self.connection.take() {
            state.idle.insert(
                self.key.clone(),
                IdleConnection {
                    connection,
                    last_used: std::time::Instant::now(),
                },
            );
        }
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.pool.state.lock() {
            if state.generation == self.generation {
                state.active.remove(&self.key);
            }
        }
    }
}
pub(crate) async fn call(
    core: &crate::AppState,
    session: &str,
    server: &Value,
    tool: &Value,
    args: &Value,
) -> Result<Value, String> {
    let (owner,launch) = {
        let store = core.store.lock().map_err(|e| e.to_string())?;
        require_scope(&store,session,server)?;
        if server["enabled"]!=true || !server["tools"].as_array().is_some_and(|items|items.iter().enumerate().any(|(index,item)|item==tool && selected(item,index))){return Err("MCP 工具未启用或配置已变更".into());}
        (crate::agent_account::owner(&store)?,launch_config(&store,server)?)
    };
    let native=tool["kind"].as_str();
    let (method,parameters)=if native.is_some(){crate::agent_mcp_content::params(tool,args)?}else{("tools/call".to_owned(),json!({"name":tool["name"],"arguments":args}))};
    let key = ConnectionKey {
        owner: owner.clone(),
        session: session.to_owned(),
        server: server["id"].as_str().ok_or("MCP 服务无效")?.to_owned(),
        fingerprint: crate::agent_actions::hash(server.to_string().as_bytes()),
    };
    let mut lease = core.mcp_pool.checkout(key)?;
    let result=tokio::time::timeout(Duration::from_secs(60),async {
        if lease.connection.is_none(){
            let client=tokio::select!{biased;_=&mut lease.invalidated=>return Err("MCP 连接已断开或配置已变更".to_string()),result=Client::start(&launch)=>result?};
            lease.connection=Some(Connection{client,next_id:2});
        }
        let connection=lease.connection.as_mut().unwrap();
        let capability=method.split('/').next().unwrap_or("tools");
        if !connection.client.capabilities()[capability].is_object(){return Err("MCP 服务能力已变更，请重新发现服务能力".to_owned());}
        let id=connection.next_id;connection.next_id+=1;
        let response=tokio::select!{biased;_=&mut lease.invalidated=>return Err("MCP 连接已断开或配置已变更；外部操作结果需核实，不要自动重试".to_string()),result=connection.client.request(id,&method,parameters)=>result.map_err(|e|format!("{e}；本次调用未自动重放，外部操作结果需核实"))?};
        let images=crate::agent_mcp_images::extract(&method,&response)?;
        if native.is_some(){let mut value=crate::agent_mcp_content::normalize(&method,response)?;if !images.is_empty(){crate::agent_mcp_images::mark_placeholders(&mut value);}value["keeperImageData"]=json!(images);return Ok(Ok(value));}
        let text=response["content"].as_array().ok_or("MCP 工具结果无效")?.iter().filter_map(|x|if x["type"]=="text"{x["text"].as_str()}else{None}).collect::<Vec<_>>().join("\n");
        if text.len()>40000{return Err("MCP 工具结果过大".to_owned());}
        Ok(if response["isError"]==true{Err(format!("MCP 工具错误：{text}"))}else{Ok(json!({"output":text,"keeperImageData":images}))})
    }).await.map_err(|_|"MCP 工具执行超时；外部操作结果需核实，不要自动重试".to_owned())??;
    // Recheck identity and exact configuration before retaining a live connection.
    let valid = {
        let store = core.store.lock().map_err(|e| e.to_string())?;
        crate::agent_account::owner(&store)? == owner
            && require_scope(&store,session,server).is_ok()
            && list(&store)?
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s == server)
    };
    if !valid{return Err("账号或 MCP 配置已变化，已丢弃结果".into());}
    if valid {
        let _ = lease.retain();
    }
    result
}

#[cfg(test)]
mod pool_tests {
    use super::*;
    fn key(session: usize) -> ConnectionKey {
        ConnectionKey {
            owner: "fixture-owner".into(),
            session: session.to_string(),
            server: "fixture-server".into(),
            fingerprint: "fixture-config".into(),
        }
    }
    #[test]
    fn supported_native_capabilities_do_not_require_tools() {
        for kind in ["tools","resources","prompts"] {
            let mut capabilities=json!({});capabilities[kind]=json!({});
            assert!(supported_capabilities(&json!({"capabilities":capabilities})).is_ok());
        }
        assert!(supported_capabilities(&json!({"capabilities":{}})).is_err());
        assert!(supported_capabilities(&json!({"capabilities":{"resources":true}})).is_err());
        assert!(supported_capabilities(&json!({})).is_err());
    }
    #[test]
    fn native_selection_requires_explicit_same_kind_authorization() {
        let mut native=json!({"name":"__keeper_mcp_resources_read","kind":"resources/read","inputSchema":{"type":"object"}});
        assert!(!selected(&native,0));
        let merged=merge_selection(&json!({"tools":[]}),vec![native.clone()]);
        assert_eq!(merged[0]["enabled"],false);
        native["enabled"]=json!(true);
        let prior=json!({"tools":[native]});
        let merged=merge_selection(&prior,vec![json!({"name":"__keeper_mcp_resources_read","kind":"resources/read"})]);
        assert_eq!(merged[0]["enabled"],true);
        let merged=merge_selection(&prior,vec![json!({"name":"__keeper_mcp_resources_read","kind":"prompts/get"})]);
        assert_eq!(merged[0]["enabled"],false);
        let merged=merge_selection(&json!({"tools":[]}),vec![json!({"name":"read_file","inputSchema":{"type":"object"}})]);
        assert_eq!(merged[0]["enabled"],true);
    }
    fn settings_store()->Store {
        let store=Store{conn:rusqlite::Connection::open_in_memory().unwrap()};
        store.conn.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE agent_projects(id TEXT PRIMARY KEY,name TEXT,directory TEXT,created_at INTEGER); INSERT INTO agent_projects VALUES ('p1','One','/tmp/one',0),('p2','Two','/tmp/two',0); CREATE TABLE agent_sessions(id TEXT PRIMARY KEY,title TEXT,updated_at INTEGER,project_id TEXT,pinned INTEGER,owner_key TEXT,archived_at INTEGER DEFAULT 0,deleted_at INTEGER DEFAULT 0); INSERT INTO agent_sessions(id,title,updated_at,project_id,pinned,owner_key) VALUES ('s1','one',0,'p1',0,'guest'),('s2','two',0,'p2',0,'guest'),('foreign','hidden',0,'p1',0,'other');").unwrap();
        initialize(&store).unwrap();store
    }
    fn settings_server(store:&Store)->Value {
        save(store,&json!({"name":"Fixture","command":"fixture-command","args":[],"transport":"stdio","environmentChanges":{"TEST_TOKEN":"private-env-sentinel","EMPTY":""},"projectIds":["p1"]})).unwrap()["servers"][0].clone()
    }
    fn enable_fixture(store:&Store,id:&str){
        let tools=json!([{"name":"read_fixture","enabled":true,"inputSchema":{"type":"object","properties":{}}}]);
        store.conn.execute("UPDATE agent_mcp_servers SET enabled=1,tools=?2 WHERE id=?1",params![id,tools.to_string()]).unwrap();
    }
    #[test]
    fn legacy_schema_adds_private_environment_and_global_scope_without_changing_rows(){
        let store=Store{conn:rusqlite::Connection::open_in_memory().unwrap()};
        store.conn.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE agent_mcp_servers(id TEXT PRIMARY KEY,owner_key TEXT,name TEXT,command TEXT,args TEXT,enabled INTEGER,tools TEXT); INSERT INTO agent_mcp_servers VALUES ('legacy','guest','Legacy','tool','[]',1,'[]');").unwrap();
        initialize(&store).unwrap();initialize(&store).unwrap();let servers=list(&store).unwrap();
        assert_eq!(servers[0]["name"],"Legacy");assert_eq!(servers[0]["enabled"],true);assert_eq!(servers[0]["environmentKeys"],json!([]));assert_eq!(servers[0]["environmentRevision"],"");assert_eq!(servers[0]["projectIds"],Value::Null);
    }
    #[test]
    fn environment_is_private_and_patch_revisions_change_only_with_values(){
        let store=settings_store();let initial=settings_server(&store);let id=initial["id"].as_str().unwrap();
        assert!(!initial.to_string().contains("private-env-sentinel"));assert!(uuid::Uuid::parse_str(initial["environmentRevision"].as_str().unwrap()).is_ok());
        assert_eq!(launch_config(&store,&initial).unwrap()["_keeperEnvironment"]["TEST_TOKEN"],"private-env-sentinel");
        enable_fixture(&store,id);let mut old=list(&store).unwrap()[0].clone();let revision=old["environmentRevision"].clone();
        old.as_object_mut().unwrap().remove("enabled");let unchanged=save(&store,&old).unwrap()["servers"][0].clone();
        assert_eq!(unchanged["enabled"],true);assert_eq!(unchanged["projectIds"],json!(["p1"]));assert_eq!(unchanged["environmentRevision"],revision);
        let mut patch=unchanged.clone();patch["environmentChanges"]=json!({"TEST_TOKEN":"private-env-sentinel"});
        assert_eq!(save(&store,&patch).unwrap()["servers"][0]["environmentRevision"],revision);
        patch["environmentChanges"]=json!({"TEST_TOKEN":null,"NEW_TOKEN":"different-private-value"});let changed=save(&store,&patch).unwrap()["servers"][0].clone();
        assert_ne!(changed["environmentRevision"],revision);assert_eq!(changed["enabled"],false);assert_eq!(changed["tools"],json!([]));assert_eq!(changed["environmentKeys"],json!(["EMPTY","NEW_TOKEN"]));assert!(!changed.to_string().contains("different-private-value"));
        assert!(launch_config(&store,&unchanged).is_err());
        let mut remote=changed.clone();remote["transport"]=json!("http");remote["command"]=json!("https://fixture.invalid/mcp");
        let saved=save(&store,&remote).unwrap()["servers"][0].clone();assert_eq!(saved["environmentKeys"],json!([]));assert!(launch_config(&store,&saved).unwrap().get("_keeperEnvironment").is_none());
        remote["environmentChanges"]=json!({"NEW_TOKEN":"should-not-be-sent"});assert!(save(&store,&remote).unwrap_err().find("should-not-be-sent").is_none());
    }
    #[test]
    fn definitions_and_resolution_use_the_sessions_actual_project_and_owner(){
        let store=settings_store();let initial=settings_server(&store);let id=initial["id"].as_str().unwrap();enable_fixture(&store,id);
        let p1=definitions_for(&store,"s1").unwrap();assert_eq!(p1.len(),1);assert!(definitions_for(&store,"s2").unwrap().is_empty());assert!(definitions_for(&store,"foreign").is_err());assert!(!json!(p1).to_string().contains("private-env-sentinel"));
        let name=p1[0]["function"]["name"].as_str().unwrap();assert!(resolve_for(&store,"s1",name).is_ok());assert!(resolve_for(&store,"s2",name).is_err());assert!(resolve_for(&store,"foreign",name).is_err());
        let mut scope=list(&store).unwrap()[0].clone();let old_tools=scope["tools"].clone();let revision=scope["environmentRevision"].clone();scope["projectIds"]=json!(["p2"]);scope.as_object_mut().unwrap().remove("enabled");let changed=save(&store,&scope).unwrap()["servers"][0].clone();
        assert_eq!(changed["enabled"],true);assert_eq!(changed["tools"],old_tools);assert_eq!(changed["environmentRevision"],revision);assert!(definitions_for(&store,"s1").unwrap().is_empty());assert_eq!(definitions_for(&store,"s2").unwrap().len(),1);assert!(resolve_for(&store,"s1",name).is_err());
        store.conn.execute("UPDATE agent_sessions SET archived_at=1 WHERE id='s2'",[]).unwrap();assert!(definitions_for(&store,"s2").is_err());
    }
    #[tokio::test]
    async fn connection_limit_and_single_checkout_are_enforced() {
        let pool = std::sync::Arc::new(ConnectionPool::default());
        let mut leases = Vec::new();
        for i in 0..MAX_CONNECTIONS {
            leases.push(pool.checkout(key(i)).unwrap());
        }
        assert!(pool.checkout(key(MAX_CONNECTIONS)).is_err());
        assert!(pool.checkout(key(0)).is_err());
        drop(leases.pop());
        assert!(pool.checkout(key(MAX_CONNECTIONS)).is_ok());
        pool.clear().unwrap();
        assert!(pool.state.lock().unwrap().active.is_empty());
    }
    #[tokio::test]
    async fn old_leases_cannot_remove_or_restore_a_new_generation() {
        let pool = std::sync::Arc::new(ConnectionPool::default());
        let mut old = pool.checkout(key(0)).unwrap();
        pool.clear().unwrap();
        assert!(old.invalidated.try_recv().is_ok());
        let new = pool.checkout(key(0)).unwrap();
        assert!(old.retain().is_err());
        assert_eq!(pool.state.lock().unwrap().active.len(), 1);
        drop(new);
        assert!(pool.state.lock().unwrap().active.is_empty());
    }
}
