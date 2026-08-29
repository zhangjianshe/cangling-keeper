use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use russh::{ChannelMsg, Disconnect, client};
use serde::Serialize;
use tauri::Emitter;
use tokio::net::{TcpListener, TcpStream};

use crate::host::Host;
use crate::tunnel::Tunnel;

/// Result of running a remote command.
#[derive(Debug, Clone, Serialize)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_status: i32,
}

/// Authentication resolved to a concrete credential (password or key path).
#[derive(Debug, Clone)]
pub enum ResolvedAuth {
    Password(String),
    Key(String),
}

pub(crate) struct SshClient;

impl client::Handler for SshClient {
    type Error = russh::Error;

    // TODO: implement host-key verification (trust-on-first-use) instead of
    // accepting every server key. Acceptable for a local tool, not for
    // untrusted networks.
    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

async fn connect_with<H>(host: &str, port: u16, handler: H) -> Result<client::Handle<H>, String>
where
    H: client::Handler + Send + 'static,
    H::Error: From<russh::Error> + Send + std::fmt::Debug,
{
    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| "Connection timed out after 10s".to_string())?
        .map_err(|e| format!("TCP connection failed: {e}"))?;

    let config = Arc::new(client::Config::default());
    client::connect_stream(config, tcp, handler)
        .await
        .map_err(|e| format!("SSH handshake failed: {e:?}"))
}

pub(crate) async fn connect(host: &str, port: u16) -> Result<client::Handle<SshClient>, String> {
    connect_with(host, port, SshClient).await
}

pub(crate) async fn authenticate<H>(
    session: &mut client::Handle<H>,
    username: &str,
    auth: &ResolvedAuth,
) -> Result<(), String>
where
    H: client::Handler + Send,
    H::Error: From<russh::Error> + Send + std::fmt::Debug,
{
    let result = match auth {
        ResolvedAuth::Password(password) => session
            .authenticate_password(username, password)
            .await
            .map_err(|e| format!("Authentication error: {e:?}"))?,
        ResolvedAuth::Key(path) => {
            let key = russh::keys::load_secret_key(path, None)
                .map_err(|e| format!("Failed to load private key '{path}': {e}"))?;
            let rsa_hash = session
                .best_supported_rsa_hash()
                .await
                .map_err(|e| format!("Authentication error: {e:?}"))?
                .flatten();
            session
                .authenticate_publickey(
                    username,
                    russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), rsa_hash),
                )
                .await
                .map_err(|e| format!("Authentication error: {e:?}"))?
        }
    };

    if !result.success() {
        return Err("Authentication rejected — check username/password/key".into());
    }
    Ok(())
}

pub async fn execute(
    host: &Host,
    command: &str,
    auth: &ResolvedAuth,
) -> Result<ExecOutput, String> {
    if command.trim().is_empty() {
        return Err("Command is empty".into());
    }

    let mut session = connect(&host.hostname, host.port).await?;
    authenticate(&mut session, &host.username, auth).await?;

    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("Failed to open session: {e}"))?;

    channel
        .exec(true, command)
        .await
        .map_err(|e| format!("Failed to execute command: {e}"))?;

    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_status: i32 = -1;

    loop {
        let Some(msg) = channel.wait().await else {
            break;
        };
        match msg {
            ChannelMsg::Data { data } => {
                stdout.push_str(&String::from_utf8_lossy(&data));
            }
            ChannelMsg::ExtendedData { data, ext } if ext == 1 => {
                stderr.push_str(&String::from_utf8_lossy(&data));
            }
            ChannelMsg::ExitStatus { exit_status: code } => {
                exit_status = code as i32;
            }
            _ => {}
        }
    }

    Ok(ExecOutput {
        stdout,
        stderr,
        exit_status,
    })
}

// ---- local port-forwarding tunnels -----------------------------------------

pub struct EstablishedTunnel {
    session: client::Handle<SshClient>,
    listener: TcpListener,
}

pub async fn establish_tunnel(
    tunnel: &Tunnel,
    auth: &ResolvedAuth,
) -> Result<EstablishedTunnel, String> {
    let mut session = connect(&tunnel.ssh_host, tunnel.ssh_port).await?;
    authenticate(&mut session, &tunnel.username, auth).await?;

    let listener = TcpListener::bind(("127.0.0.1", tunnel.local_port))
        .await
        .map_err(|e| format!("Failed to bind local port {}: {e}", tunnel.local_port))?;

    Ok(EstablishedTunnel { session, listener })
}

/// Open an SSH session and bind a local port for a later forward.
/// Prefers `prefer_port` (the remote cangling-update listen port) so the
/// opened URL uses the same port; falls back to an ephemeral port if busy.
pub async fn establish_host_local_forward(
    host: &Host,
    auth: &ResolvedAuth,
    prefer_port: u16,
) -> Result<(EstablishedTunnel, u16), String> {
    let mut session = connect(&host.hostname, host.port).await?;
    authenticate(&mut session, &host.username, auth).await?;

    let listener = if prefer_port > 0 {
        match TcpListener::bind(("127.0.0.1", prefer_port)).await {
            Ok(l) => l,
            Err(_) => TcpListener::bind(("127.0.0.1", 0))
                .await
                .map_err(|e| format!("无法绑定本机控制台端口: {e}"))?,
        }
    } else {
        TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| format!("无法绑定本机控制台端口: {e}"))?
    };
    let local_port = listener
        .local_addr()
        .map_err(|e| format!("无法读取本机控制台端口: {e}"))?
        .port();
    if local_port == 0 {
        return Err("本机控制台端口分配失败".into());
    }

    Ok((EstablishedTunnel { session, listener }, local_port))
}

pub async fn accept_loop(
    established: EstablishedTunnel,
    remote_host: String,
    remote_port: u16,
    mut cancel: tokio::sync::oneshot::Receiver<()>,
) {
    let EstablishedTunnel { session, listener } = established;

    loop {
        tokio::select! {
            _ = &mut cancel => break,
            accepted = listener.accept() => {
                match accepted {
                    Ok((socket, _)) => {
                        match session
                            .channel_open_direct_tcpip(
                                remote_host.clone(),
                                remote_port as u32,
                                "127.0.0.1",
                                0,
                            )
                            .await
                        {
                            Ok(channel) => {
                                tauri::async_runtime::spawn(async move {
                                    let _ = pipe(socket, channel).await;
                                });
                            }
                            Err(_) => {
                                drop(socket);
                            }
                        }
                    }
                    Err(_) => {
                        // Accept error; keep serving.
                    }
                }
            }
        }
    }
}

async fn pipe(mut socket: TcpStream, channel: russh::Channel<russh::client::Msg>) {
    let mut stream = channel.into_stream();
    let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
}

// ---- reverse proxy inject (`ssh -N -R 7890:local_proxy`) -------------------

struct ReverseSshClient {
    local_host: String,
    local_port: u16,
}

impl client::Handler for ReverseSshClient {
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
        let local_host = self.local_host.clone();
        let local_port = self.local_port;
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            match TcpStream::connect((local_host.as_str(), local_port)).await {
                Ok(mut tcp) => {
                    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut stream).await;
                }
                Err(_) => {}
            }
        });
        Ok(())
    }
}

pub struct InjectedProxy {
    session: client::Handle<ReverseSshClient>,
    remote_port: u16,
}

/// Open `ssh -N -R remote_port:local_host:local_port` (does not block).
pub async fn establish_inject(
    host: &Host,
    auth: &ResolvedAuth,
    local_host: &str,
    local_port: u16,
    remote_port: u16,
) -> Result<InjectedProxy, String> {
    let handler = ReverseSshClient {
        local_host: local_host.to_string(),
        local_port,
    };
    let mut session = connect_with(&host.hostname, host.port, handler).await?;
    authenticate(&mut session, &host.username, auth).await?;

    if session
        .tcpip_forward("localhost", remote_port as u32)
        .await
        .is_err()
    {
        session
            .tcpip_forward("127.0.0.1", remote_port as u32)
            .await
            .map_err(|e| {
                format!("Remote forward -R {remote_port} rejected (is the port in use?): {e:?}")
            })?;
    }

    Ok(InjectedProxy {
        session,
        remote_port,
    })
}

/// Hold the reverse forward until `cancel` or the SSH session dies.
pub async fn hold_inject(injected: InjectedProxy, mut cancel: tokio::sync::oneshot::Receiver<()>) {
    let InjectedProxy {
        session,
        remote_port,
    } = injected;

    loop {
        tokio::select! {
            _ = &mut cancel => break,
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                if session.is_closed() {
                    break;
                }
                let _ = session.send_keepalive(false).await;
            }
        }
    }

    let _ = session
        .cancel_tcpip_forward("localhost", remote_port as u32)
        .await;
    let _ = session
        .cancel_tcpip_forward("127.0.0.1", remote_port as u32)
        .await;
    let _ = session
        .disconnect(Disconnect::ByApplication, "inject stopped", "en")
        .await;
}

// ---- key generation --------------------------------------------------------

/// Generate an ed25519 key pair, writing the private key to `private_path`
/// (mode 0600 on Unix) and the public key to `<private_path>.pub`.
/// Returns the OpenSSH public key string.
pub fn generate_keypair(private_path: &Path) -> Result<String, String> {
    if let Some(parent) = private_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let mut rng = rand::rng();
    let key = PrivateKey::random(&mut rng, Algorithm::Ed25519).map_err(|e| e.to_string())?;

    key.write_openssh_file(private_path, LineEnding::LF)
        .map_err(|e| e.to_string())?;

    let public = key.public_key().to_openssh().map_err(|e| e.to_string())?;
    let public_path = private_path.with_extension("pub");
    std::fs::write(&public_path, public.trim_end()).map_err(|e| e.to_string())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(private_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }

    Ok(public.trim().to_string())
}

// ---- interactive terminal (PTY) --------------------------------------------

pub struct PtySession {
    session: client::Handle<SshClient>,
    channel: russh::Channel<russh::client::Msg>,
}

/// Open an SSH session with an interactive shell (PTY) for `host`.
pub async fn open_pty(
    host: &Host,
    auth: &ResolvedAuth,
    cols: u32,
    rows: u32,
) -> Result<PtySession, String> {
    let mut session = connect(&host.hostname, host.port).await?;
    authenticate(&mut session, &host.username, auth).await?;

    let channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("Failed to open session: {e}"))?;

    channel
        .request_pty(true, "xterm-256color", cols, rows, 0, 0, &[])
        .await
        .map_err(|e| format!("Failed to allocate PTY: {e}"))?;

    channel
        .request_shell(true)
        .await
        .map_err(|e| format!("Failed to start shell: {e}"))?;

    Ok(PtySession { session, channel })
}

/// Messages sent from the frontend to an active terminal session.
pub enum TerminalMsg {
    Input(Vec<u8>),
    Resize(u32, u32),
}

#[derive(Clone, Serialize)]
pub struct TerminalData {
    pub id: String,
    pub data: String,
}

/// Run the terminal session until it is cancelled or the remote side closes.
pub async fn terminal_loop(
    pty: PtySession,
    mut input_rx: tokio::sync::mpsc::UnboundedReceiver<TerminalMsg>,
    mut cancel: tokio::sync::oneshot::Receiver<()>,
    app: tauri::AppHandle,
    term_id: String,
) {
    let (mut read_half, write_half) = pty.channel.split();
    let _session = pty.session; // keep the SSH connection alive

    loop {
        tokio::select! {
            _ = &mut cancel => break,
            msg = read_half.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        let _ = app.emit("terminal-data", TerminalData {
                            id: term_id.clone(),
                            data: String::from_utf8_lossy(&data).into_owned(),
                        });
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        let _ = app.emit("terminal-data", TerminalData {
                            id: term_id.clone(),
                            data: String::from_utf8_lossy(&data).into_owned(),
                        });
                    }
                    Some(ChannelMsg::ExitStatus { .. })
                    | Some(ChannelMsg::Close)
                    | Some(ChannelMsg::Eof)
                    | None => break,
                    _ => {}
                }
            }
            cmd = input_rx.recv() => {
                match cmd {
                    Some(TerminalMsg::Input(data)) => {
                        let _ = write_half.data_bytes(data).await;
                    }
                    Some(TerminalMsg::Resize(cols, rows)) => {
                        let _ = write_half.window_change(cols, rows, 0, 0).await;
                    }
                    None => break,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_loadable_ed25519_key() {
        let path = std::path::PathBuf::from("config/keys/__test_key__");
        let public = generate_keypair(&path).unwrap();
        assert!(public.starts_with("ssh-ed25519 "));

        let key = russh::keys::load_secret_key(&path, None).unwrap();
        assert_eq!(key.algorithm(), Algorithm::Ed25519);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("pub"));
    }
}
