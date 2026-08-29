use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use russh_sftp::client::SftpSession;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::ssh;
use crate::{AppState, resolve_auth};

const SKIP_DIRS: &[&str] = &[".git"];
const REMOTE_REPO_NAME: &str = "repo";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSoftwareSyncResult {
    pub remote_path: String,
    pub total_files: u32,
    pub uploaded: u32,
    pub skipped: u32,
    pub failed: u32,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncProgress {
    host_id: String,
    current: u32,
    total: u32,
    file: String,
    action: String,
    bytes_done: u64,
    bytes_total: u64,
    overall_done: u64,
    overall_total: u64,
    remote_path: String,
}

struct LocalFile {
    rel: String,
    abs: PathBuf,
    size: u64,
}

fn unix_parent(path: &str) -> Option<&str> {
    let path = path.trim_end_matches('/');
    path.rsplit_once('/').map(|(p, _)| p)
}

fn join_remote(root: &str, rel: &str) -> String {
    let root = root.trim_end_matches('/');
    let rel = rel.trim_start_matches('/');
    if rel.is_empty() {
        root.to_string()
    } else {
        format!("{root}/{rel}")
    }
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<LocalFile>) -> Result<(), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("读取目录失败：{e}"))?;
    for item in rd {
        let item = item.map_err(|e| e.to_string())?;
        let path = item.path();
        let name = item.file_name().to_string_lossy().into_owned();
        if SKIP_DIRS.iter().any(|d| *d == name) {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, out)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        if name.ends_with(".part") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if rel.is_empty() {
            continue;
        }
        let size = item.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(LocalFile {
            rel,
            abs: path,
            size,
        });
    }
    Ok(())
}

fn collect_local_software(data_dir: &Path) -> Result<Vec<LocalFile>, String> {
    let root = crate::repo::sets_root(data_dir);
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    let rd = std::fs::read_dir(&root).map_err(|e| format!("读取本地软件集失败：{e}"))?;
    for item in rd {
        let item = item.map_err(|e| e.to_string())?;
        let path = item.path();
        if path.is_dir() {
            // Keep the software-set directory in the relative path so two
            // sets cannot overwrite each other at the same remote name.
            collect_files(&root, &path, &mut files)?;
        }
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(files)
}

fn emit_progress(app: &AppHandle, p: &SyncProgress) {
    let _ = app.emit("host-software-sync-progress", p);
}

async fn ensure_remote_dir(sftp: &SftpSession, path: &str) -> Result<(), String> {
    let path = path.trim_end_matches('/');
    if path.is_empty() || path == "/" {
        return Ok(());
    }
    if sftp.try_exists(path).await.unwrap_or(false) {
        return Ok(());
    }
    if let Some(parent) = unix_parent(path) {
        Box::pin(ensure_remote_dir(sftp, parent)).await?;
    }
    match sftp.create_dir(path).await {
        Ok(()) => Ok(()),
        Err(e) => {
            if sftp.try_exists(path).await.unwrap_or(false) {
                Ok(())
            } else {
                Err(format!("创建远端目录 {path} 失败：{e}"))
            }
        }
    }
}

async fn remote_repo_path(sftp: &SftpSession, binary: &str) -> Result<String, String> {
    if !binary.trim().is_empty() {
        if let Some(dir) = unix_parent(binary.trim()) {
            if !dir.is_empty() {
                return Ok(join_remote(dir, REMOTE_REPO_NAME));
            }
        }
    }
    let home = sftp
        .canonicalize(".")
        .await
        .unwrap_or_else(|_| "/root".into());
    Ok(join_remote(
        &format!("{}/update", home.trim_end_matches('/')),
        REMOTE_REPO_NAME,
    ))
}

async fn sha256_remote(sftp: &SftpSession, path: &str) -> Result<String, String> {
    let mut file = sftp
        .open(path)
        .await
        .map_err(|e| format!("读取远端失败：{e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(crate::repo::hex_encode(&hasher.finalize()))
}

/// Unchanged only when size and SHA-256 both match. Size-only is not enough:
/// files like `version.txt` often keep the same length after a version bump.
async fn remote_unchanged(sftp: &SftpSession, remote: &str, local: &LocalFile) -> bool {
    let Ok(meta) = sftp.metadata(remote).await else {
        return false;
    };
    if meta.size != Some(local.size) {
        return false;
    }
    let Ok(local_hash) = crate::repo::sha256_file(&local.abs) else {
        return false;
    };
    match sha256_remote(sftp, remote).await {
        Ok(remote_hash) => remote_hash.eq_ignore_ascii_case(&local_hash),
        Err(_) => false,
    }
}

async fn upload_file(
    sftp: &SftpSession,
    local: &Path,
    remote: &str,
    size: u64,
    mut on_bytes: impl FnMut(u64),
) -> Result<(), String> {
    if let Some(parent) = unix_parent(remote) {
        ensure_remote_dir(sftp, parent).await?;
    }
    let mut src = std::fs::File::open(local).map_err(|e| format!("读取本地文件失败：{e}"))?;
    let mut dest = sftp
        .create(remote)
        .await
        .map_err(|e| format!("创建远端文件失败：{e}"))?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = src.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        dest.write_all(&buf[..n])
            .await
            .map_err(|e| format!("写入远端失败：{e}"))?;
        done += n as u64;
        on_bytes(done);
    }
    dest.shutdown()
        .await
        .map_err(|e| format!("关闭远端文件失败：{e}"))?;
    let _ = size;
    Ok(())
}

#[tauri::command]
pub async fn sync_host_software(
    app: AppHandle,
    state: State<'_, AppState>,
    host_id: String,
) -> Result<HostSoftwareSyncResult, String> {
    let data_dir = state.data_dir.clone();
    let files = collect_local_software(&data_dir)?;
    if files.is_empty() {
        return Err("本地没有已拉取的软件，请先在「软件仓库」同步软件集".into());
    }

    let (host, auth) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let host = store.get_host(&host_id)?;
        let auth = resolve_auth(&store, &host.auth, &data_dir)?;
        (host, auth)
    };

    let probe = crate::host_actions::wrap_probe_command();
    let probe_out = ssh::execute(&host, &probe, &auth).await.ok();
    let (role, binary) = if let Some(out) = probe_out.as_ref() {
        crate::host_actions::parse_probe(&out.stdout)
            .map(|p| (p.role.to_ascii_lowercase(), p.binary))
            .unwrap_or_default()
    } else {
        (String::new(), String::new())
    };
    if role == "worker" {
        return Err("只能同步到 Master 主机，当前主机是 Worker".into());
    }

    let mut session = ssh::connect(&host.hostname, host.port).await?;
    ssh::authenticate(&mut session, &host.username, &auth).await?;
    let channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("打开 SSH 会话失败：{e}"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("请求 SFTP 失败：{e}"))?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| format!("初始化 SFTP 失败：{e}"))?;
    sftp.set_timeout(30 * 60);

    let remote_root = remote_repo_path(&sftp, &binary).await?;
    ensure_remote_dir(&sftp, &remote_root).await?;

    let total = files.len() as u32;
    let overall_total: u64 = files.iter().map(|f| f.size).sum();
    let mut uploaded = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;
    let mut last_error = String::new();
    let mut overall_done = 0u64;

    for (index, file) in files.iter().enumerate() {
        let current = (index as u32) + 1;
        let remote = join_remote(&remote_root, &file.rel);
        let mut progress = SyncProgress {
            host_id: host_id.clone(),
            current,
            total,
            file: file.rel.clone(),
            action: "upload".into(),
            bytes_done: 0,
            bytes_total: file.size,
            overall_done,
            overall_total,
            remote_path: remote_root.clone(),
        };

        let same = remote_unchanged(&sftp, &remote, file).await;
        if same {
            skipped += 1;
            overall_done = overall_done.saturating_add(file.size);
            progress.action = "skip".into();
            progress.bytes_done = file.size;
            progress.overall_done = overall_done;
            emit_progress(&app, &progress);
            continue;
        }

        emit_progress(&app, &progress);
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let upload = upload_file(&sftp, &file.abs, &remote, file.size, |done| {
            let now = Instant::now();
            if now.duration_since(last_emit) >= Duration::from_millis(200) || done >= file.size {
                last_emit = now;
                progress.bytes_done = done;
                progress.overall_done = overall_done.saturating_add(done);
                emit_progress(&app, &progress);
            }
        })
        .await;
        match upload {
            Ok(()) => {
                uploaded += 1;
                overall_done = overall_done.saturating_add(file.size);
                progress.action = "upload".into();
                progress.bytes_done = file.size;
                progress.overall_done = overall_done;
                emit_progress(&app, &progress);
            }
            Err(e) => {
                failed += 1;
                last_error = format!("{}: {e}", file.rel);
                progress.action = "fail".into();
                emit_progress(&app, &progress);
            }
        }
    }

    let _ = sftp.close().await;
    drop(session);

    if failed > 0 && uploaded == 0 && skipped == 0 {
        return Err(if last_error.is_empty() {
            "同步失败".into()
        } else {
            last_error
        });
    }

    Ok(HostSoftwareSyncResult {
        remote_path: remote_root,
        total_files: total,
        uploaded,
        skipped,
        failed,
        error: last_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tree(root: &Path) {
        std::fs::create_dir_all(root.join("np4")).unwrap();
        std::fs::create_dir_all(root.join("cangling-repo/linux-x86")).unwrap();
        std::fs::write(root.join("np4/version.txt"), "v1").unwrap();
        std::fs::write(root.join("cangling-repo/linux-x86/version.txt"), "v2").unwrap();
        std::fs::write(root.join("cangling-repo/linux-x86/pkg.rpm"), b"rpm").unwrap();
    }

    #[test]
    fn collect_keeps_software_set_prefix() {
        let tmp = std::env::temp_dir().join(format!("ck-host-sync-{}", uuid::Uuid::new_v4()));
        write_tree(&tmp);
        let mut files = Vec::new();
        collect_files(&tmp, &tmp.join("np4"), &mut files).unwrap();
        collect_files(&tmp, &tmp.join("cangling-repo"), &mut files).unwrap();
        files.sort_by(|a, b| a.rel.cmp(&b.rel));
        let rels: Vec<_> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(
            rels,
            [
                "cangling-repo/linux-x86/pkg.rpm",
                "cangling-repo/linux-x86/version.txt",
                "np4/version.txt",
            ]
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn same_size_version_txt_has_different_hash() {
        let tmp = std::env::temp_dir().join(format!("ck-host-sync-hash-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let a = tmp.join("a.txt");
        let b = tmp.join("b.txt");
        std::fs::write(&a, "v1.0.0\n").unwrap();
        std::fs::write(&b, "v1.0.1\n").unwrap();
        assert_eq!(a.metadata().unwrap().len(), b.metadata().unwrap().len());
        let ha = crate::repo::sha256_file(&a).unwrap();
        let hb = crate::repo::sha256_file(&b).unwrap();
        assert_ne!(ha, hb);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn join_remote_keeps_set_and_file() {
        assert_eq!(
            join_remote("/opt/cangling-update/repo", "np4/version.txt"),
            "/opt/cangling-update/repo/np4/version.txt"
        );
    }
}
