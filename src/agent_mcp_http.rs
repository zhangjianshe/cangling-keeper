//! MCP Streamable HTTP and legacy HTTP+SSE transports.
use reqwest::{
    Client as HttpClient, Response, Url,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
};
use serde_json::{Value, json};

pub(crate) fn validate_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| "MCP 地址无效")?;
    let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if (url.scheme() != "https" && !(url.scheme() == "http" && local))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("MCP 地址需要 HTTPS；本机允许 HTTP，地址不能包含账号、密码或片段".into());
    }
    Ok(url)
}
pub(crate) fn headers(args: &[Value]) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    for item in args {
        let line = item.as_str().ok_or("MCP 请求头无效")?;
        let (name, value) = line.split_once(':').ok_or("请求头格式应为 名称: 值")?;
        let key =
            HeaderName::from_bytes(name.trim().as_bytes()).map_err(|_| "MCP 请求头名称无效")?;
        if matches!(
            key.as_str(),
            "host"
                | "connection"
                | "content-length"
                | "transfer-encoding"
                | "accept"
                | "content-type"
                | "mcp-session-id"
                | "mcp-protocol-version"
        ) {
            return Err("该 MCP 请求头由系统管理，不能手动设置".into());
        }
        let value = HeaderValue::from_str(value.trim()).map_err(|_| "MCP 请求头值无效")?;
        if headers.contains_key(&key) {
            return Err("MCP 请求头不能重复".into());
        }
        headers.insert(key, value);
    }
    Ok(headers)
}
struct EventStream {
    response: Response,
    buffer: Vec<u8>,
}
impl EventStream {
    async fn next(&mut self) -> Result<(String, String), String> {
        loop {
            if let Some(pos) = self.buffer.windows(2).position(|w| w == b"\n\n") {
                let data = self.buffer.drain(..pos + 2).collect::<Vec<_>>();
                let text = std::str::from_utf8(&data).map_err(|_| "MCP SSE 编码无效")?;
                let mut kind = "message";
                let mut lines = Vec::new();
                for line in text.lines() {
                    if let Some(v) = line.strip_prefix("event:") {
                        kind = v.trim();
                    } else if let Some(v) = line.strip_prefix("data:") {
                        lines.push(v.strip_prefix(' ').unwrap_or(v));
                    }
                }
                if !lines.is_empty() {
                    return Ok((kind.to_owned(), lines.join("\n")));
                }
                continue;
            }
            let chunk = self
                .response
                .chunk()
                .await
                .map_err(|_| "MCP SSE 连接中断")?
                .ok_or("MCP SSE 连接已关闭")?;
            for byte in chunk {
                if byte != b'\r' {
                    self.buffer.push(byte);
                }
            }
            if self.buffer.len() > 262144 {
                return Err("MCP SSE 消息过大".into());
            }
        }
    }
}
enum Transport {
    Legacy {
        stream: EventStream,
        endpoint: Url,
    },
    Streamable {
        endpoint: Url,
        session: Option<String>,
        version: Option<String>,
    },
}
pub(crate) struct Client {
    http: HttpClient,
    headers: HeaderMap,
    transport: Transport,
    pending: Option<u64>,
    capabilities: Value,
}
impl Client {
    pub(crate) async fn start(server: &Value) -> Result<Self, String> {
        let url = validate_url(server["command"].as_str().ok_or("缺少 MCP 地址")?)?;
        let headers = headers(server["args"].as_array().ok_or("MCP 请求头无效")?)?;
        let http = HttpClient::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|_| "无法建立 MCP 连接")?;
        let transport = if server["transport"] == "sse" {
            let response = http
                .get(url.clone())
                .headers(headers.clone())
                .header(ACCEPT, "text/event-stream")
                .send()
                .await
                .map_err(|_| "无法连接 MCP SSE")?;
            if !response.status().is_success()
                || !response
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|s| s.starts_with("text/event-stream"))
            {
                return Err("MCP SSE 服务响应无效".into());
            }
            let mut stream = EventStream {
                response,
                buffer: Vec::new(),
            };
            let (kind, data) = stream.next().await?;
            if kind != "endpoint" {
                return Err("MCP SSE 缺少 endpoint 事件".into());
            }
            let endpoint = url.join(&data).map_err(|_| "MCP SSE endpoint 无效")?;
            if endpoint.origin() != url.origin()
                || !endpoint.username().is_empty()
                || endpoint.password().is_some()
                || endpoint.fragment().is_some()
            {
                return Err("MCP SSE endpoint 必须与服务地址同源".into());
            }
            Transport::Legacy { stream, endpoint }
        } else {
            Transport::Streamable {
                endpoint: url,
                session: None,
                version: None,
            }
        };
        let mut c = Self {
            http,
            headers,
            transport,
            pending: None,
            capabilities: Value::Null,
        };
        let info=c.request(1,"initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"cangling-keeper","version":env!("CARGO_PKG_VERSION")}})).await?;
        c.capabilities = crate::agent_mcp::supported_capabilities(&info)?;
        if let Transport::Streamable { version, .. } = &mut c.transport {
            *version = info["protocolVersion"].as_str().map(str::to_owned);
        }
        c.notify("notifications/initialized", json!({})).await?;
        Ok(c)
    }
    pub(crate) fn capabilities(&self) -> &Value { &self.capabilities }
    async fn post(
        &self,
        endpoint: &Url,
        body: &Value,
        session: Option<&str>,
        version: Option<&str>,
    ) -> Result<Response, String> {
        let mut req = self
            .http
            .post(endpoint.clone())
            .headers(self.headers.clone())
            .header(ACCEPT, "application/json, text/event-stream")
            .json(body);
        if let Some(s) = session {
            req = req.header("Mcp-Session-Id", s);
        }
        if let Some(v) = version {
            req = req.header("MCP-Protocol-Version", v);
        }
        let response = req.send().await.map_err(|_| "MCP HTTP 请求失败")?;
        if !response.status().is_success() {
            return Err(format!(
                "MCP HTTP 请求失败（HTTP {}）",
                response.status().as_u16()
            ));
        }
        Ok(response)
    }
    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let (endpoint, session, version) = match &self.transport {
            Transport::Legacy { endpoint, .. } => (endpoint.clone(), None, None),
            Transport::Streamable {
                endpoint,
                session,
                version,
            } => (endpoint.clone(), session.clone(), version.clone()),
        };
        self.post(
            &endpoint,
            &json!({"jsonrpc":"2.0","method":method,"params":params}),
            session.as_deref(),
            version.as_deref(),
        )
        .await?;
        Ok(())
    }
    pub(crate) async fn request(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        self.pending = Some(id);
        let result = self.request_inner(id, method, params).await;
        self.pending = None;
        result
    }
    async fn request_inner(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let (endpoint, session, version) = match &self.transport {
            Transport::Legacy { endpoint, .. } => (endpoint.clone(), None, None),
            Transport::Streamable {
                endpoint,
                session,
                version,
            } => (endpoint.clone(), session.clone(), version.clone()),
        };
        let response = self
            .post(
                &endpoint,
                &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
                session.as_deref(),
                version.as_deref(),
            )
            .await?;
        if let Transport::Streamable { session, .. } = &mut self.transport {
            if method == "initialize" {
                *session = response
                    .headers()
                    .get("Mcp-Session-Id")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
            }
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        match &mut self.transport {
            Transport::Legacy { stream, .. } => loop {
                let (kind, data) = stream.next().await?;
                if kind != "message" {
                    continue;
                }
                let v: Value = serde_json::from_str(&data).map_err(|_| "MCP SSE 响应无效")?;
                if v["id"] == id {
                    return result(v);
                }
            },
            Transport::Streamable { .. } => {
                if content_type.starts_with("text/event-stream") {
                    let mut stream = EventStream {
                        response,
                        buffer: Vec::new(),
                    };
                    loop {
                        let (_, data) = stream.next().await?;
                        let v: Value =
                            serde_json::from_str(&data).map_err(|_| "MCP HTTP SSE 响应无效")?;
                        if v["id"] == id {
                            return result(v);
                        }
                    }
                }
                if !content_type.starts_with("application/json") {
                    return Err("MCP HTTP 内容类型无效".into());
                }
                let mut response = response;
                let mut bytes = Vec::new();
                while let Some(chunk) = response.chunk().await.map_err(|_| "MCP HTTP 读取失败")?
                {
                    if bytes.len() + chunk.len() > 262144 {
                        return Err("MCP HTTP 响应过大".into());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                let value: Value =
                    serde_json::from_slice(&bytes).map_err(|_| "MCP HTTP 响应无效")?;
                if value["id"] != id {
                    return Err("MCP HTTP 响应 ID 不匹配".into());
                }
                result(value)
            }
        }
    }
}
fn result(v: Value) -> Result<Value, String> {
    if v.get("error").is_some() {
        return Err("MCP Server 返回错误".into());
    }
    v.get("result").cloned().ok_or("MCP 响应缺少结果".into())
}

impl Drop for Client {
    fn drop(&mut self) {
        let id = self.pending;
        let (endpoint, session, version) = match &self.transport {
            Transport::Legacy { endpoint, .. } => (endpoint.clone(), None, None),
            Transport::Streamable {
                endpoint,
                session,
                version,
            } => (endpoint.clone(), session.clone(), version.clone()),
        };
        if id.is_none() && session.is_none() {
            return;
        }
        let http = self.http.clone();
        let headers = self.headers.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
   if let Some(id)=id{let mut request=http.post(endpoint.clone()).headers(headers.clone()).header(ACCEPT,"application/json, text/event-stream").json(&json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id,"reason":"Stopped by user or timeout"}}));if let Some(s)=&session{request=request.header("Mcp-Session-Id",s);}if let Some(v)=&version{request=request.header("MCP-Protocol-Version",v);}let _=tokio::time::timeout(std::time::Duration::from_secs(5),request.send()).await;}
   if let Some(session)=session{let mut request=http.delete(endpoint).headers(headers).header("Mcp-Session-Id",session);if let Some(v)=version{request=request.header("MCP-Protocol-Version",v);}let _=tokio::time::timeout(std::time::Duration::from_secs(5),request.send()).await;}
  });
        }
    }
}
