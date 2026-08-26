mod auth;
mod certificate;
mod host;
mod host_actions;
mod proxy;
mod ssh;
mod store;
mod tunnel;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

use auth::Auth;
use certificate::Certificate;
use host::Host;
use proxy::{ProxySettings, ProxyStatus};
use ssh::{ResolvedAuth, TerminalMsg};
use store::Store;
use tauri::{AppHandle, Emitter, Manager, State};
use tunnel::{Tunnel, TunnelInfo};
use uuid::Uuid;

struct AppState {
    store: Mutex<Store>,
    active_tunnels: Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>,
    active_terminals: Mutex<HashMap<String, TerminalHandle>>,
    proxy: Mutex<ProxyRuntime>,
    injected_proxies: Mutex<HashMap<String, InjectedHandle>>,
}

struct InjectedHandle {
    stop_tx: tokio::sync::oneshot::Sender<()>,
    remote_port: u16,
    local_endpoint: String,
}

struct ProxyRuntime {
    settings: ProxySettings,
    stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

struct TerminalHandle {
    input_tx: tokio::sync::mpsc::UnboundedSender<TerminalMsg>,
    cancel_tx: tokio::sync::oneshot::Sender<()>,
}

fn resolve_auth(store: &Store, auth: &Auth) -> Result<ResolvedAuth, String> {
    match auth {
        Auth::Password { password } => Ok(ResolvedAuth::Password(password.clone())),
        Auth::Certificate { certificate_id } => {
            let cert = store.get_certificate(certificate_id)?;
            Ok(ResolvedAuth::Key(cert.private_key_path.clone()))
        }
    }
}

fn create_certificate(name: &str) -> Result<Certificate, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Certificate name is required".into());
    }
    let id = Uuid::new_v4().to_string();
    let private_path = PathBuf::from("config").join("keys").join(&id);
    let public_key = ssh::generate_keypair(&private_path)?;
    Ok(Certificate {
        id,
        name: name.to_string(),
        private_key_path: private_path.to_string_lossy().into_owned(),
        public_key,
    })
}

fn err_box(e: String) -> Box<dyn std::error::Error> {
    e.into()
}

// ---- hosts -----------------------------------------------------------------

#[tauri::command]
fn list_hosts(state: State<'_, AppState>) -> Result<Vec<Host>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.list_hosts()
}

#[tauri::command]
fn add_host(state: State<'_, AppState>, mut host: Host) -> Result<Host, String> {
    if host.id.is_empty() {
        host.id = Uuid::new_v4().to_string();
    }
    host.validate()?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.add_host(&host)?;
    Ok(host)
}

#[tauri::command]
fn update_host(state: State<'_, AppState>, host: Host) -> Result<(), String> {
    host.validate()?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.update_host(&host)
}

#[tauri::command]
fn delete_host(state: State<'_, AppState>, id: String) -> Result<(), String> {
    if let Some(handle) = state
        .injected_proxies
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&id)
    {
        let _ = handle.stop_tx.send(());
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.delete_host(&id)
}

#[tauri::command]
async fn ssh_execute(
    state: State<'_, AppState>,
    host_id: String,
    command: String,
) -> Result<ssh::ExecOutput, String> {
    let (host, auth) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let host = store.get_host(&host_id)?;
        let auth = resolve_auth(&store, &host.auth)?;
        (host, auth)
    };
    ssh::execute(&host, &command, &auth).await
}

// ---- tunnels ---------------------------------------------------------------

#[tauri::command]
fn list_tunnels(state: State<'_, AppState>) -> Result<Vec<TunnelInfo>, String> {
    let tunnels = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store.list_tunnels()?
    };
    let active = state.active_tunnels.lock().map_err(|e| e.to_string())?;
    Ok(tunnels
        .into_iter()
        .map(|tunnel| {
            let is_active = active.contains_key(&tunnel.id);
            TunnelInfo {
                tunnel,
                active: is_active,
            }
        })
        .collect())
}

#[tauri::command]
fn add_tunnel(state: State<'_, AppState>, mut tunnel: Tunnel) -> Result<Tunnel, String> {
    if tunnel.id.is_empty() {
        tunnel.id = Uuid::new_v4().to_string();
    }
    tunnel.validate()?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.add_tunnel(&tunnel)?;
    Ok(tunnel)
}

#[tauri::command]
fn update_tunnel(state: State<'_, AppState>, tunnel: Tunnel) -> Result<(), String> {
    tunnel.validate()?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.update_tunnel(&tunnel)
}

#[tauri::command]
fn delete_tunnel(state: State<'_, AppState>, id: String) -> Result<(), String> {
    if let Some(tx) = state
        .active_tunnels
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&id)
    {
        let _ = tx.send(());
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.delete_tunnel(&id)
}

#[tauri::command]
fn parse_ssh_command(command: String) -> Result<Tunnel, String> {
    tunnel::parse_ssh_command(&command)
}

#[tauri::command]
async fn tunnel_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    tunnel_id: String,
) -> Result<(), String> {
    {
        let active = state.active_tunnels.lock().map_err(|e| e.to_string())?;
        if active.contains_key(&tunnel_id) {
            return Err("Tunnel is already connected".into());
        }
    }

    let (tunnel, auth) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let tunnel = store.get_tunnel(&tunnel_id)?;
        let auth = resolve_auth(&store, &tunnel.auth)?;
        (tunnel, auth)
    };

    let established = ssh::establish_tunnel(&tunnel, &auth).await?;

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    {
        let mut active = state.active_tunnels.lock().map_err(|e| e.to_string())?;
        if active.contains_key(&tunnel_id) {
            return Err("Tunnel is already connected".into());
        }
        active.insert(tunnel_id.clone(), tx);
    }

    let remote_host = tunnel.remote_host.clone();
    let remote_port = tunnel.remote_port;
    let app2 = app.clone();
    let id = tunnel_id.clone();

    tauri::async_runtime::spawn(async move {
        ssh::accept_loop(established, remote_host, remote_port, rx).await;

        if let Some(st) = app2.try_state::<AppState>() {
            if let Ok(mut active) = st.active_tunnels.lock() {
                active.remove(&id);
            }
        }
        let _ = app2.emit("tunnel-stopped", &id);
    });

    Ok(())
}

#[tauri::command]
fn tunnel_disconnect(state: State<'_, AppState>, tunnel_id: String) -> Result<(), String> {
    let mut active = state.active_tunnels.lock().map_err(|e| e.to_string())?;
    match active.remove(&tunnel_id) {
        Some(tx) => {
            let _ = tx.send(());
            Ok(())
        }
        None => Err("Tunnel is not connected".into()),
    }
}

// ---- terminals -------------------------------------------------------------

#[tauri::command]
async fn start_terminal(
    app: AppHandle,
    state: State<'_, AppState>,
    host_id: String,
    cols: u32,
    rows: u32,
) -> Result<String, String> {
    let (host, auth) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let host = store.get_host(&host_id)?;
        let auth = resolve_auth(&store, &host.auth)?;
        (host, auth)
    };

    let pty = ssh::open_pty(&host, &auth, cols, rows).await?;

    let term_id = Uuid::new_v4().to_string();
    let (input_tx, input_rx) = tokio::sync::mpsc::unbounded_channel::<TerminalMsg>();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

    {
        let mut terms = state.active_terminals.lock().map_err(|e| e.to_string())?;
        terms.insert(
            term_id.clone(),
            TerminalHandle {
                input_tx,
                cancel_tx,
            },
        );
    }

    let app2 = app.clone();
    let id = term_id.clone();
    tauri::async_runtime::spawn(async move {
        ssh::terminal_loop(pty, input_rx, cancel_rx, app2.clone(), id.clone()).await;

        if let Some(st) = app2.try_state::<AppState>() {
            if let Ok(mut terms) = st.active_terminals.lock() {
                terms.remove(&id);
            }
        }
        let _ = app2.emit("terminal-closed", &id);
    });

    Ok(term_id)
}

#[tauri::command]
fn terminal_input(state: State<'_, AppState>, term_id: String, data: String) -> Result<(), String> {
    let terms = state.active_terminals.lock().map_err(|e| e.to_string())?;
    match terms.get(&term_id) {
        Some(handle) => handle
            .input_tx
            .send(TerminalMsg::Input(data.into_bytes()))
            .map_err(|e| e.to_string()),
        None => Err("Terminal session not found".into()),
    }
}

#[tauri::command]
fn terminal_resize(
    state: State<'_, AppState>,
    term_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let terms = state.active_terminals.lock().map_err(|e| e.to_string())?;
    match terms.get(&term_id) {
        Some(handle) => handle
            .input_tx
            .send(TerminalMsg::Resize(cols, rows))
            .map_err(|e| e.to_string()),
        None => Err("Terminal session not found".into()),
    }
}

#[tauri::command]
fn terminal_close(state: State<'_, AppState>, term_id: String) -> Result<(), String> {
    let mut terms = state.active_terminals.lock().map_err(|e| e.to_string())?;
    match terms.remove(&term_id) {
        Some(handle) => {
            let _ = handle.cancel_tx.send(());
            Ok(())
        }
        None => Err("Terminal session not found".into()),
    }
}

// ---- certificates ----------------------------------------------------------

#[tauri::command]
fn list_certificates(state: State<'_, AppState>) -> Result<Vec<Certificate>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.list_certificates()
}

#[tauri::command]
fn add_certificate(state: State<'_, AppState>, name: String) -> Result<Certificate, String> {
    let cert = create_certificate(&name)?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.add_certificate(&cert)?;
    Ok(cert)
}

#[tauri::command]
fn delete_certificate(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let cert = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store.get_certificate(&id)?
    };
    {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store.delete_certificate(&id)?;
    }
    let _ = std::fs::remove_file(&cert.private_key_path);
    let _ = std::fs::remove_file(format!("{}.pub", cert.private_key_path));
    Ok(())
}

// ---- proxy -----------------------------------------------------------------

fn persist_proxy(state: &AppState, settings: &ProxySettings) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.save_proxy_settings(settings)
}

fn stop_local_locked(runtime: &mut ProxyRuntime) {
    if let Some(tx) = runtime.stop_tx.take() {
        let _ = tx.send(());
    }
}

fn current_proxy_status(state: &AppState) -> Result<ProxyStatus, String> {
    let runtime = state.proxy.lock().map_err(|e| e.to_string())?;
    let running = runtime.stop_tx.is_some();
    Ok(ProxyStatus::from_settings(&runtime.settings, running))
}

fn apply_probe(settings: &mut ProxySettings, probe: proxy::ProbeResult) {
    settings.last_reachable = probe.reachable;
    settings.last_http = probe.http;
    settings.last_socks5 = probe.socks5;
    settings.last_message = probe.message;
    settings.last_checked_at = proxy::now_unix();
}

fn usable_local_proxy(state: &AppState) -> Result<(String, u16, String), String> {
    let status = current_proxy_status(state)?;
    if !status.enabled || !status.reachable || !(status.http || status.socks5) {
        return Err("请先在「代理」面板启用并检测可用的代理".into());
    }
    if status.port == 0 {
        return Err("代理端口无效".into());
    }
    Ok((status.host, status.port, status.endpoint))
}

/// Reuse a known-good proxy, otherwise probe saved settings then 127.0.0.1:7890.
async fn ensure_usable_proxy(
    app: &AppHandle,
    state: &AppState,
) -> Result<(String, u16, String), String> {
    if let Ok(ok) = usable_local_proxy(state) {
        return Ok(ok);
    }

    let (saved_host, saved_port, saved_mode, running) = {
        let runtime = state.proxy.lock().map_err(|e| e.to_string())?;
        let host = if runtime.settings.mode == "local" || runtime.settings.host.trim().is_empty() {
            "127.0.0.1".to_string()
        } else {
            runtime.settings.host.clone()
        };
        (
            host,
            runtime.settings.port,
            runtime.settings.mode.clone(),
            runtime.stop_tx.is_some(),
        )
    };

    let mut candidates: Vec<(String, u16, String, bool)> = Vec::new();
    if saved_port != 0 {
        candidates.push((saved_host.clone(), saved_port, saved_mode.clone(), running));
    }
    if saved_host != "127.0.0.1" || saved_port != 7890 {
        candidates.push(("127.0.0.1".to_string(), 7890, "existing".to_string(), false));
    }
    if candidates.is_empty() {
        candidates.push(("127.0.0.1".to_string(), 7890, "existing".to_string(), false));
    }

    let mut tried = HashSet::new();
    for (host, port, mode, is_running) in candidates {
        if !tried.insert((host.clone(), port)) {
            continue;
        }
        let probe = proxy::probe_proxy(&host, port).await;
        if !(probe.reachable && (probe.http || probe.socks5)) {
            continue;
        }
        let keep_local = mode == "local" && is_running;
        let mut settings = ProxySettings {
            mode: if keep_local {
                "local".into()
            } else {
                "existing".into()
            },
            host: host.clone(),
            port,
            enabled: true,
            ..Default::default()
        };
        apply_probe(&mut settings, probe);
        if settings.last_message.is_empty() {
            settings.last_message = format!("已检测到代理 {host}:{port}");
        }
        {
            let mut runtime = state.proxy.lock().map_err(|e| e.to_string())?;
            runtime.settings = settings.clone();
        }
        persist_proxy(state, &settings)?;
        let status = current_proxy_status(state)?;
        let _ = app.emit("proxy-status", &status);
        return Ok((host, port, status.endpoint));
    }

    Err("未检测到可用代理，请先在「代理」面板启用并检测".into())
}

/// Live reverse-forward (`ssh -N -R`) of the local proxy onto a host.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProxyInjection {
    host_id: String,
    active: bool,
    remote_port: u16,
    local_endpoint: String,
}

#[tauri::command]
fn get_proxy_status(state: State<'_, AppState>) -> Result<ProxyStatus, String> {
    current_proxy_status(&state)
}

#[tauri::command]
async fn start_local_proxy(
    app: AppHandle,
    state: State<'_, AppState>,
    port: u16,
) -> Result<ProxyStatus, String> {
    proxy::validate_port(port)?;

    {
        let mut runtime = state.proxy.lock().map_err(|e| e.to_string())?;
        stop_local_locked(&mut runtime);
    }

    let listener = proxy::bind_local(port).await?;
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tauri::async_runtime::spawn(async move {
        proxy::accept_loop(listener, rx).await;
    });

    let probe = proxy::probe_proxy("127.0.0.1", port).await;
    let mut settings = ProxySettings {
        mode: "local".into(),
        host: "127.0.0.1".into(),
        port,
        enabled: true,
        ..Default::default()
    };
    apply_probe(&mut settings, probe);
    if settings.last_message.is_empty() {
        settings.last_message = format!("本机混合代理已启动 0.0.0.0:{port}");
    }

    {
        let mut runtime = state.proxy.lock().map_err(|e| e.to_string())?;
        runtime.settings = settings.clone();
        runtime.stop_tx = Some(tx);
    }
    persist_proxy(&state, &settings)?;
    let status = current_proxy_status(&state)?;
    let _ = app.emit("proxy-status", &status);
    Ok(status)
}

#[tauri::command]
async fn use_existing_proxy(
    app: AppHandle,
    state: State<'_, AppState>,
    host: String,
    port: u16,
) -> Result<ProxyStatus, String> {
    let host = host.trim().to_string();
    proxy::validate_host(&host)?;
    proxy::validate_port(port)?;

    {
        let mut runtime = state.proxy.lock().map_err(|e| e.to_string())?;
        stop_local_locked(&mut runtime);
    }

    let probe = proxy::probe_proxy(&host, port).await;
    let mut settings = ProxySettings {
        mode: "existing".into(),
        host: host.clone(),
        port,
        enabled: probe.reachable && (probe.http || probe.socks5),
        ..Default::default()
    };
    apply_probe(&mut settings, probe);
    if !settings.enabled && settings.last_message.is_empty() {
        settings.last_message = "已有代理不可用".into();
    }

    {
        let mut runtime = state.proxy.lock().map_err(|e| e.to_string())?;
        runtime.settings = settings.clone();
        runtime.stop_tx = None;
    }
    persist_proxy(&state, &settings)?;
    let status = current_proxy_status(&state)?;
    let _ = app.emit("proxy-status", &status);
    Ok(status)
}

#[tauri::command]
async fn check_proxy(app: AppHandle, state: State<'_, AppState>) -> Result<ProxyStatus, String> {
    let (mode, host, port, enabled) = {
        let runtime = state.proxy.lock().map_err(|e| e.to_string())?;
        (
            runtime.settings.mode.clone(),
            if runtime.settings.mode == "local" {
                "127.0.0.1".to_string()
            } else {
                runtime.settings.host.clone()
            },
            runtime.settings.port,
            runtime.settings.enabled,
        )
    };

    if port == 0 || mode.is_empty() {
        return current_proxy_status(&state);
    }

    let probe = proxy::probe_proxy(&host, port).await;
    let settings = {
        let mut runtime = state.proxy.lock().map_err(|e| e.to_string())?;
        apply_probe(&mut runtime.settings, probe);
        if mode == "existing" {
            runtime.settings.enabled = runtime.settings.last_reachable
                && (runtime.settings.last_http || runtime.settings.last_socks5);
        } else if enabled && mode == "local" && runtime.stop_tx.is_none() {
            runtime.settings.enabled = false;
            if runtime.settings.last_message.is_empty() {
                runtime.settings.last_message = "本机代理未在运行".into();
            }
        }
        runtime.settings.clone()
    };
    persist_proxy(&state, &settings)?;
    let status = current_proxy_status(&state)?;
    let _ = app.emit("proxy-status", &status);
    Ok(status)
}

#[tauri::command]
fn stop_proxy(app: AppHandle, state: State<'_, AppState>) -> Result<ProxyStatus, String> {
    let settings = {
        let mut runtime = state.proxy.lock().map_err(|e| e.to_string())?;
        stop_local_locked(&mut runtime);
        runtime.settings.enabled = false;
        runtime.settings.last_reachable = false;
        runtime.settings.last_http = false;
        runtime.settings.last_socks5 = false;
        runtime.settings.last_message = "已停止".into();
        runtime.settings.last_checked_at = proxy::now_unix();
        runtime.settings.clone()
    };
    persist_proxy(&state, &settings)?;
    let status = current_proxy_status(&state)?;
    let _ = app.emit("proxy-status", &status);
    Ok(status)
}

#[tauri::command]
fn list_proxy_injections(state: State<'_, AppState>) -> Result<Vec<ProxyInjection>, String> {
    let injected = state.injected_proxies.lock().map_err(|e| e.to_string())?;
    Ok(injected
        .iter()
        .map(|(host_id, handle)| ProxyInjection {
            host_id: host_id.clone(),
            active: true,
            remote_port: handle.remote_port,
            local_endpoint: handle.local_endpoint.clone(),
        })
        .collect())
}

#[tauri::command]
async fn inject_proxy(
    app: AppHandle,
    state: State<'_, AppState>,
    host_id: String,
) -> Result<ProxyInjection, String> {
    {
        let injected = state.injected_proxies.lock().map_err(|e| e.to_string())?;
        if injected.contains_key(&host_id) {
            return Err("代理已注入该主机".into());
        }
    }

    let (local_host, local_port, local_endpoint) = ensure_usable_proxy(&app, &state).await?;
    let (host, auth) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let host = store.get_host(&host_id)?;
        let auth = resolve_auth(&store, &host.auth)?;
        (host, auth)
    };

    let remote_port = host.inject_remote_port_or_default();
    let established =
        ssh::establish_inject(&host, &auth, &local_host, local_port, remote_port).await?;

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    {
        let mut injected = state.injected_proxies.lock().map_err(|e| e.to_string())?;
        if injected.contains_key(&host_id) {
            return Err("代理已注入该主机".into());
        }
        injected.insert(
            host_id.clone(),
            InjectedHandle {
                stop_tx: tx,
                remote_port,
                local_endpoint: local_endpoint.clone(),
            },
        );
    }

    let app2 = app.clone();
    let id = host_id.clone();
    tauri::async_runtime::spawn(async move {
        ssh::hold_inject(established, rx).await;
        if let Some(st) = app2.try_state::<AppState>() {
            if let Ok(mut injected) = st.injected_proxies.lock() {
                injected.remove(&id);
            }
        }
        let _ = app2.emit(
            "proxy-injection",
            ProxyInjection {
                host_id: id,
                active: false,
                remote_port,
                local_endpoint: String::new(),
            },
        );
    });

    let info = ProxyInjection {
        host_id,
        active: true,
        remote_port,
        local_endpoint,
    };
    let _ = app.emit("proxy-injection", &info);
    Ok(info)
}

fn require_injected(state: &AppState, host_id: &str) -> Result<u16, String> {
    let injected = state.injected_proxies.lock().map_err(|e| e.to_string())?;
    match injected.get(host_id) {
        Some(handle) => Ok(handle.remote_port),
        None => Err("请先注入代理，下载走远程已注入的代理端口".into()),
    }
}

async fn ssh_run(
    state: &State<'_, AppState>,
    host_id: &str,
    command: String,
) -> Result<ssh::ExecOutput, String> {
    let (host, auth) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let host = store.get_host(host_id)?;
        let auth = resolve_auth(&store, &host.auth)?;
        (host, auth)
    };
    ssh::execute(&host, &command, &auth).await
}

async fn run_probe(
    state: &State<'_, AppState>,
    host_id: &str,
) -> Result<host_actions::UpdateProbe, String> {
    let out = ssh_run(state, host_id, host_actions::wrap_probe_command()).await?;
    if out.exit_status != 0 {
        return Err(format!(
            "probe failed (exit {}): {}",
            out.exit_status,
            out.stderr.trim()
        ));
    }
    host_actions::parse_probe(&out.stdout)
}

async fn fetch_latest_version(
    state: &State<'_, AppState>,
    host_id: &str,
    remote_port: u16,
) -> Result<String, String> {
    let cmd = host_actions::wrap_check_command(&host_actions::inject_proxy_url(remote_port));
    let out = ssh_run(state, host_id, cmd).await?;
    if out.exit_status != 0 {
        return Err(format!(
            "version check failed (exit {}): {}",
            out.exit_status,
            out.stderr.trim()
        ));
    }
    host_actions::parse_latest_version(&out.stdout)
}

#[tauri::command]
async fn probe_cangling_update(
    state: State<'_, AppState>,
    host_id: String,
) -> Result<host_actions::UpdateProbe, String> {
    let mut probe = run_probe(&state, &host_id).await?;
    if probe.installed && probe.supported {
        match require_injected(&state, &host_id) {
            Ok(remote_port) => match fetch_latest_version(&state, &host_id, remote_port).await {
                Ok(latest) => {
                    probe.latest = latest.clone();
                    probe.update_available = host_actions::is_newer(&latest, &probe.version);
                }
                Err(e) => probe.version_error = e,
            },
            // Without an injected proxy the remote host cannot reach the
            // update server; keep the probe result and let the UI ask for one.
            Err(_) => {}
        }
    }
    Ok(probe)
}

#[tauri::command]
async fn run_cangling_update(
    state: State<'_, AppState>,
    host_id: String,
) -> Result<host_actions::UpdateApplyResult, String> {
    let remote_port = require_injected(&state, &host_id)?;
    let probe = run_probe(&state, &host_id).await?;
    if !probe.supported {
        return Err(format!("unsupported CPU arch: {}", probe.arch));
    }
    let action = if probe.installed { "update" } else { "install" };
    let cmd = host_actions::wrap_apply_command(
        action,
        &probe.arch,
        &host_actions::inject_proxy_url(remote_port),
    );
    let out = ssh_run(&state, &host_id, cmd).await?;
    if out.exit_status != 0 {
        return Err(format!(
            "{action} failed (exit {}): {}\n{}",
            out.exit_status,
            out.stderr.trim(),
            out.stdout.trim()
        ));
    }
    Ok(host_actions::UpdateApplyResult {
        action: action.to_string(),
        stdout: out.stdout,
        stderr: out.stderr,
        exit_status: out.exit_status,
    })
}

#[tauri::command]
fn uninject_proxy(state: State<'_, AppState>, host_id: String) -> Result<(), String> {
    let mut injected = state.injected_proxies.lock().map_err(|e| e.to_string())?;
    match injected.remove(&host_id) {
        Some(handle) => {
            let _ = handle.stop_tx.send(());
            Ok(())
        }
        None => Err("该主机未注入代理".into()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // User data lives in ./config/data.sql (SQLite), relative to the
            // directory the app is launched from.
            let data_file = PathBuf::from("config").join("data.sql");
            let store = Store::load(data_file)?;

            // Ensure at least one certificate exists on startup.
            if store.list_certificates().map_err(err_box)?.is_empty() {
                let cert = create_certificate("Local Certificate").map_err(err_box)?;
                store.add_certificate(&cert).map_err(err_box)?;
            }

            let proxy_settings = store.get_proxy_settings().unwrap_or_default();
            app.manage(AppState {
                store: Mutex::new(store),
                active_tunnels: Mutex::new(HashMap::new()),
                active_terminals: Mutex::new(HashMap::new()),
                proxy: Mutex::new(ProxyRuntime {
                    settings: proxy_settings,
                    stop_tx: None,
                }),
                injected_proxies: Mutex::new(HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_hosts,
            add_host,
            update_host,
            delete_host,
            ssh_execute,
            list_tunnels,
            add_tunnel,
            update_tunnel,
            delete_tunnel,
            parse_ssh_command,
            tunnel_connect,
            tunnel_disconnect,
            list_certificates,
            add_certificate,
            delete_certificate,
            start_terminal,
            terminal_input,
            terminal_resize,
            terminal_close,
            get_proxy_status,
            start_local_proxy,
            use_existing_proxy,
            check_proxy,
            stop_proxy,
            list_proxy_injections,
            inject_proxy,
            uninject_proxy,
            probe_cangling_update,
            run_cangling_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
