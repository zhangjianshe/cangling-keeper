//! Sync named key pairs independently of the hosts and tunnels that refer to them.
use crate::{AppState, Certificate, login_credentials, resolve_key_path, sync};
use russh::keys::{Algorithm, PrivateKey, PublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

static SYNC_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
#[derive(Serialize, Deserialize)]
struct Baseline {
    name: String,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    uploaded: usize,
    downloaded: usize,
    renamed: usize,
}

pub(crate) fn identity(public_key: &str) -> Result<String, String> {
    let key = public_key
        .trim()
        .parse::<PublicKey>()
        .map_err(|_| "证书公钥格式无效")?;
    if key.algorithm() != Algorithm::Ed25519 {
        return Err("仅支持 Ed25519 证书同步".into());
    }
    Ok(key
        .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
        .to_string())
}
fn validate_pair(public: &str, private: &str) -> Result<(), String> {
    let key = PrivateKey::from_openssh(private).map_err(|_| "同步证书私钥格式无效")?;
    let derived = key
        .public_key()
        .to_openssh()
        .map_err(|_| "同步证书公钥无效")?;
    if identity(public)? != identity(&derived)? {
        return Err("同步证书的公钥和私钥不匹配".into());
    }
    Ok(())
}

pub async fn synchronize(state: &AppState) -> Result<Summary, String> {
    let _guard = SYNC_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let (url, token, account) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let (url, token) = login_credentials(&store).ok_or("请先登录再同步证书")?;
        let user = store
            .get_setting(sync::SETTING_USER_ID)?
            .filter(|v| !v.is_empty())
            .or(store.get_setting(sync::SETTING_USERNAME)?)
            .ok_or("无法确定登录账号")?;
        let account = format!(
            "{:x}",
            Sha256::digest(format!("{}|{user}", url.trim_end_matches('/')).as_bytes())
        );
        (url, token, account)
    };
    let mut remote = sync::pull_certificates(&url, &token).await?;
    // Validate all downloaded keys before writing any files or changing names.
    for cert in &remote {
        validate_pair(&cert.public_key, &cert.private_key)?;
    }
    let local = state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .list_certificates()?;
    let mut summary = Summary::default();
    for cert in local {
        let key = identity(&cert.public_key)?;
        let marker = format!("certificate_sync:{account}:{key}");
        let baseline = state
            .store
            .lock()
            .map_err(|e| e.to_string())?
            .get_setting(&marker)?
            .and_then(|v| serde_json::from_str::<Baseline>(&v).ok());
        let index = remote
            .iter()
            .position(|other| identity(&other.public_key).ok().as_deref() == Some(key.as_str()));
        let dirty = state
            .store
            .lock()
            .map_err(|e| e.to_string())?
            .get_setting(&format!("certificate_name_changed:{}", cert.id))?;
        let changed = baseline.as_ref().is_some_and(|b| b.name != cert.name)
            || dirty.as_deref() == Some(cert.name.as_str());
        if index.is_none() || changed {
            let path = resolve_key_path(&state.data_dir, &cert.private_key_path);
            let private = std::fs::read_to_string(path).map_err(|_| "无法读取待同步证书私钥")?;
            validate_pair(&cert.public_key, &private)?;
            let uploaded = sync::push_certificate(
                &url,
                &token,
                &sync::SyncCertificate {
                    id: String::new(),
                    name: cert.name.clone(),
                    public_key: cert.public_key.clone(),
                    private_key: private,
                },
            )
            .await?;
            validate_pair(&uploaded.public_key, &uploaded.private_key)?;
            if identity(&uploaded.public_key)? != key {
                return Err("服务器返回了错误的证书公钥".into());
            }
            let store = state.store.lock().map_err(|e| e.to_string())?;
            store.set_setting(
                &marker,
                &serde_json::to_string(&Baseline {
                    name: cert.name.clone(),
                })
                .unwrap(),
            )?;
            if store.get_certificate(&cert.id)?.name == cert.name {
                store.delete_setting(&format!("certificate_name_changed:{}", cert.id))?;
            }
            summary.uploaded += 1;
        } else if let Some(index) = index {
            let other = &remote[index];
            let store = state.store.lock().map_err(|e| e.to_string())?;
            // An edit made while the request was in flight must be left for the next sync.
            let current = store.get_certificate(&cert.id)?;
            if current.name == cert.name {
                if current.name != other.name {
                    store.update_certificate_name(&cert.id, &other.name)?;
                    summary.renamed += 1;
                }
                store.set_setting(
                    &marker,
                    &serde_json::to_string(&Baseline {
                        name: other.name.clone(),
                    })
                    .unwrap(),
                )?;
            }
        }
        if let Some(index) = index {
            remote.remove(index);
        }
    }
    for other in remote {
        let key = identity(&other.public_key)?;
        let store = state.store.lock().map_err(|e| e.to_string())?;
        // Recheck after network awaits: a key may have been imported by another action.
        if store
            .list_certificates()?
            .iter()
            .any(|c| identity(&c.public_key).ok().as_deref() == Some(key.as_str()))
        {
            continue;
        }
        let cert = crate::import_certificate(
            &state.data_dir.join("keys"),
            &other.name,
            &other.private_key,
        )?;
        store.add_certificate(&cert)?;
        store.set_setting(
            &format!("certificate_sync:{account}:{key}"),
            &serde_json::to_string(&Baseline { name: other.name }).unwrap(),
        )?;
        summary.downloaded += 1;
    }
    crate::logging::info(&format!(
        "证书同步完成：上传 {}，下载 {}，名称更新 {}",
        summary.uploaded, summary.downloaded, summary.renamed
    ));
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_identity_ignores_comments_and_rejects_mismatched_private_keys() {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let public = key.public_key().to_openssh().unwrap();
        let private = key
            .to_openssh(russh::keys::ssh_key::LineEnding::LF)
            .unwrap();
        assert_eq!(
            identity(&public).unwrap(),
            identity(&format!("{public} another computer")).unwrap()
        );
        assert!(validate_pair(&public, &private).is_ok());
        let other = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        assert!(validate_pair(&other.public_key().to_openssh().unwrap(), &private).is_err());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::State,
        routing::{get, post},
    };
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};
    type Catalog = Arc<Mutex<Vec<sync::SyncCertificate>>>;
    async fn list(State(catalog): State<Catalog>) -> Json<Value> {
        Json(
            json!({"code":200,"success":true,"data":{"certificates":catalog.lock().unwrap().clone()}}),
        )
    }
    async fn save(State(catalog): State<Catalog>, Json(body): Json<Value>) -> Json<Value> {
        let mut cert: sync::SyncCertificate =
            serde_json::from_value(body["certificate"].clone()).unwrap();
        cert.id = "remote-cert".into();
        let mut catalog = catalog.lock().unwrap();
        if let Some(existing) = catalog.iter_mut().find(|other| {
            identity(&other.public_key).unwrap() == identity(&cert.public_key).unwrap()
        }) {
            *existing = cert.clone();
        } else {
            catalog.push(cert.clone());
        }
        Json(json!({"code":200,"success":true,"data":{"certificate":cert}}))
    }
    #[tokio::test]
    async fn named_keys_round_trip_and_offline_rename_wins_without_duplicate_keys() {
        let catalog: Catalog = Arc::new(Mutex::new(Vec::new()));
        let router = Router::new()
            .route("/api/v1/certificate/list", get(list))
            .route("/api/v1/certificate/save", post(save))
            .with_state(catalog.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let dir =
            std::env::temp_dir().join(format!("keeper-cert-roundtrip-{}", uuid::Uuid::new_v4()));
        let a = crate::create_app_state(dir.join("a")).unwrap();
        let b = crate::create_app_state(dir.join("b")).unwrap();
        for state in [&a, &b] {
            let store = state.store.lock().unwrap();
            store.set_setting(sync::SETTING_SERVER_URL, &url).unwrap();
            store.set_setting(sync::SETTING_TOKEN, "test-only").unwrap();
            store.set_setting(sync::SETTING_USER_ID, "123").unwrap();
        }
        let original = a.store.lock().unwrap().list_certificates().unwrap()[0].clone();
        a.store
            .lock()
            .unwrap()
            .update_certificate_name(&original.id, "河南维护证书")
            .unwrap();
        assert_eq!(synchronize(&a).await.unwrap().uploaded, 1);
        assert_eq!(synchronize(&b).await.unwrap().downloaded, 1);
        let imported = b
            .store
            .lock()
            .unwrap()
            .list_certificates()
            .unwrap()
            .into_iter()
            .find(|c| identity(&c.public_key).unwrap() == identity(&original.public_key).unwrap())
            .unwrap();
        assert_eq!(imported.name, "河南维护证书");
        let before =
            std::fs::read(resolve_key_path(&b.data_dir, &imported.private_key_path)).unwrap();
        b.store
            .lock()
            .unwrap()
            .update_certificate_name(&imported.id, "河南新名称")
            .unwrap();
        assert_eq!(synchronize(&b).await.unwrap().uploaded, 1);
        synchronize(&a).await.unwrap();
        assert_eq!(
            a.store
                .lock()
                .unwrap()
                .get_certificate(&original.id)
                .unwrap()
                .name,
            "河南新名称"
        );
        synchronize(&b).await.unwrap();
        assert_eq!(
            std::fs::read(resolve_key_path(&b.data_dir, &imported.private_key_path)).unwrap(),
            before
        );
        assert_eq!(catalog.lock().unwrap().len(), 2); // Each computer has its own default key.
        let imported = b
            .store
            .lock()
            .unwrap()
            .list_certificates()
            .unwrap()
            .into_iter()
            .find(|c| identity(&c.public_key).unwrap() == identity(&original.public_key).unwrap())
            .unwrap();
        assert_eq!(imported.name, "河南新名称");
        assert_eq!(
            b.store.lock().unwrap().list_certificates().unwrap().len(),
            2
        );
        a.store
            .lock()
            .unwrap()
            .update_certificate_name(&original.id, "河南维护证书（更新）")
            .unwrap();
        synchronize(&a).await.unwrap();
        synchronize(&b).await.unwrap();
        assert_eq!(
            b.store
                .lock()
                .unwrap()
                .get_certificate(&imported.id)
                .unwrap()
                .name,
            "河南维护证书（更新）"
        );
        assert_eq!(
            b.store.lock().unwrap().list_certificates().unwrap().len(),
            2
        );
        drop(a);
        drop(b);
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
