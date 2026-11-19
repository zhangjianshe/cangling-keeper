use super::*;
use axum::{
    Json, Router,
    extract::State as ApiState,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone)]
struct Remote {
    row: Arc<Mutex<Value>>,
    drop_websocket: bool,
}
async fn save(ApiState(remote): ApiState<Remote>, Json(body): Json<Value>) -> Json<Value> {
    let mut row = body["tunnel"].clone();
    row["id"] = json!("server-tunnel");
    row["userId"] = json!(1);
    if remote.drop_websocket {
        row.as_object_mut().unwrap().remove("tunnelType");
        row.as_object_mut().unwrap().remove("websocketUrl");
    }
    *remote.row.lock().unwrap() = row.clone();
    Json(json!({"code":200,"success":true,"data":{"tunnel":row}}))
}
async fn list(ApiState(remote): ApiState<Remote>) -> Json<Value> {
    let row = remote.row.lock().unwrap().clone();
    Json(
        json!({"code":200,"success":true,"data":{"tunnels":if row.is_null(){vec![]}else{vec![row]}}}),
    )
}
async fn setup(
    drop_websocket: bool,
) -> (
    Arc<AppState>,
    Remote,
    tokio::task::JoinHandle<()>,
    PathBuf,
    Tunnel,
) {
    let remote = Remote {
        row: Arc::new(Mutex::new(Value::Null)),
        drop_websocket,
    };
    let router = Router::new()
        .route("/api/v1/keeper/tunnel/save", post(save))
        .route("/api/v1/keeper/tunnel/list", get(list))
        .route(
            "/api/v1/keeper/host/list",
            get(|| async { Json(json!({"code":200,"success":true,"data":{"hosts":[]}})) }),
        )
        .with_state(remote.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let path = std::env::temp_dir().join(format!("keeper-upload-tunnel-{}", Uuid::new_v4()));
    let state = Arc::new(create_app_state(path.clone()).unwrap());
    let mut tunnel =
        tunnel::parse_ssh_command("ssh -L 12345:localhost:22 root@example.test").unwrap();
    tunnel.name = "test tunnel".into();
    tunnel.remote_id = "server-tunnel".into();
    tunnel.user_id = 1;
    {
        let store = state.store.lock().unwrap();
        store.set_setting(sync::SETTING_SERVER_URL, &url).unwrap();
        store
            .set_setting(sync::SETTING_TOKEN, "test-token")
            .unwrap();
        store.add_tunnel(&tunnel).unwrap();
        let mut stale = sync::tunnel_to_sync(&store, &state.data_dir, &tunnel).unwrap();
        stale.local_port = 9999;
        *remote.row.lock().unwrap() = serde_json::to_value(stale).unwrap();
    }
    (state, remote, task, path, tunnel)
}

#[tokio::test]
async fn unsent_local_edit_survives_pull_then_explicit_upload_replaces_server() {
    let (state, remote, task, path, tunnel) = setup(false).await;
    update_tunnel(&state, tunnel.clone()).await.unwrap();
    let url = state
        .store
        .lock()
        .unwrap()
        .get_setting(sync::SETTING_SERVER_URL)
        .unwrap()
        .unwrap();
    pull_sync(&state, &url, "test-token").await.unwrap();
    assert_eq!(
        state
            .store
            .lock()
            .unwrap()
            .get_tunnel(&tunnel.id)
            .unwrap()
            .local_port,
        12345
    );
    upload_tunnel_to_server(&state, &tunnel.id).await.unwrap();
    assert_eq!(remote.row.lock().unwrap()["localPort"], 12345);
    assert!(
        state
            .store
            .lock()
            .unwrap()
            .get_setting(&tunnel_dirty_key(&tunnel.id))
            .unwrap()
            .is_none()
    );
    pull_sync(&state, &url, "test-token").await.unwrap();
    assert_eq!(
        state
            .store
            .lock()
            .unwrap()
            .get_tunnel(&tunnel.id)
            .unwrap()
            .local_port,
        12345
    );
    task.abort();
    drop(state);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn websocket_upload_round_trips_all_transport_fields() {
    let (state, remote, task, path, mut tunnel) = setup(false).await;
    tunnel.tunnel_type = "websocket".into();
    tunnel.websocket_url = "wss://example.test/update/api/tunnel/ws".into();
    tunnel.auth = Auth::Certificate {
        certificate_id: state.store.lock().unwrap().list_certificates().unwrap()[0]
            .id
            .clone(),
    };
    update_tunnel(&state, tunnel.clone()).await.unwrap();
    upload_tunnel_to_server(&state, &tunnel.id).await.unwrap();
    assert_eq!(remote.row.lock().unwrap()["tunnelType"], "websocket");
    assert_eq!(
        remote.row.lock().unwrap()["websocketUrl"],
        tunnel.websocket_url
    );
    assert!(
        state
            .store
            .lock()
            .unwrap()
            .get_setting(&tunnel_dirty_key(&tunnel.id))
            .unwrap()
            .is_none()
    );
    task.abort();
    drop(state);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn old_server_cannot_report_websocket_upload_success_or_discard_local_edits() {
    let (state, _remote, task, path, mut tunnel) = setup(true).await;
    tunnel.tunnel_type = "websocket".into();
    tunnel.websocket_url = "wss://example.test/update/api/tunnel/ws".into();
    tunnel.auth = Auth::Certificate {
        certificate_id: state.store.lock().unwrap().list_certificates().unwrap()[0]
            .id
            .clone(),
    };
    update_tunnel(&state, tunnel.clone()).await.unwrap();
    assert!(
        upload_tunnel_to_server(&state, &tunnel.id)
            .await
            .unwrap_err()
            .contains("没有完整保存")
    );
    assert_eq!(
        state
            .store
            .lock()
            .unwrap()
            .get_setting(&tunnel_dirty_key(&tunnel.id))
            .unwrap()
            .as_deref(),
        Some("1")
    );
    assert_eq!(
        state
            .store
            .lock()
            .unwrap()
            .get_tunnel(&tunnel.id)
            .unwrap()
            .websocket_url,
        tunnel.websocket_url
    );
    task.abort();
    drop(state);
    std::fs::remove_dir_all(path).unwrap();
}
