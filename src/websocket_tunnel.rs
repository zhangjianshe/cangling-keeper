use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use futures_util::{SinkExt, StreamExt};
use russh::keys::ssh_key::{HashAlg, Signature};
use russh::keys::{Algorithm, PrivateKey};
use russh::{Disconnect, client};
use signature::Signer;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

use crate::ssh::{self, ResolvedAuth};
use crate::tunnel::Tunnel;

const SIGNATURE_DOMAIN: &str = "cangling-tunnel-v1";
const KEY_HEADER: &str = "x-cangling-tunnel-key";
const TIME_HEADER: &str = "x-cangling-tunnel-time";
const NONCE_HEADER: &str = "x-cangling-tunnel-nonce";
const SIGNATURE_HEADER: &str = "x-cangling-tunnel-signature";

pub type Reporter = Arc<dyn Fn(&str, &str) + Send + Sync>;

pub struct Established {
    listener: TcpListener,
    url: String,
    private_key: Arc<PrivateKey>,
    report: Option<Reporter>,
}

struct ReverseWebSocketClient {
    url: String,
    private_key: Arc<PrivateKey>,
    report: Option<Reporter>,
}

impl client::Handler for ReverseWebSocketClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _connected_address: &str,
        _connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        let url = self.url.clone();
        let private_key = self.private_key.clone();
        let report = self.report.clone();
        tokio::spawn(async move {
            forward_reported(channel.into_stream(), &url, &private_key, report).await;
        });
        Ok(())
    }
}

pub struct EstablishedReverse {
    report: Option<Reporter>,
    session: client::Handle<ReverseWebSocketClient>,
    remote_host: String,
    remote_port: u16,
}

pub async fn establish(
    tunnel: &Tunnel,
    private_key_path: &str,
    report: Option<Reporter>,
) -> Result<Established, String> {
    let private_key = russh::keys::load_secret_key(private_key_path, None)
        .map_err(|e| format!("Failed to load WebSocket tunnel key '{private_key_path}': {e}"))?;
    if private_key.algorithm() != Algorithm::Ed25519 {
        return Err("WebSocket tunnel requires an Ed25519 certificate".into());
    }
    let bind = format!("{}:{}", tunnel.local_host, tunnel.local_port);
    let listener = TcpListener::bind(&bind)
        .await
        .map_err(|e| format!("Failed to listen on {bind}: {e}"))?;
    Ok(Established {
        listener,
        url: tunnel.websocket_url.clone(),
        private_key: Arc::new(private_key),
        report,
    })
}

pub async fn establish_reverse(
    tunnel: &Tunnel,
    private_key_path: &str,
    report: Option<Reporter>,
) -> Result<EstablishedReverse, String> {
    let private_key = russh::keys::load_secret_key(private_key_path, None)
        .map_err(|e| format!("Failed to load WebSocket tunnel key '{private_key_path}': {e}"))?;
    if private_key.algorithm() != Algorithm::Ed25519 {
        return Err("WebSocket tunnel requires an Ed25519 certificate".into());
    }
    let handler = ReverseWebSocketClient {
        url: tunnel.websocket_url.clone(),
        private_key: Arc::new(private_key),
        report: report.clone(),
    };
    let mut session = ssh::connect_with(&tunnel.ssh_host, tunnel.ssh_port, handler).await?;
    ssh::authenticate(
        &mut session,
        &tunnel.username,
        &ResolvedAuth::Key(private_key_path.to_string()),
    )
    .await?;
    let remote_host = tunnel.remote_host.trim().to_string();
    session
        .tcpip_forward(&remote_host, tunnel.local_port as u32)
        .await
        .map_err(|error| {
            let hint = if remote_host == "0.0.0.0" {
                "; 请检查远端 sshd 是否启用 GatewayPorts clientspecified（或 yes）"
            } else {
                ""
            };
            format!(
                "远端监听 {remote_host}:{} 失败: {error:?}{hint}",
                tunnel.local_port
            )
        })?;
    Ok(EstablishedReverse {
        report,
        session,
        remote_host,
        remote_port: tunnel.local_port,
    })
}

pub async fn hold_reverse(
    established: EstablishedReverse,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) {
    let EstablishedReverse {
        report,
        session,
        remote_host,
        remote_port,
    } = established;
    loop {
        tokio::select! {
            _ = &mut stop => break,
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                if session.is_closed() {
                    if let Some(report) = &report { report("error", "远端监听的 SSH 连接已断开，请重新连接隧道"); }
                    break;
                }
                if let Err(error) = session.send_keepalive(false).await {
                    if let Some(report) = &report { report("error", &format!("远端监听 SSH 保活失败：{error}")); }
                    break;
                }
            }
        }
    }
    let _ = session
        .cancel_tcpip_forward(&remote_host, remote_port as u32)
        .await;
    let _ = session
        .disconnect(Disconnect::ByApplication, "tunnel stopped", "en")
        .await;
}

pub async fn accept_loop(established: Established, mut stop: tokio::sync::oneshot::Receiver<()>) {
    loop {
        tokio::select! {
            _ = &mut stop => break,
            accepted = established.listener.accept() => {
                match accepted {
                    Ok((tcp, _peer)) => {
                        let url = established.url.clone();
                        let private_key = established.private_key.clone();
                        let report = established.report.clone();
                        tokio::spawn(async move {
                            forward_reported(tcp, &url, &private_key, report).await;
                        });
                    }
                    Err(error) => {
                        let message = format!("监听连接失败：{error}");
                        if let Some(report) = &established.report { report("error", &message); }
                        else { crate::logging::error(&message); }
                        break;
                    }
                }
            }
        }
    }
}

async fn forward_reported<S>(tcp: S, url: &str, key: &PrivateKey, report: Option<Reporter>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if let Some(report) = &report {
        report("connecting", "正在建立 WSS 连接…");
    }
    match forward(tcp, url, key, &report).await {
        Ok(()) => {
            if let Some(report) = &report {
                report("closed", "最近一次转发连接已结束，监听仍可接受新连接");
            }
        }
        Err(error) => {
            if let Some(report) = &report {
                report("error", &error);
            } else {
                crate::logging::error(&error);
            }
        }
    }
}

async fn forward<S>(
    tcp: S,
    url: &str,
    private_key: &PrivateKey,
    report: &Option<Reporter>,
) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut request = url
        .into_client_request()
        .map_err(|e| format!("Invalid WebSocket URL: {e}"))?;
    add_signature_headers(request.headers_mut(), private_key)?;
    let (socket, response) = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .map_err(|_| "WSS 握手超时（20 秒），请检查地址和网络".to_string())?
    .map_err(handshake_error)?;
    if response.status() != 101 {
        return Err(format!(
            "WebSocket handshake returned HTTP {}",
            response.status()
        ));
    }

    if let Some(report) = report {
        report(
            "connected",
            "WSS 握手成功，正在转发；尚未确认目标 SSH 是否响应",
        );
    }
    let mut received = false;
    let (mut ws_out, mut ws_in) = socket.split();
    let (mut tcp_in, mut tcp_out) = tokio::io::split(tcp);
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        tokio::select! {
            read = tcp_in.read(&mut buffer) => match read {
                Ok(0) => break,
                Err(error) => return Err(format!("TCP 读取失败：{error}")),
                Ok(size) => ws_out.send(Message::Binary(buffer[..size].to_vec().into())).await
                    .map_err(|e| e.to_string())?,
            },
            message = ws_in.next() => match message {
                Some(Ok(Message::Binary(data))) => {
                    tcp_out.write_all(&data).await.map_err(|e| format!("TCP 写入失败：{e}"))?;
                    if !data.is_empty() && !received {
                        received = true;
                        if let Some(report) = report { report("forwarding", "转发正常，已收到远端目标数据"); }
                    }
                },
                Some(Ok(Message::Ping(data))) => ws_out.send(Message::Pong(data)).await.map_err(|e| e.to_string())?,
                Some(Ok(Message::Pong(_))) => {},
                Some(Err(error)) => return Err(format!("WSS 连接中断：{error}")),
                Some(Ok(Message::Close(_))) | None => {
                    if !received { return Err("WSS 已关闭且未收到目标 SSH 数据，请检查服务端隧道日志及 127.0.0.1:22".into()); }
                    break;
                },
                Some(Ok(_)) => return Err("WSS 返回了不支持的消息类型".into()),
            }
        }
    }
    let _ = tcp_out.shutdown().await;
    let _ = ws_out.send(Message::Close(None)).await;
    Ok(())
}

fn handshake_error(error: tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error;
    match error {
        Error::Http(response) => {
            let detail = response
                .body()
                .as_deref()
                .and_then(handshake_response_detail);
            let hint = match response.status().as_u16() {
                401 | 403 => {
                    "请检查服务端授权公钥、所选证书及 Windows 与服务器的时间（签名允许相差 60 秒）"
                }
                404 => {
                    "请检查 WSS 路径 /update/api/tunnel/ws 或 /api/tunnel/ws，以及服务端是否启用隧道"
                }
                409 => "服务端隧道连接数已达上限，请关闭多余连接后重试",
                502 | 503 | 504 => "请检查反向代理是否能连接 cangling-update 服务",
                _ => "请检查服务端和反向代理日志",
            };
            match detail {
                Some(detail) => format!(
                    "WSS 握手失败：HTTP {}；服务端：{detail}；{hint}",
                    response.status().as_u16()
                ),
                None => format!(
                    "WSS 握手失败：HTTP {}；服务端未返回可读取的错误详情；{hint}",
                    response.status().as_u16()
                ),
            }
        }
        Error::Tls(error) => {
            format!(
                "WSS TLS 证书验证失败：{error}；请检查服务器证书链、域名及有效期（当前 WSS 使用内置公共根证书）"
            )
        }
        other if other.to_string().contains("InvalidContentType") => format!(
            "WSS TLS 握手失败：收到非 TLS 数据（InvalidContentType）；请核对 URL 协议和端口：普通 HTTP 5400/7800 使用 ws://，HTTPS 7443 使用 wss://，不能连接 SSH 22 端口。原始错误：{other}"
        ),
        other => format!("WSS 握手失败：{other}"),
    }
}

// Tungstenite includes any response body received with the handshake headers.
// Accept structured error text only, not proxy HTML or headers containing secrets.
fn handshake_response_detail(body: &[u8]) -> Option<String> {
    if body.len() > 16 * 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let text = value
        .get("error")
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("message").and_then(serde_json::Value::as_str))?;
    let text = text
        .chars()
        .filter(|c| !c.is_control())
        .take(500)
        .collect::<String>();
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn add_signature_headers(
    headers: &mut tokio_tungstenite::tungstenite::http::HeaderMap,
    private_key: &PrivateKey,
) -> Result<(), String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    let nonce = uuid::Uuid::new_v4().to_string();
    let key_id = private_key.fingerprint(HashAlg::Sha256).to_string();
    let message =
        format!("{SIGNATURE_DOMAIN}\nGET\n/api/tunnel/ws\n{key_id}\n{timestamp}\n{nonce}");
    let signature: Signature = private_key
        .try_sign(message.as_bytes())
        .map_err(|e| format!("Failed to sign WebSocket request: {e}"))?;
    insert(headers, KEY_HEADER, &key_id)?;
    insert(headers, TIME_HEADER, &timestamp)?;
    insert(headers, NONCE_HEADER, &nonce)?;
    insert(
        headers,
        SIGNATURE_HEADER,
        &BASE64.encode(signature.as_bytes()),
    )?;
    Ok(())
}

fn insert(
    headers: &mut tokio_tungstenite::tungstenite::http::HeaderMap,
    name: &'static str,
    value: &str,
) -> Result<(), String> {
    headers.insert(
        name,
        HeaderValue::from_str(value).map_err(|e| format!("Invalid tunnel auth header: {e}"))?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    async fn run_mock(close_after_upgrade: bool) -> Vec<(String, String)> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut tcp, _) = listener.accept().await.unwrap();
            if close_after_upgrade {
                let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                ws.close(None).await.unwrap();
            } else {
                let mut request = vec![0; 8192];
                let _ = tcp.read(&mut request).await.unwrap();
                tcp.write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            }
        });
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let (tcp, _peer) = tokio::io::duplex(1024);
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = events.clone();
        let reporter: Reporter = Arc::new(move |phase, message| {
            captured
                .lock()
                .unwrap()
                .push((phase.to_string(), message.to_string()))
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            forward_reported(
                tcp,
                &format!("ws://{address}/api/tunnel/ws"),
                &key,
                Some(reporter),
            ),
        )
        .await
        .unwrap();
        server.await.unwrap();
        Arc::try_unwrap(events).unwrap().into_inner().unwrap()
    }
    #[test]
    fn includes_specific_server_authentication_reason() {
        for reason in ["隧道公钥未授权", "隧道签名已过期", "隧道签名验证失败"]
        {
            let response = tokio_tungstenite::tungstenite::http::Response::builder()
                .status(401)
                .body(Some(
                    serde_json::to_vec(&serde_json::json!({"error": reason})).unwrap(),
                ))
                .unwrap();
            let text = handshake_error(tokio_tungstenite::tungstenite::Error::Http(Box::new(
                response,
            )));
            assert!(text.contains(reason));
            assert!(text.contains("401"));
        }
    }

    #[test]
    fn handles_absent_partial_and_proxy_error_bodies() {
        for body in [b"".as_slice(), b"{\"error\":", b"<html>proxy login</html>"] {
            assert!(handshake_response_detail(body).is_none());
        }
        let response = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(401)
            .body(None)
            .unwrap();
        assert!(
            handshake_error(tokio_tungstenite::tungstenite::Error::Http(Box::new(
                response
            )))
            .contains("未返回")
        );
    }

    #[tokio::test]
    async fn reports_auth_failure_to_tunnel_panel() {
        let events = run_mock(false).await;
        assert_eq!(events.last().unwrap().0, "error");
        assert!(events.last().unwrap().1.contains("401"));
        assert!(!events.iter().any(|(phase, _)| phase == "connected"));
    }
    #[tokio::test]
    async fn reports_target_close_after_successful_upgrade() {
        let events = run_mock(true).await;
        assert!(events.iter().any(|(phase, _)| phase == "connected"));
        assert_eq!(events.last().unwrap().0, "error");
        assert!(events.last().unwrap().1.contains("SSH"));
    }
}
