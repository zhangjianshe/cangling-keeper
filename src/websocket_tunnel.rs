use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use futures_util::{SinkExt, StreamExt};
use russh::keys::ssh_key::{HashAlg, Signature};
use russh::keys::{Algorithm, PrivateKey};
use signature::Signer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

use crate::tunnel::Tunnel;

const SIGNATURE_DOMAIN: &str = "cangling-tunnel-v1";
const KEY_HEADER: &str = "x-cangling-tunnel-key";
const TIME_HEADER: &str = "x-cangling-tunnel-time";
const NONCE_HEADER: &str = "x-cangling-tunnel-nonce";
const SIGNATURE_HEADER: &str = "x-cangling-tunnel-signature";

pub struct Established {
    listener: TcpListener,
    url: String,
    private_key: Arc<PrivateKey>,
}

pub async fn establish(tunnel: &Tunnel, private_key_path: &str) -> Result<Established, String> {
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
    })
}

pub async fn accept_loop(established: Established, mut stop: tokio::sync::oneshot::Receiver<()>) {
    loop {
        tokio::select! {
            _ = &mut stop => break,
            accepted = established.listener.accept() => {
                match accepted {
                    Ok((tcp, peer)) => {
                        let url = established.url.clone();
                        let private_key = established.private_key.clone();
                        tokio::spawn(async move {
                            if let Err(error) = forward(tcp, &url, &private_key).await {
                                eprintln!("WebSocket tunnel connection failed ({peer}): {error}");
                            }
                        });
                    }
                    Err(error) => {
                        eprintln!("WebSocket tunnel accept failed: {error}");
                        break;
                    }
                }
            }
        }
    }
}

async fn forward(tcp: TcpStream, url: &str, private_key: &PrivateKey) -> Result<(), String> {
    let mut request = url
        .into_client_request()
        .map_err(|e| format!("Invalid WebSocket URL: {e}"))?;
    add_signature_headers(request.headers_mut(), private_key)?;
    let (socket, response) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("WebSocket handshake failed: {e}"))?;
    if response.status() != 101 {
        return Err(format!(
            "WebSocket handshake returned HTTP {}",
            response.status()
        ));
    }

    let (mut ws_out, mut ws_in) = socket.split();
    let (mut tcp_in, mut tcp_out) = tcp.into_split();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        tokio::select! {
            read = tcp_in.read(&mut buffer) => match read {
                Ok(0) | Err(_) => break,
                Ok(size) => ws_out.send(Message::Binary(buffer[..size].to_vec().into())).await
                    .map_err(|e| e.to_string())?,
            },
            message = ws_in.next() => match message {
                Some(Ok(Message::Binary(data))) => tcp_out.write_all(&data).await.map_err(|e| e.to_string())?,
                Some(Ok(Message::Ping(data))) => ws_out.send(Message::Pong(data)).await.map_err(|e| e.to_string())?,
                Some(Ok(Message::Pong(_))) => {},
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => break,
            }
        }
    }
    let _ = tcp_out.shutdown().await;
    let _ = ws_out.send(Message::Close(None)).await;
    Ok(())
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
