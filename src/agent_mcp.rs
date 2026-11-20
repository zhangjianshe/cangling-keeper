//! Local MCP stdio connections and persisted, user-configured server metadata.
use crate::store::Store;
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
    Ok(())
}
pub(crate) fn list(store: &Store) -> Result<Value, String> {
    let owner = crate::agent_account::owner(store)?;
    let mut stmt=store.conn.prepare("SELECT id,name,command,args,enabled,tools,transport FROM agent_mcp_servers WHERE owner_key=?1 ORDER BY rowid").map_err(|e|e.to_string())?;
    let rows=stmt.query_map([owner],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"command":r.get::<_,String>(2)?,"args":serde_json::from_str::<Value>(&r.get::<_,String>(3)?).unwrap_or(json!([])),"enabled":r.get::<_,bool>(4)?,"tools":serde_json::from_str::<Value>(&r.get::<_,String>(5)?).unwrap_or(json!([])),"transport":r.get::<_,String>(6)?}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    Ok(json!(rows))
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
    let unchanged = existing.as_ref().is_some_and(|v| {
        v["command"] == command && v["args"] == args["args"] && v["transport"] == transport
    });
    let tools = if unchanged {
        existing.as_ref().unwrap()["tools"].to_string()
    } else {
        "[]".to_string()
    };
    let enabled = unchanged && args["enabled"] == true;
    store.conn.execute("INSERT INTO agent_mcp_servers(id,owner_key,name,command,args,enabled,tools,transport) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET name=excluded.name,command=excluded.command,args=excluded.args,enabled=excluded.enabled,tools=excluded.tools,transport=excluded.transport WHERE owner_key=excluded.owner_key",params![id,owner,name,command,json!(argv).to_string(),enabled,tools,transport]).map_err(|e|e.to_string())?;
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
        };
        let reply=c.request(1,"initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"cangling-keeper","version":env!("CARGO_PKG_VERSION")}})).await?;
        if !reply["capabilities"]["tools"].is_object() {
            return Err("MCP Server 不支持工具能力".into());
        }
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
                .take(262145)
                .read_until(b'\n', &mut line)
                .await
                .map_err(|_| "读取 MCP 响应失败")?;
            if n == 0 || line.len() > 262144 || !line.ends_with(b"\n") {
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
    async fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, String> {
        match self {
            Self::Stdio(c) => c.request(id, method, params).await,
            Self::Http(c) => c.request(id, method, params).await,
        }
    }
}
pub(crate) async fn discover(server: &Value) -> Result<Vec<Value>, String> {
    tokio::time::timeout(Duration::from_secs(15),async{
 let mut client=Client::start(server).await?;let mut tools=Vec::new();let mut cursor=None::<String>;
 for id in 2..12 {let result=client.request(id,"tools/list",cursor.as_ref().map_or(json!({}),|s|json!({"cursor":s}))).await?;
 for tool in result["tools"].as_array().ok_or("MCP 工具列表无效")? {let name=tool["name"].as_str().ok_or("MCP 工具名称无效")?;if name.is_empty() || name.len()>128 || tool["inputSchema"]["type"]!="object" || tool.to_string().len()>32768{return Err("MCP 工具声明无效或过大".into());}if tools.iter().any(|t:&Value|t["name"]==name){return Err("MCP 工具名称重复".into());}tools.push(json!({"name":name,"description":tool["description"].as_str().unwrap_or(""),"inputSchema":tool["inputSchema"]}));if tools.len()>64{return Err("MCP 工具数量超过限制".into());}}
 cursor=result["nextCursor"].as_str().map(str::to_owned);if cursor.is_none(){return Ok(tools);}}
 Err("MCP 工具分页超过限制".into())
 }).await.map_err(|_|"MCP Server 连接超时".to_owned())?
}

pub(crate) fn definitions(store: &Store) -> Result<Vec<Value>, String> {
    let servers = list(store)?;
    let mut result = Vec::new();
    for server in servers
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["enabled"] == true)
    {
        for (index, tool) in server["tools"].as_array().unwrap().iter().enumerate() {
            if !selected(tool, index) {
                continue;
            }
            result.push(json!({"type":"function","function":{"name":tool_id(server,tool,index),"description":format!("MCP 服务 {} / {}: {}",server["name"].as_str().unwrap_or(""),tool["name"].as_str().unwrap_or(""),tool["description"].as_str().unwrap_or("")),"parameters":tool["inputSchema"]}}));
        }
    }
    Ok(result)
}
fn selected(tool: &Value, index: usize) -> bool {
    tool["enabled"].as_bool().unwrap_or(index < 8)
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
        let enabled = if has_selection {
            previous
                .unwrap()
                .iter()
                .find(|p| p["name"] == tool["name"])
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
pub(crate) async fn call(server: &Value, tool: &Value, args: &Value) -> Result<Value, String> {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mut client = Client::start(server).await?;
        let response = client
            .request(
                2,
                "tools/call",
                json!({"name":tool["name"],"arguments":args}),
            )
            .await?;
        let text = response["content"]
            .as_array()
            .ok_or("MCP 工具结果无效")?
            .iter()
            .filter_map(|x| {
                if x["type"] == "text" {
                    x["text"].as_str()
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.len() > 40000 {
            return Err("MCP 工具结果过大")?;
        }
        if response["isError"] == true {
            return Err(format!("MCP 工具错误：{text}"));
        }
        Ok(json!({"output":text}))
    })
    .await
    .map_err(|_| "MCP 工具执行超时".to_owned())?
}
