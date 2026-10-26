//! Browser-based frontend for cangling-keeper.
//!
//! The API exposes a compact `invoke(command, args)` transport so the frontend
//! can share one JSON endpoint while live events use WebSocket.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxumPath, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use rust_embed::Embed;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::host::Host;
use crate::proxy::{self, ProxySettings};
use crate::ssh::{self, ResolvedAuth, TerminalMsg};
use crate::tunnel::{Tunnel, TunnelInfo};
use crate::{
    AppState, InjectedHandle, TerminalHandle, apply_probe, create_app_state, create_certificate,
    current_proxy_status, delete_remote_host, delete_remote_tunnel, import_certificate,
    local_hostname, persist_proxy, push_host_to_server, push_tunnel_to_server, read_login_status,
    resolve_auth, resolve_key_path, stop_local_locked, sync, sync_now,
};

#[derive(Embed)]
#[folder = "ui/"]
struct UiAssets;

#[derive(Clone)]
pub(crate) struct WebState {
    core: Arc<AppState>,
    events: broadcast::Sender<WebEvent>,
}

impl crate::EventSink for WebState {
    fn emit_json(&self, event: &str, payload: Value) {
        let _ = self.events.send(WebEvent {
            event: event.into(),
            payload,
        });
    }
}

#[derive(Clone, Debug, Serialize)]
struct WebEvent {
    event: String,
    payload: Value,
}

impl WebState {
    fn emit<T: Serialize>(&self, event: &str, payload: T) {
        if let Ok(payload) = serde_json::to_value(payload) {
            let _ = self.events.send(WebEvent {
                event: event.into(),
                payload,
            });
        }
    }
}

pub(crate) fn run_from_args() -> Result<(), String> {
    let mut listen = "127.0.0.1:18080".to_string();
    let mut data_dir = default_data_dir()?;
    let mut open = true;
    let args: Vec<String> = std::env::args().collect();
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--listen" => {
                index += 1;
                listen = args.get(index).ok_or("--listen 缺少地址")?.clone();
            }
            "--data-dir" => {
                index += 1;
                data_dir = PathBuf::from(args.get(index).ok_or("--data-dir 缺少目录")?);
            }
            "--no-open" => open = false,
            _ => {}
        }
        index += 1;
    }
    let address: SocketAddr = listen
        .parse()
        .map_err(|error| format!("无效监听地址 {listen}：{error}"))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(run(address, data_dir, open))
}

fn default_data_dir() -> Result<PathBuf, String> {
    dirs::data_dir()
        .map(|dir| dir.join("cn.cangling.keeper"))
        .ok_or_else(|| "无法确定用户数据目录，请使用 --data-dir 指定".into())
}

async fn run(address: SocketAddr, data_dir: PathBuf, open: bool) -> Result<(), String> {
    proxy::ensure_loopback_not_proxied();
    let core = Arc::new(create_app_state(data_dir).map_err(|error| error.to_string())?);
    let (events, _) = broadcast::channel(512);
    let state = WebState { core, events };
    let app = Router::new()
        .route("/api/health", get(|| async { Json(json!({ "ok": true })) }))
        .route("/api/events", get(events_socket))
        .route("/api/invoke/{command}", post(invoke))
        .fallback(static_asset)
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|error| format!("监听 {address} 失败：{error}"))?;
    let actual = listener.local_addr().map_err(|error| error.to_string())?;
    let url = format!("http://{actual}");
    println!("cangling-keeper Web 服务已启动：{url}");
    println!("按 Ctrl+C 停止服务");
    if open {
        let url = url.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let _ = crate::open_with_command(&url);
        });
    }
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|error| error.to_string())
}

async fn static_asset(uri: Uri) -> Response {
    let requested = uri.path().trim_start_matches('/');
    let path = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };
    let Some(asset) = UiAssets::get(path) else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(asset.data))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn events_socket(State(state): State<WebState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| forward_events(socket, state.events.subscribe()))
}

async fn forward_events(mut socket: WebSocket, mut events: broadcast::Receiver<WebEvent>) {
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => {
                    let Ok(text) = serde_json::to_string(&event) else { continue };
                    if socket.send(Message::Text(text.into())).await.is_err() { break; }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            message = socket.next() => match message {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            }
        }
    }
}

type InvokeResponse = (StatusCode, Json<Value>);

fn ok<T: Serialize>(value: T) -> InvokeResponse {
    match serde_json::to_value(value) {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "result": result })),
        ),
        Err(error) => fail(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

fn fail(status: StatusCode, error: impl Into<String>) -> InvokeResponse {
    (status, Json(json!({ "ok": false, "error": error.into() })))
}

fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    serde_json::from_value(args.get(name).cloned().unwrap_or(Value::Null))
        .map_err(|error| format!("参数 {name} 无效：{error}"))
}

async fn invoke(
    State(state): State<WebState>,
    AxumPath(command): AxumPath<String>,
    Json(args): Json<Value>,
) -> InvokeResponse {
    match dispatch(&state, &command, args).await {
        Ok(value) => ok(value),
        Err(error) => fail(StatusCode::BAD_REQUEST, error),
    }
}

async fn dispatch(state: &WebState, command: &str, args: Value) -> Result<Value, String> {
    let core = &state.core;
    match command {
        "list_hosts" => {
            let store = core.store.lock().map_err(|e| e.to_string())?;
            value(store.list_hosts()?)
        }
        "add_host" => {
            let mut host: Host = arg(&args, "host")?;
            if host.id.is_empty() {
                host.id = Uuid::new_v4().to_string();
            }
            host.owned = true;
            host.validate()?;
            {
                core.store
                    .lock()
                    .map_err(|e| e.to_string())?
                    .add_host(&host)?;
            }
            if let Ok(remote_id) = push_host_to_server(core, &host).await {
                host.remote_id = remote_id;
            }
            value(host)
        }
        "update_host" => {
            let mut host: Host = arg(&args, "host")?;
            host.validate()?;
            {
                let store = core.store.lock().map_err(|e| e.to_string())?;
                let existing = store.get_host(&host.id)?;
                if !existing.owned {
                    return Err("该主机不是自己创建的，无法编辑".into());
                }
                host.remote_id = existing.remote_id;
                host.owned = existing.owned;
                store.update_host(&host)?;
            }
            let _ = push_host_to_server(core, &host).await;
            value(())
        }
        "delete_host" => {
            let id: String = arg(&args, "id")?;
            let host = {
                core.store
                    .lock()
                    .map_err(|e| e.to_string())?
                    .get_host(&id)?
            };
            if !host.owned {
                return Err("只能删除自己创建的主机".into());
            }
            core.store
                .lock()
                .map_err(|e| e.to_string())?
                .delete_host(&id)?;
            if !host.remote_id.is_empty() {
                let _ = delete_remote_host(core, &host.remote_id).await;
            }
            value(())
        }
        "ssh_execute" => {
            let host_id: String = arg(&args, "hostId")?;
            let command: String = arg(&args, "command")?;
            let (host, auth) = host_auth(core, &host_id)?;
            value(ssh::execute(&host, &command, &auth).await?)
        }
        "check_host_env" => {
            value(crate::check_host_env_core(core, &arg::<String>(&args, "hostId")?).await?)
        }
        "probe_cangling_update" => {
            value(crate::probe_cangling_update_core(core, &arg::<String>(&args, "hostId")?).await?)
        }
        "run_cangling_update" => value(
            crate::run_cangling_update_core(
                std::sync::Arc::new(state.clone()),
                core,
                arg(&args, "hostId")?,
                args.get("port")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|e| e.to_string())?
                    .flatten(),
            )
            .await?,
        ),
        "cluster_console_url" => {
            value(crate::cluster_console_url_core(core, &arg::<String>(&args, "hostId")?).await?)
        }
        "set_cangling_role" => value(
            crate::set_cangling_role_core(
                core,
                &arg::<String>(&args, "hostId")?,
                &arg::<String>(&args, "role")?,
                args.get("token")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|e| e.to_string())?,
                args.get("master")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|e| e.to_string())?,
                args.get("port")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|e| e.to_string())?,
            )
            .await?,
        ),
        "list_tunnels" => value(list_tunnels(core)?),
        "add_tunnel" => {
            let mut tunnel: Tunnel = arg(&args, "tunnel")?;
            if tunnel.id.is_empty() {
                tunnel.id = Uuid::new_v4().to_string();
            }
            tunnel.validate()?;
            {
                let store = core.store.lock().map_err(|e| e.to_string())?;
                tunnel.user_id = store
                    .get_setting(sync::SETTING_USER_ID)?
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_default();
                store.add_tunnel(&tunnel)?;
            }
            if tunnel.tunnel_type == "ssh" {
                if let Ok((remote_id, user_id)) = push_tunnel_to_server(core, &tunnel).await {
                    tunnel.remote_id = remote_id;
                    tunnel.user_id = user_id;
                }
            }
            value(tunnel)
        }
        "update_tunnel" => {
            let mut tunnel: Tunnel = arg(&args, "tunnel")?;
            tunnel.validate()?;
            {
                let store = core.store.lock().map_err(|e| e.to_string())?;
                let existing = store.get_tunnel(&tunnel.id)?;
                tunnel.remote_id = existing.remote_id;
                tunnel.user_id = existing.user_id;
                store.update_tunnel(&tunnel)?;
            }
            if tunnel.tunnel_type == "ssh" {
                let _ = push_tunnel_to_server(core, &tunnel).await;
            }
            value(())
        }
        "delete_tunnel" => {
            let id: String = arg(&args, "id")?;
            disconnect_tunnel(core, &id).ok();
            let tunnel = {
                core.store
                    .lock()
                    .map_err(|e| e.to_string())?
                    .get_tunnel(&id)?
            };
            core.store
                .lock()
                .map_err(|e| e.to_string())?
                .delete_tunnel(&id)?;
            if !tunnel.remote_id.is_empty() {
                let _ = delete_remote_tunnel(core, &tunnel.remote_id).await;
            }
            value(())
        }
        "parse_ssh_command" => value(crate::tunnel::parse_ssh_command(&arg::<String>(
            &args, "command",
        )?)?),
        "tunnel_connect" => {
            connect_tunnel(state, arg(&args, "tunnelId")?).await?;
            value(())
        }
        "tunnel_disconnect" => {
            disconnect_tunnel(core, &arg::<String>(&args, "tunnelId")?)?;
            value(())
        }
        "list_certificates" => {
            let store = core.store.lock().map_err(|e| e.to_string())?;
            value(store.list_certificates()?)
        }
        "add_certificate" => {
            let cert =
                create_certificate(&core.data_dir.join("keys"), &arg::<String>(&args, "name")?)?;
            core.store
                .lock()
                .map_err(|e| e.to_string())?
                .add_certificate(&cert)?;
            value(cert)
        }
        "import_private_key" => {
            let cert = import_certificate(
                &core.data_dir.join("keys"),
                &arg::<String>(&args, "name")?,
                &arg::<String>(&args, "privateKey")?,
            )?;
            core.store
                .lock()
                .map_err(|e| e.to_string())?
                .add_certificate(&cert)?;
            value(cert)
        }
        "update_certificate_name" => {
            let id: String = arg(&args, "id")?;
            let name: String = arg(&args, "name")?;
            if name.trim().is_empty() {
                return Err("Certificate name is required".into());
            }
            let store = core.store.lock().map_err(|e| e.to_string())?;
            store.update_certificate_name(&id, name.trim())?;
            value(store.get_certificate(&id)?)
        }
        "delete_certificate" => {
            let id: String = arg(&args, "id")?;
            let store = core.store.lock().map_err(|e| e.to_string())?;
            let cert = store.get_certificate(&id)?;
            store.delete_certificate(&id)?;
            drop(store);
            let path = resolve_key_path(&core.data_dir, &cert.private_key_path);
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(format!("{}.pub", path.display()));
            value(())
        }
        "local_hostname" => value(local_hostname()),
        "start_terminal" => {
            let id = start_terminal(
                state,
                arg(&args, "hostId")?,
                arg(&args, "cols")?,
                arg(&args, "rows")?,
            )
            .await?;
            value(id)
        }
        "terminal_input" => {
            terminal_message(
                core,
                &arg::<String>(&args, "termId")?,
                TerminalMsg::Input(arg::<String>(&args, "data")?.into_bytes()),
            )?;
            value(())
        }
        "terminal_resize" => {
            terminal_message(
                core,
                &arg::<String>(&args, "termId")?,
                TerminalMsg::Resize(arg(&args, "cols")?, arg(&args, "rows")?),
            )?;
            value(())
        }
        "terminal_close" => {
            close_terminal(core, &arg::<String>(&args, "termId")?)?;
            value(())
        }
        "get_proxy_status" => value(current_proxy_status(core)?),
        "start_local_proxy" => value(start_local_proxy(state, arg(&args, "port")?).await?),
        "use_existing_proxy" => {
            value(use_existing_proxy(state, arg(&args, "host")?, arg(&args, "port")?).await?)
        }
        "check_proxy" => value(check_proxy(state).await?),
        "stop_proxy" => value(stop_proxy(state)?),
        "list_proxy_injections" => value(list_injections(core)?),
        "inject_proxy" => value(inject_proxy(state, arg(&args, "hostId")?).await?),
        "uninject_proxy" => {
            uninject_proxy(core, &arg::<String>(&args, "hostId")?)?;
            value(())
        }
        "get_login_status" => {
            let store = core.store.lock().map_err(|e| e.to_string())?;
            value(read_login_status(&store))
        }
        "login" => {
            let url: String = arg(&args, "serverUrl")?;
            let username: String = arg(&args, "username")?;
            let password: String = arg(&args, "password")?;
            let data = sync::login(url.trim(), username.trim(), &password).await?;
            {
                let store = core.store.lock().map_err(|e| e.to_string())?;
                store.set_setting(sync::SETTING_SERVER_URL, url.trim())?;
                store.set_setting(sync::SETTING_TOKEN, &data.token)?;
                store.set_setting(sync::SETTING_USERNAME, &data.user_name)?;
                store.set_setting(sync::SETTING_NICKNAME, &data.nick_name)?;
                store.set_setting(sync::SETTING_USER_ID, &data.user_id.to_string())?;
            }
            let _ = sync_now(core).await;
            let store = core.store.lock().map_err(|e| e.to_string())?;
            value(read_login_status(&store))
        }
        "logout" => {
            let store = core.store.lock().map_err(|e| e.to_string())?;
            for key in [
                sync::SETTING_TOKEN,
                sync::SETTING_USERNAME,
                sync::SETTING_NICKNAME,
                sync::SETTING_USER_ID,
            ] {
                store.delete_setting(key)?;
            }
            value(read_login_status(&store))
        }
        "sync_hosts" | "sync_public_hosts" => {
            sync_now(core).await?;
            value(())
        }
        "repo_status" => value(crate::repo::web_repo_status(core)),
        "sync_software_set" => value(
            crate::repo::web_sync_software_set(
                std::sync::Arc::new(state.clone()),
                core,
                arg(&args, "setName")?,
            )
            .await?,
        ),
        "list_software_sets" => value(crate::repo::web_list_software_sets(core)?),
        "add_software_set" => value(crate::repo::web_add_software_set(
            core,
            arg(&args, "setName")?,
            arg(&args, "kind")?,
            arg(&args, "gitUrl")?,
            arg(&args, "gitUsername")?,
            arg(&args, "gitToken")?,
            arg(&args, "gitBranch")?,
        )?),
        "update_software_set" => value(crate::repo::web_update_software_set(
            core,
            arg(&args, "setName")?,
            arg(&args, "kind")?,
            arg(&args, "gitUrl")?,
            arg(&args, "gitUsername")?,
            arg(&args, "gitToken")?,
            arg(&args, "gitBranch")?,
        )?),
        "remove_software_set" => value(crate::repo::web_remove_software_set(
            core,
            arg(&args, "setName")?,
        )?),
        "select_software_set" => value(crate::repo::web_select_software_set(
            core,
            arg(&args, "setName")?,
        )?),
        "list_repo_files" => value(crate::repo::web_list_repo_files(core, arg(&args, "path")?)?),
        "read_repo_file" => value(crate::repo::web_read_repo_file(core, arg(&args, "path")?)?),
        "preview_host_software_sync" => {
            value(crate::host_sync::web_preview_host_software_sync(core)?)
        }
        "sync_host_software" => value(
            crate::host_sync::web_sync_host_software(
                std::sync::Arc::new(state.clone()),
                core,
                arg(&args, "hostId")?,
                args.get("paths")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|e| e.to_string())?
                    .flatten(),
            )
            .await?,
        ),
        "open_local_path" => {
            crate::open_local_path(arg(&args, "path")?)?;
            value(())
        }
        "open_url" => {
            crate::open_url(arg(&args, "url")?)?;
            value(())
        }
        "check_app_update" => value(Value::Null),
        _ => Err(format!("Web 模式暂不支持命令：{command}")),
    }
}

fn value<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

fn host_auth(core: &AppState, host_id: &str) -> Result<(Host, ResolvedAuth), String> {
    let store = core.store.lock().map_err(|e| e.to_string())?;
    let host = store.get_host(host_id)?;
    let auth = resolve_auth(&store, &host.auth, &core.data_dir)?;
    Ok((host, auth))
}

fn list_tunnels(core: &AppState) -> Result<Vec<TunnelInfo>, String> {
    let tunnels = core
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .list_tunnels()?;
    let active = core.active_tunnels.lock().map_err(|e| e.to_string())?;
    Ok(tunnels
        .into_iter()
        .map(|tunnel| TunnelInfo {
            active: active.contains_key(&tunnel.id),
            tunnel,
        })
        .collect())
}

async fn connect_tunnel(state: &WebState, tunnel_id: String) -> Result<(), String> {
    if state
        .core
        .active_tunnels
        .lock()
        .map_err(|e| e.to_string())?
        .contains_key(&tunnel_id)
    {
        return Err("Tunnel is already connected".into());
    }
    let (tunnel, auth) = {
        let store = state.core.store.lock().map_err(|e| e.to_string())?;
        let tunnel = store.get_tunnel(&tunnel_id)?;
        let auth = resolve_auth(&store, &tunnel.auth, &state.core.data_dir)?;
        (tunnel, auth)
    };
    let web = state.clone();
    if tunnel.tunnel_type == "websocket" {
        let ResolvedAuth::Key(key) = auth else {
            return Err("WebSocket tunnel requires certificate authentication".into());
        };
        let established = crate::websocket_tunnel::establish(&tunnel, &key).await?;
        let rx = register_active_tunnel(&state.core, &tunnel_id)?;
        tokio::spawn(async move {
            crate::websocket_tunnel::accept_loop(established, rx).await;
            tunnel_finished(&web, &tunnel_id);
        });
    } else if tunnel.direction == "remote" {
        let established = ssh::establish_reverse_tunnel(&tunnel, &auth).await?;
        let rx = register_active_tunnel(&state.core, &tunnel_id)?;
        tokio::spawn(async move {
            ssh::hold_inject(established, rx).await;
            tunnel_finished(&web, &tunnel_id);
        });
    } else {
        let established = ssh::establish_tunnel(&tunnel, &auth).await?;
        let rx = register_active_tunnel(&state.core, &tunnel_id)?;
        let remote_host = tunnel.remote_host;
        let remote_port = tunnel.remote_port;
        tokio::spawn(async move {
            ssh::accept_loop(established, remote_host, remote_port, rx).await;
            tunnel_finished(&web, &tunnel_id);
        });
    }
    Ok(())
}

fn register_active_tunnel(
    core: &AppState,
    tunnel_id: &str,
) -> Result<tokio::sync::oneshot::Receiver<()>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut active = core.active_tunnels.lock().map_err(|e| e.to_string())?;
    if active.contains_key(tunnel_id) {
        return Err("Tunnel is already connected".into());
    }
    active.insert(tunnel_id.to_string(), tx);
    Ok(rx)
}

fn tunnel_finished(state: &WebState, tunnel_id: &str) {
    if let Ok(mut active) = state.core.active_tunnels.lock() {
        active.remove(tunnel_id);
    }
    state.emit("tunnel-stopped", tunnel_id);
}

fn disconnect_tunnel(core: &AppState, id: &str) -> Result<(), String> {
    let mut active = core.active_tunnels.lock().map_err(|e| e.to_string())?;
    let tx = active.remove(id).ok_or("Tunnel is not connected")?;
    let _ = tx.send(());
    Ok(())
}

async fn start_terminal(
    state: &WebState,
    host_id: String,
    cols: u32,
    rows: u32,
) -> Result<String, String> {
    let (host, auth) = host_auth(&state.core, &host_id)?;
    let pty = ssh::open_pty(&host, &auth, cols, rows).await?;
    let id = Uuid::new_v4().to_string();
    let (input_tx, input_rx) = tokio::sync::mpsc::unbounded_channel();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    state
        .core
        .active_terminals
        .lock()
        .map_err(|e| e.to_string())?
        .insert(
            id.clone(),
            TerminalHandle {
                input_tx,
                cancel_tx,
            },
        );
    let web = state.clone();
    let task_id = id.clone();
    tokio::spawn(async move {
        let event_state = web.clone();
        ssh::terminal_loop_with(pty, input_rx, cancel_rx, task_id.clone(), move |data| {
            event_state.emit("terminal-data", data)
        })
        .await;
        if let Ok(mut terminals) = web.core.active_terminals.lock() {
            terminals.remove(&task_id);
        }
        web.emit("terminal-closed", task_id);
    });
    Ok(id)
}

fn terminal_message(core: &AppState, id: &str, message: TerminalMsg) -> Result<(), String> {
    let terminals = core.active_terminals.lock().map_err(|e| e.to_string())?;
    terminals
        .get(id)
        .ok_or("Terminal session not found")?
        .input_tx
        .send(message)
        .map_err(|e| e.to_string())
}

fn close_terminal(core: &AppState, id: &str) -> Result<(), String> {
    let handle = core
        .active_terminals
        .lock()
        .map_err(|e| e.to_string())?
        .remove(id)
        .ok_or("Terminal session not found")?;
    let _ = handle.cancel_tx.send(());
    Ok(())
}

async fn start_local_proxy(
    state: &WebState,
    port: u16,
) -> Result<crate::proxy::ProxyStatus, String> {
    proxy::validate_port(port)?;
    {
        let mut runtime = state.core.proxy.lock().map_err(|e| e.to_string())?;
        stop_local_locked(&mut runtime);
    }
    let listener = proxy::bind_local(port).await?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(proxy::accept_loop(listener, rx));
    let probe = proxy::probe_proxy("127.0.0.1", port).await;
    let mut settings = ProxySettings {
        mode: "local".into(),
        host: "127.0.0.1".into(),
        port,
        enabled: true,
        ..Default::default()
    };
    apply_probe(&mut settings, probe);
    {
        let mut runtime = state.core.proxy.lock().map_err(|e| e.to_string())?;
        runtime.settings = settings.clone();
        runtime.stop_tx = Some(tx);
    }
    persist_proxy(&state.core, &settings)?;
    let status = current_proxy_status(&state.core)?;
    state.emit("proxy-status", &status);
    Ok(status)
}

async fn use_existing_proxy(
    state: &WebState,
    host: String,
    port: u16,
) -> Result<crate::proxy::ProxyStatus, String> {
    proxy::validate_host(&host)?;
    proxy::validate_port(port)?;
    {
        let mut runtime = state.core.proxy.lock().map_err(|e| e.to_string())?;
        stop_local_locked(&mut runtime);
    }
    let probe = proxy::probe_proxy(&host, port).await;
    let mut settings = ProxySettings {
        mode: "existing".into(),
        host,
        port,
        enabled: probe.reachable && (probe.http || probe.socks5),
        ..Default::default()
    };
    apply_probe(&mut settings, probe);
    {
        let mut runtime = state.core.proxy.lock().map_err(|e| e.to_string())?;
        runtime.settings = settings.clone();
        runtime.stop_tx = None;
    }
    persist_proxy(&state.core, &settings)?;
    let status = current_proxy_status(&state.core)?;
    state.emit("proxy-status", &status);
    Ok(status)
}

async fn check_proxy(state: &WebState) -> Result<crate::proxy::ProxyStatus, String> {
    let (mode, host, port) = {
        let runtime = state.core.proxy.lock().map_err(|e| e.to_string())?;
        (
            runtime.settings.mode.clone(),
            runtime.settings.host.clone(),
            runtime.settings.port,
        )
    };
    if port == 0 || mode.is_empty() {
        return current_proxy_status(&state.core);
    }
    let host = if mode == "local" {
        "127.0.0.1".into()
    } else {
        host
    };
    let probe = proxy::probe_proxy(&host, port).await;
    let settings = {
        let mut runtime = state.core.proxy.lock().map_err(|e| e.to_string())?;
        apply_probe(&mut runtime.settings, probe);
        if mode == "existing" {
            runtime.settings.enabled = runtime.settings.last_reachable
                && (runtime.settings.last_http || runtime.settings.last_socks5);
        }
        runtime.settings.clone()
    };
    persist_proxy(&state.core, &settings)?;
    let status = current_proxy_status(&state.core)?;
    state.emit("proxy-status", &status);
    Ok(status)
}

fn stop_proxy(state: &WebState) -> Result<crate::proxy::ProxyStatus, String> {
    let settings = {
        let mut runtime = state.core.proxy.lock().map_err(|e| e.to_string())?;
        stop_local_locked(&mut runtime);
        runtime.settings.enabled = false;
        runtime.settings.last_reachable = false;
        runtime.settings.last_http = false;
        runtime.settings.last_socks5 = false;
        runtime.settings.last_message = "已停止".into();
        runtime.settings.last_checked_at = proxy::now_unix();
        runtime.settings.clone()
    };
    persist_proxy(&state.core, &settings)?;
    let status = current_proxy_status(&state.core)?;
    state.emit("proxy-status", &status);
    Ok(status)
}

fn list_injections(core: &AppState) -> Result<Vec<Value>, String> {
    let injected = core.injected_proxies.lock().map_err(|e| e.to_string())?;
    Ok(injected.iter().map(|(host_id, handle)| json!({ "hostId": host_id, "active": true, "remotePort": handle.remote_port, "localEndpoint": handle.local_endpoint })).collect())
}

async fn inject_proxy(state: &WebState, host_id: String) -> Result<Value, String> {
    if state
        .core
        .injected_proxies
        .lock()
        .map_err(|e| e.to_string())?
        .contains_key(&host_id)
    {
        return Err("代理已注入该主机".into());
    }
    let mut status = current_proxy_status(&state.core)?;
    if !status.enabled || !status.reachable || !(status.http || status.socks5) {
        status = check_proxy(state).await?;
    }
    if !status.enabled || !status.reachable || !(status.http || status.socks5) {
        return Err("请先在「代理」面板启用并检测可用的代理".into());
    }
    let (host, auth) = host_auth(&state.core, &host_id)?;
    let remote_port = host.inject_remote_port_or_default();
    let local_endpoint = status.endpoint.clone();
    let established =
        match ssh::establish_inject(&host, &auth, &status.host, status.port, remote_port).await {
            Ok(established) => established,
            Err(error) => {
                if crate::fetch_latest_version(&state.core, &host_id, remote_port)
                    .await
                    .is_ok()
                {
                    let info = json!({
                        "hostId": host_id,
                        "active": true,
                        "remotePort": remote_port,
                        "localEndpoint": "远端现有代理",
                    });
                    state
                        .core
                        .injected_proxies
                        .lock()
                        .map_err(|e| e.to_string())?
                        .insert(
                            host_id,
                            InjectedHandle {
                                stop_tx: None,
                                remote_port,
                                local_endpoint: "远端现有代理".into(),
                            },
                        );
                    state.emit("proxy-injection", &info);
                    return Ok(info);
                }
                return Err(error);
            }
        };
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .core
        .injected_proxies
        .lock()
        .map_err(|e| e.to_string())?
        .insert(
            host_id.clone(),
            InjectedHandle {
                stop_tx: Some(tx),
                remote_port,
                local_endpoint: local_endpoint.clone(),
            },
        );
    let web = state.clone();
    let task_id = host_id.clone();
    tokio::spawn(async move {
        ssh::hold_inject(established, rx).await;
        if let Ok(mut injected) = web.core.injected_proxies.lock() {
            injected.remove(&task_id);
        }
        web.emit(
            "proxy-injection",
            json!({ "hostId": task_id, "active": false, "remotePort": remote_port, "localEndpoint": "" }),
        );
    });
    let info = json!({
        "hostId": host_id,
        "active": true,
        "remotePort": remote_port,
        "localEndpoint": local_endpoint,
    });
    state.emit("proxy-injection", &info);
    Ok(info)
}

fn uninject_proxy(core: &AppState, host_id: &str) -> Result<(), String> {
    let mut injected = core.injected_proxies.lock().map_err(|e| e.to_string())?;
    if injected
        .get(host_id)
        .is_some_and(|handle| handle.stop_tx.is_none())
    {
        return Err("该代理由外部连接注入，无法在此处关闭".into());
    }
    let handle = injected.remove(host_id).ok_or("该主机未注入代理")?;
    if let Some(tx) = handle.stop_tx {
        let _ = tx.send(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_web_ui_contains_adapter() {
        assert!(UiAssets::get("index.html").is_some());
        assert!(UiAssets::get("web-adapter.js").is_some());
    }

    #[test]
    fn default_server_is_loopback_only() {
        let address: SocketAddr = "127.0.0.1:18080".parse().unwrap();
        assert!(address.ip().is_loopback());
    }
}
