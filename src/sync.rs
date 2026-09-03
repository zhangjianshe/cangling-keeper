use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};

use crate::auth::Auth;
use crate::certificate::Certificate;
use crate::host::Host;
use crate::store::Store;
use uuid::Uuid;

pub const SETTING_SERVER_URL: &str = "server_url";
pub const SETTING_TOKEN: &str = "login_token";
pub const SETTING_NICKNAME: &str = "login_nickname";
pub const SETTING_USERNAME: &str = "login_username";

/// Generic server response envelope: `{ code, message, success, data }`.
#[derive(Debug, Deserialize)]
struct ApiEnvelope {
    #[serde(default)]
    code: i32,
    #[serde(default)]
    message: String,
    #[serde(default)]
    success: bool,
    #[serde(default)]
    data: Option<serde_json::Value>,
}

/// A host as stored on the server (matches `CanglingHostEntity` camelCase JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncHost {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default, deserialize_with = "null_to_default_u16")]
    pub update_port: u16,
    #[serde(default)]
    pub update_role: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub auth_method: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub private_key: Option<String>,
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub inject_remote_port: u16,
    #[serde(default)]
    pub catalog: Option<String>,
    #[serde(default)]
    pub is_public: u8,
    /// Whether this host belongs to the currently logged-in user.
    #[serde(default)]
    pub mine: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginData {
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub user_name: String,
    #[serde(default)]
    pub nick_name: String,
}

/// Newer server fields may be NULL on rows created before the schema change.
/// Treat null exactly like a missing port so old records remain synchronizable.
fn null_to_default_u16<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<u16>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
struct HostListData {
    #[serde(default)]
    hosts: Vec<SyncHost>,
}

#[derive(Debug, Deserialize)]
struct SaveHostData {
    #[serde(default)]
    host: Option<SyncHost>,
}

fn base_url(server_url: &str) -> String {
    server_url.trim_end_matches('/').to_string()
}

fn client() -> Result<reqwest::Client, String> {
    // The server uses a self-signed certificate, accept it the same way the
    // existing update/self-update code does.
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())
}

async fn unwrap_envelope<T>(text: String) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    let env: ApiEnvelope =
        serde_json::from_str(&text).map_err(|e| format!("解析服务器响应失败: {e}"))?;
    if !env.success || env.code != 200 {
        let msg = if env.message.is_empty() {
            format!("服务器错误 code={}", env.code)
        } else {
            env.message
        };
        return Err(msg);
    }
    let data = env.data.ok_or_else(|| "服务器返回空数据".to_string())?;
    serde_json::from_value::<T>(data).map_err(|e| format!("解析服务器数据失败: {e}"))
}

pub async fn login(server_url: &str, username: &str, password: &str) -> Result<LoginData, String> {
    let url = format!("{}/api/v1/user/login", base_url(server_url));
    let body = serde_json::json!({ "userName": username, "password": password });
    let resp = client()?
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("登录请求失败: {e}"))?;
    let text = resp.text().await.map_err(|e| e.to_string())?;
    unwrap_envelope::<LoginData>(text).await
}

pub async fn pull_hosts(server_url: &str, token: &str) -> Result<Vec<SyncHost>, String> {
    let url = format!("{}/api/v1/host/list", base_url(server_url));
    let resp = client()?
        .get(&url)
        .header("API-TOKEN", token)
        .send()
        .await
        .map_err(|e| format!("拉取主机列表失败: {e}"))?;
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let data: HostListData = unwrap_envelope(text).await?;
    Ok(data.hosts)
}

pub async fn push_host(server_url: &str, token: &str, host: &SyncHost) -> Result<SyncHost, String> {
    let url = format!("{}/api/v1/host/save", base_url(server_url));
    let body = serde_json::json!({ "host": host });
    let resp = client()?
        .post(&url)
        .header("API-TOKEN", token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("保存主机到服务器失败: {e}"))?;
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let data: SaveHostData = unwrap_envelope(text).await?;
    data.host
        .ok_or_else(|| "服务器未返回保存后的主机".to_string())
}

pub async fn delete_remote(server_url: &str, token: &str, id: &str) -> Result<(), String> {
    let url = format!("{}/api/v1/host/delete", base_url(server_url));
    let body = serde_json::json!({ "id": id });
    let resp = client()?
        .post(&url)
        .header("API-TOKEN", token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("删除服务器主机失败: {e}"))?;
    let text = resp.text().await.map_err(|e| e.to_string())?;
    // delete returns an empty object as data
    unwrap_envelope::<serde_json::Value>(text).await?;
    Ok(())
}

/// Convert a local host into the server representation, reading the private key
/// file for certificate auth so it can be synced too.
pub fn host_to_sync(store: &Store, data_dir: &Path, host: &Host) -> Result<SyncHost, String> {
    let (auth_method, password, private_key, public_key) = match &host.auth {
        Auth::Password { password } => ("password".to_string(), Some(password.clone()), None, None),
        Auth::Certificate { certificate_id } => {
            let cert = store.get_certificate(certificate_id)?;
            let key_path = crate::resolve_key_path(data_dir, &cert.private_key_path);
            let private = std::fs::read_to_string(&key_path)
                .map_err(|e| format!("读取私钥失败 {}: {e}", key_path.display()))?;
            (
                "certificate".to_string(),
                None,
                Some(private),
                Some(cert.public_key),
            )
        }
    };

    Ok(SyncHost {
        id: host.remote_id.clone(),
        name: host.name.clone(),
        hostname: host.hostname.clone(),
        port: host.port,
        update_port: host.update_port_or_default(),
        update_role: host.update_role.clone(),
        username: host.username.clone(),
        auth_method,
        password,
        private_key,
        public_key,
        inject_remote_port: host.inject_remote_port_or_default(),
        catalog: if host.catalog.trim().is_empty() {
            None
        } else {
            Some(host.catalog.clone())
        },
        is_public: if host.is_public { 1 } else { 0 },
        mine: false,
    })
}

/// Convert a server host into a local host, importing the certificate private
/// key if the host uses certificate auth.
pub fn sync_to_host(store: &Store, keys_dir: &Path, s: &SyncHost) -> Result<Host, String> {
    let auth = if s.auth_method == "certificate" {
        let cert = import_certificate(
            store,
            keys_dir,
            s.public_key.clone().unwrap_or_default(),
            s.private_key.clone().unwrap_or_default(),
        )?;
        Auth::Certificate {
            certificate_id: cert.id,
        }
    } else {
        Auth::Password {
            password: s.password.clone().unwrap_or_default(),
        }
    };

    Ok(Host {
        id: Uuid::new_v4().to_string(),
        name: s.name.clone(),
        hostname: s.hostname.clone(),
        port: if s.port == 0 { 22 } else { s.port },
        update_port: if s.update_port == 0 { 5400 } else { s.update_port },
        update_role: if s.update_role.trim().is_empty() {
            "standalone".into()
        } else {
            s.update_role.clone()
        },
        username: s.username.clone(),
        inject_remote_port: if s.inject_remote_port == 0 {
            7890
        } else {
            s.inject_remote_port
        },
        auth,
        catalog: s.catalog.clone().unwrap_or_default(),
        remote_id: s.id.clone(),
        is_public: s.is_public != 0,
        owned: s.mine,
    })
}

/// Import a synced certificate: reuse an existing one with the same public key,
/// otherwise write the key files and register a new certificate.
fn import_certificate(
    store: &Store,
    keys_dir: &Path,
    public_key: String,
    private_key: String,
) -> Result<Certificate, String> {
    let public_key = public_key.trim().to_string();
    if public_key.is_empty() || private_key.trim().is_empty() {
        return Err("服务器主机缺少证书私钥/公钥".into());
    }

    // Reuse a local certificate that already has this public key.
    for cert in store.list_certificates()? {
        if cert.public_key.trim() == public_key {
            return Ok(cert);
        }
    }

    let id = Uuid::new_v4().to_string();
    let private_path = keys_dir.join(&id);
    std::fs::create_dir_all(keys_dir).map_err(|e| e.to_string())?;
    std::fs::write(&private_path, private_key.trim_end())
        .map_err(|e| format!("写入私钥失败: {e}"))?;
    let public_path = private_path.with_extension("pub");
    std::fs::write(&public_path, public_key.trim_end())
        .map_err(|e| format!("写入公钥失败: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&private_path, std::fs::Permissions::from_mode(0o600));
    }

    let cert = Certificate {
        id,
        name: "Synced Key".to_string(),
        private_key_path: private_path.to_string_lossy().into_owned(),
        public_key,
    };
    store.add_certificate(&cert)?;
    Ok(cert)
}

#[cfg(test)]
mod tests {
    use super::SyncHost;

    #[test]
    fn old_remote_host_with_null_update_port_uses_default() {
        let host: SyncHost = serde_json::from_str(
            r#"{"id":"h1","name":"host","hostname":"10.0.0.1","updatePort":null}"#,
        )
        .unwrap();
        assert_eq!(host.update_port, 0);
    }
}
