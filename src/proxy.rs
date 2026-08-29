use std::time::{SystemTime, UNIX_EPOCH};

/// Hosts that must never go through HTTP_PROXY / Clash (SSH console forwards
/// bind here; a system proxy often answers those CONNECT requests with 502).
const LOOPBACK_NO_PROXY: &[&str] = &["localhost", "127.0.0.1", "::1", "[::1]"];

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, timeout};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_HEADER: usize = 64 * 1024;

/// Saved proxy choice (singleton in SQLite).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettings {
    /// `local`, `existing`, or empty when never configured.
    pub mode: String,
    pub host: String,
    pub port: u16,
    pub enabled: bool,
    pub last_reachable: bool,
    pub last_http: bool,
    pub last_socks5: bool,
    pub last_message: String,
    pub last_checked_at: i64,
}

/// Merge loopback entries into `NO_PROXY` / `no_proxy` so WebKit does not send
/// `http://127.0.0.1:<mapped-port>` through the system proxy.
pub fn ensure_loopback_not_proxied() {
    for key in ["NO_PROXY", "no_proxy"] {
        let merged = merge_no_proxy(std::env::var(key).ok().as_deref());
        // SAFETY: called once at process start, before the webview starts.
        unsafe {
            std::env::set_var(key, merged);
        }
    }
    // GNOME's proxy resolver often ignores NO_PROXY and still CONNECT-proxies
    // loopback; Clash then returns 502. Dummy resolver is process-local and
    // only affects GIO/WebKit — reqwest still honours HTTP_PROXY.
    #[cfg(target_os = "linux")]
    if std::env::var_os("GIO_USE_PROXY_RESOLVER").is_none() {
        unsafe {
            std::env::set_var("GIO_USE_PROXY_RESOLVER", "dummy");
        }
    }
}

fn merge_no_proxy(existing: Option<&str>) -> String {
    let mut parts: Vec<String> = existing
        .unwrap_or("")
        .split(|c: char| c == ',' || c == ';' || c.is_ascii_whitespace())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    for extra in LOOPBACK_NO_PROXY {
        if !parts.iter().any(|p| p.eq_ignore_ascii_case(extra)) {
            parts.push((*extra).to_string());
        }
    }
    parts.join(",")
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            mode: String::new(),
            host: "127.0.0.1".into(),
            port: 7890,
            enabled: false,
            last_reachable: false,
            last_http: false,
            last_socks5: false,
            last_message: String::new(),
            last_checked_at: 0,
        }
    }
}

/// Live status returned to the UI and other panels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub mode: String,
    pub host: String,
    pub port: u16,
    /// Address other panels should dial (always a host this process can use).
    pub endpoint: String,
    /// Bind address for a local mixed proxy (`0.0.0.0`).
    pub bind: String,
    pub enabled: bool,
    pub running: bool,
    pub reachable: bool,
    pub http: bool,
    pub socks5: bool,
    pub message: String,
    pub last_checked_at: i64,
}

impl ProxyStatus {
    pub fn from_settings(settings: &ProxySettings, running: bool) -> Self {
        let local = settings.mode == "local";
        let host = if local {
            "127.0.0.1".to_string()
        } else if settings.host.trim().is_empty() {
            "127.0.0.1".to_string()
        } else {
            settings.host.clone()
        };
        let port = settings.port;
        let endpoint = if port == 0 {
            String::new()
        } else {
            format!("{host}:{port}")
        };
        Self {
            mode: settings.mode.clone(),
            host,
            port,
            endpoint,
            bind: if local {
                "0.0.0.0".into()
            } else {
                String::new()
            },
            enabled: settings.enabled,
            running,
            reachable: settings.last_reachable,
            http: settings.last_http,
            socks5: settings.last_socks5,
            message: settings.last_message.clone(),
            last_checked_at: settings.last_checked_at,
        }
    }
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn validate_port(port: u16) -> Result<(), String> {
    if port == 0 {
        Err("Port must be between 1 and 65535".into())
    } else {
        Ok(())
    }
}

pub fn validate_host(host: &str) -> Result<(), String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("Proxy address is required".into());
    }
    if host.contains('/') || host.contains(' ') {
        return Err("Invalid proxy address".into());
    }
    Ok(())
}

/// Bind a mixed HTTP CONNECT + SOCKS5 listener on every interface (host network).
pub async fn bind_local(port: u16) -> Result<TcpListener, String> {
    validate_port(port)?;
    TcpListener::bind(("0.0.0.0", port))
        .await
        .map_err(|e| format!("Failed to bind 0.0.0.0:{port}: {e}"))
}

pub async fn accept_loop(listener: TcpListener, mut cancel: tokio::sync::oneshot::Receiver<()>) {
    loop {
        tokio::select! {
            _ = &mut cancel => break,
            accepted = listener.accept() => {
                match accepted {
                    Ok((socket, _)) => {
                        tokio::spawn(async move {
                            let _ = handle_client(socket).await;
                        });
                    }
                    Err(_) => {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        }
    }
}

async fn handle_client(socket: TcpStream) -> Result<(), std::io::Error> {
    let mut peek = [0u8; 1];
    let n = timeout(HEADER_TIMEOUT, socket.peek(&mut peek))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "peek timeout"))??;
    if n == 0 {
        return Ok(());
    }
    if peek[0] == 0x05 {
        handle_socks5(socket).await
    } else {
        handle_http(socket).await
    }
}

async fn handle_socks5(mut socket: TcpStream) -> Result<(), std::io::Error> {
    let mut hdr = [0u8; 2];
    timeout(HEADER_TIMEOUT, socket.read_exact(&mut hdr))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "socks greeting"))??;
    if hdr[0] != 0x05 {
        return Ok(());
    }
    let nmethods = hdr[1] as usize;
    let mut methods = vec![0u8; nmethods];
    if nmethods > 0 {
        timeout(HEADER_TIMEOUT, socket.read_exact(&mut methods))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "socks methods"))??;
    }
    if !methods.is_empty() && !methods.contains(&0x00) && nmethods > 0 {
        socket.write_all(&[0x05, 0xff]).await?;
        return Ok(());
    }
    socket.write_all(&[0x05, 0x00]).await?;

    let mut req = [0u8; 4];
    timeout(HEADER_TIMEOUT, socket.read_exact(&mut req))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "socks request"))??;
    if req[0] != 0x05 {
        return Ok(());
    }
    let cmd = req[1];
    let atyp = req[3];
    let target = match read_socks_target(&mut socket, atyp).await {
        Ok(t) => t,
        Err(_) => {
            let _ = write_socks_reply(&mut socket, 0x01).await;
            return Ok(());
        }
    };

    if cmd != 0x01 {
        write_socks_reply(&mut socket, 0x07).await?;
        return Ok(());
    }

    match timeout(CONNECT_TIMEOUT, TcpStream::connect(&target)).await {
        Ok(Ok(mut upstream)) => {
            write_socks_reply(&mut socket, 0x00).await?;
            let _ = tokio::io::copy_bidirectional(&mut socket, &mut upstream).await;
        }
        _ => {
            write_socks_reply(&mut socket, 0x05).await?;
        }
    }
    Ok(())
}

async fn read_socks_target(socket: &mut TcpStream, atyp: u8) -> Result<String, std::io::Error> {
    match atyp {
        0x01 => {
            let mut buf = [0u8; 6];
            socket.read_exact(&mut buf).await?;
            let host = format!("{}.{}.{}.{}", buf[0], buf[1], buf[2], buf[3]);
            let port = u16::from_be_bytes([buf[4], buf[5]]);
            Ok(format!("{host}:{port}"))
        }
        0x03 => {
            let mut len = [0u8; 1];
            socket.read_exact(&mut len).await?;
            let mut name = vec![0u8; len[0] as usize];
            socket.read_exact(&mut name).await?;
            let mut portb = [0u8; 2];
            socket.read_exact(&mut portb).await?;
            let port = u16::from_be_bytes(portb);
            let host = String::from_utf8_lossy(&name);
            Ok(format!("{host}:{port}"))
        }
        0x04 => {
            let mut buf = [0u8; 18];
            socket.read_exact(&mut buf).await?;
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&buf[..16]);
            let addr = std::net::Ipv6Addr::from(octets);
            let port = u16::from_be_bytes([buf[16], buf[17]]);
            Ok(format!("[{addr}]:{port}"))
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unknown SOCKS atyp",
        )),
    }
}

async fn write_socks_reply(socket: &mut TcpStream, rep: u8) -> Result<(), std::io::Error> {
    socket
        .write_all(&[0x05, rep, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await
}

async fn handle_http(mut socket: TcpStream) -> Result<(), std::io::Error> {
    let head = match timeout(HEADER_TIMEOUT, read_http_head(&mut socket)).await {
        Ok(Ok(h)) if !h.is_empty() => h,
        _ => return Ok(()),
    };
    let text = String::from_utf8_lossy(&head);
    let first = text.lines().next().unwrap_or("").trim();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let target = parts.next().unwrap_or("");

    if method == "CONNECT" {
        let dest = match parse_connect_target(target) {
            Some(d) => d,
            None => {
                let _ = socket
                    .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
                    .await;
                return Ok(());
            }
        };
        match timeout(CONNECT_TIMEOUT, TcpStream::connect(&dest)).await {
            Ok(Ok(mut upstream)) => {
                socket
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await?;
                let _ = tokio::io::copy_bidirectional(&mut socket, &mut upstream).await;
            }
            _ => {
                let _ = socket
                    .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        }
        return Ok(());
    }

    if let Some((host, port, path)) = parse_absolute_http_url(target) {
        let dest = format!("{host}:{port}");
        match timeout(CONNECT_TIMEOUT, TcpStream::connect(&dest)).await {
            Ok(Ok(mut upstream)) => {
                let rest = match text.split_once("\r\n") {
                    Some((_, r)) => r,
                    None => "\r\n",
                };
                let filtered = filter_proxy_headers(rest);
                let req = format!("{method} {path} HTTP/1.1\r\n{filtered}");
                upstream.write_all(req.as_bytes()).await?;
                let _ = tokio::io::copy_bidirectional(&mut socket, &mut upstream).await;
            }
            _ => {
                let _ = socket
                    .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        }
        return Ok(());
    }

    let _ = socket
        .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
        .await;
    Ok(())
}

async fn read_http_head(socket: &mut TcpStream) -> Result<Vec<u8>, std::io::Error> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = socket.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > MAX_HEADER {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP header too large",
            ));
        }
    }
    Ok(buf)
}

fn parse_connect_target(target: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    if let Some(rest) = target.strip_prefix('[') {
        let (addr, port) = rest.split_once("]:")?;
        let _port: u16 = port.parse().ok()?;
        Some(format!("[{addr}]:{port}"))
    } else {
        let (host, port) = target.rsplit_once(':')?;
        let _port: u16 = port.parse().ok()?;
        if host.is_empty() {
            return None;
        }
        Some(format!("{host}:{port}"))
    }
}

fn parse_absolute_http_url(url: &str) -> Option<(String, u16, String)> {
    let url = url.trim();
    let rest = url.strip_prefix("http://")?;
    let (auth, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if auth.is_empty() {
        return None;
    }
    let (host, port) = if let Some(rest) = auth.strip_prefix('[') {
        let (h, p) = rest.split_once("]:")?;
        let port = if p.is_empty() { 80 } else { p.parse().ok()? };
        (h.to_string(), port)
    } else if let Some((h, p)) = auth.rsplit_once(':') {
        (h.to_string(), p.parse().ok()?)
    } else {
        (auth.to_string(), 80)
    };
    Some((host, port, path.to_string()))
}

fn filter_proxy_headers(rest: &str) -> String {
    let mut out = String::new();
    for line in rest.split("\r\n") {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("proxy-connection:") || lower.starts_with("proxy-authorization:") {
            continue;
        }
        out.push_str(line);
        out.push_str("\r\n");
        if line.is_empty() {
            break;
        }
    }
    if !out.ends_with("\r\n\r\n") {
        out.push_str("\r\n");
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct ProbeResult {
    pub reachable: bool,
    pub http: bool,
    pub socks5: bool,
    pub message: String,
}

/// TCP + protocol probe used when applying a proxy and on Check.
pub async fn probe_proxy(host: &str, port: u16) -> ProbeResult {
    if validate_port(port).is_err() {
        return ProbeResult {
            message: "Invalid port".into(),
            ..Default::default()
        };
    }
    let addr = format!("{host}:{port}");
    let tcp = timeout(PROBE_TIMEOUT, TcpStream::connect(&addr)).await;
    match tcp {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            return ProbeResult {
                message: format!("TCP connect failed: {e}"),
                ..Default::default()
            };
        }
        Err(_) => {
            return ProbeResult {
                message: "TCP connect timed out".into(),
                ..Default::default()
            };
        }
    }

    let socks5 = probe_socks5(&addr).await;
    let http = probe_http(&addr).await;

    let mut result = ProbeResult {
        reachable: true,
        http,
        socks5,
        message: String::new(),
    };
    result.message = if http && socks5 {
        "HTTP + SOCKS5 可用".into()
    } else if http {
        "HTTP 代理可用".into()
    } else if socks5 {
        "SOCKS5 代理可用".into()
    } else {
        "端口可连，但不是 HTTP/SOCKS5 代理".into()
    };
    result
}

async fn probe_socks5(addr: &str) -> bool {
    let Ok(Ok(mut stream)) = timeout(PROBE_TIMEOUT, TcpStream::connect(addr)).await else {
        return false;
    };
    if stream.write_all(&[0x05, 0x01, 0x00]).await.is_err() {
        return false;
    }
    let mut resp = [0u8; 2];
    match timeout(PROBE_TIMEOUT, stream.read_exact(&mut resp)).await {
        Ok(Ok(_)) => resp == [0x05, 0x00],
        _ => false,
    }
}

async fn probe_http(addr: &str) -> bool {
    let Ok(Ok(mut stream)) = timeout(PROBE_TIMEOUT, TcpStream::connect(addr)).await else {
        return false;
    };
    // CONNECT to a closed local port: a real HTTP proxy still speaks HTTP
    // (200 then RST, or 502/503). Non-proxies usually do not.
    let req = b"CONNECT 127.0.0.1:1 HTTP/1.1\r\nHost: 127.0.0.1:1\r\n\r\n";
    if stream.write_all(req).await.is_err() {
        return false;
    }
    let mut buf = [0u8; 16];
    match timeout(PROBE_TIMEOUT, stream.read(&mut buf)).await {
        Ok(Ok(n)) if n >= 5 => buf[..n].starts_with(b"HTTP/"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    async fn spawn_echo() -> (u16, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = tokio::spawn(async move {
            if let Ok((mut s, _)) = listener.accept().await {
                let mut buf = [0u8; 64];
                if let Ok(n) = s.read(&mut buf).await {
                    let _ = s.write_all(&buf[..n]).await;
                }
            }
        });
        (port, h)
    }

    async fn spawn_proxy() -> (
        u16,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let h = tokio::spawn(async move {
            accept_loop(listener, rx).await;
        });
        (port, tx, h)
    }

    #[tokio::test]
    async fn socks5_connects_and_pipes() {
        let (echo_port, echo) = spawn_echo().await;
        let (proxy_port, stop, proxy) = spawn_proxy().await;

        let mut c = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        c.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        let mut g = [0u8; 2];
        c.read_exact(&mut g).await.unwrap();
        assert_eq!(g, [0x05, 0x00]);

        let mut req = vec![0x05, 0x01, 0x00, 0x01, 127, 0, 0, 1];
        req.extend_from_slice(&echo_port.to_be_bytes());
        c.write_all(&req).await.unwrap();
        let mut rep = [0u8; 10];
        c.read_exact(&mut rep).await.unwrap();
        assert_eq!(rep[1], 0x00);

        c.write_all(b"ping").await.unwrap();
        let mut out = [0u8; 4];
        c.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"ping");

        let _ = stop.send(());
        let _ = echo.await;
        let _ = proxy.await;
    }

    #[tokio::test]
    async fn http_connect_pipes() {
        let (echo_port, echo) = spawn_echo().await;
        let (proxy_port, stop, proxy) = spawn_proxy().await;

        let mut c = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        let req = format!(
            "CONNECT 127.0.0.1:{echo_port} HTTP/1.1\r\nHost: 127.0.0.1:{echo_port}\r\n\r\n"
        );
        c.write_all(req.as_bytes()).await.unwrap();
        let mut buf = [0u8; 64];
        let n = c.read(&mut buf).await.unwrap();
        let head = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");

        c.write_all(b"xyz").await.unwrap();
        let mut out = [0u8; 3];
        c.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"xyz");

        let _ = stop.send(());
        let _ = echo.await;
        let _ = proxy.await;
    }

    #[tokio::test]
    async fn probe_detects_mixed_proxy() {
        let (proxy_port, stop, proxy) = spawn_proxy().await;
        let result = probe_proxy("127.0.0.1", proxy_port).await;
        assert!(result.reachable);
        assert!(result.socks5, "{}", result.message);
        assert!(result.http, "{}", result.message);
        let _ = stop.send(());
        let _ = proxy.await;
    }

    #[test]
    fn merge_no_proxy_adds_loopback() {
        let merged = merge_no_proxy(Some("example.com,10.0.0.0/8"));
        assert!(merged.contains("example.com"));
        assert!(merged.contains("127.0.0.1"));
        assert!(merged.contains("localhost"));
    }

    #[test]
    fn merge_no_proxy_does_not_duplicate() {
        let merged = merge_no_proxy(Some("localhost,127.0.0.1"));
        assert_eq!(merged.matches("127.0.0.1").count(), 1, "{merged}");
    }

    #[test]
    fn parse_connect_host_port() {
        assert_eq!(
            parse_connect_target("example.com:443").as_deref(),
            Some("example.com:443")
        );
        assert_eq!(
            parse_connect_target("[::1]:9050").as_deref(),
            Some("[::1]:9050")
        );
        assert!(parse_connect_target("no-port").is_none());
    }
}
