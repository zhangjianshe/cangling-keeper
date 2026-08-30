use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use russh_sftp::client::SftpSession;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::io::AsyncWriteExt;

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
    pub sets: Vec<String>,
    pub incomplete_sets: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSoftwareSetPreview {
    pub name: String,
    pub files: u32,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSoftwareSyncPreview {
    pub sets: Vec<HostSoftwareSetPreview>,
    pub incomplete_sets: Vec<String>,
    pub total_files: u32,
    pub total_bytes: u64,
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

fn collect_set_names(files: &[LocalFile]) -> Vec<String> {
    summarize_sets(files).into_iter().map(|s| s.name).collect()
}

fn summarize_sets(files: &[LocalFile]) -> Vec<HostSoftwareSetPreview> {
    let mut out: Vec<HostSoftwareSetPreview> = Vec::new();
    for f in files {
        let Some(name) = f.rel.split('/').next().filter(|s| !s.is_empty()) else {
            continue;
        };
        if let Some(last) = out.last_mut() {
            if last.name == name {
                last.files += 1;
                last.bytes = last.bytes.saturating_add(f.size);
                continue;
            }
        }
        out.push(HostSoftwareSetPreview {
            name: name.to_string(),
            files: 1,
            bytes: f.size,
        });
    }
    out
}

fn incomplete_set_names(data_dir: &Path, present: &[String], configured: &[String]) -> Vec<String> {
    let present: HashSet<&str> = present.iter().map(String::as_str).collect();
    let mut names = Vec::new();
    for name in configured {
        if !present.contains(name.as_str()) {
            names.push(name.clone());
        }
    }
    let root = crate::repo::sets_root(data_dir);
    if let Ok(rd) = std::fs::read_dir(&root) {
        for item in rd.flatten() {
            let path = item.path();
            if !path.is_dir() {
                continue;
            }
            let name = item.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || present.contains(name.as_str()) {
                continue;
            }
            if !names.iter().any(|n| n == &name) {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}

fn configured_set_names(state: &AppState, data_dir: &Path) -> Vec<String> {
    let Ok(store) = state.store.lock() else {
        return Vec::new();
    };
    crate::repo::list_configured_set_names(&store, data_dir).unwrap_or_default()
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

fn sh_quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b":/._-+=".contains(&b))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    }
}

fn is_path_install_dir(dir: &str) -> bool {
    matches!(
        dir,
        "/usr/local/bin" | "/usr/bin" | "/bin" | "/usr/local/sbin" | "/usr/sbin" | "/sbin"
    )
}

fn remote_repo_from_binary(binary: &str) -> String {
    let b = binary.trim();
    if let Some(dir) = unix_parent(b) {
        // install-service also drops a symlink at /usr/local/bin/cangling-update.
        // dirname of that path is not the software repo.
        if !dir.is_empty() && dir != "/" && !is_path_install_dir(dir) {
            return join_remote(dir, REMOTE_REPO_NAME);
        }
    }
    "/root/update/repo".into()
}

/// Same-size files larger than this are treated as unchanged. Hashing GB
/// package images (k3s, docker) on the host is as slow as uploading them;
/// size changes when those packages actually change. Small files such as
/// `version.txt` can keep the same length after a bump, so they are hashed.
const HASH_MAX_BYTES: u64 = 256 * 1024;

fn parse_remote_sizes(stdout: &str) -> HashMap<String, u64> {
    let mut map = HashMap::new();
    for line in stdout.lines() {
        if line.starts_with("CK_REPO\t") {
            continue;
        }
        let Some((sz, rel)) = line.split_once('\t') else {
            continue;
        };
        let Ok(n) = sz.trim().parse::<u64>() else {
            continue;
        };
        let rel = rel.trim().trim_start_matches("./").replace('\\', "/");
        if !rel.is_empty() {
            map.insert(rel, n);
        }
    }
    map
}

fn parse_inventory_root(stdout: &str, fallback: &str) -> String {
    stdout
        .lines()
        .find_map(|line| {
            line.strip_prefix("CK_REPO\t")
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty())
        })
        .unwrap_or_else(|| fallback.to_string())
}

fn parse_sha256_list(stdout: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in stdout.lines() {
        if line.len() < 66 {
            continue;
        }
        let hash = line[..64].trim();
        if !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let path = line[64..]
            .trim()
            .trim_start_matches('*')
            .trim()
            .trim_start_matches("./")
            .replace('\\', "/");
        if !path.is_empty() {
            map.insert(path, hash.to_ascii_lowercase());
        }
    }
    map
}

fn remote_inventory_cmd(preferred: &str) -> String {
    let preferred = sh_quote(preferred);
    format!(
        r#"home="${{HOME:-/root}}"
pref={preferred}
# Use the repo next to the real cangling-update binary. install-service
# also drops a symlink at /usr/local/bin/cangling-update; dirname of that
# path (/usr/local/bin/repo) is not the software repo and must not win
# just because it happens to contain leftover files.
if [ -n "$pref" ]; then
  best="$pref"
else
  best="$home/update/repo"
fi
printf 'CK_REPO\t%s\n' "$best"
if [ -d "$best" ]; then
  cd "$best" || exit 0
  if find . -maxdepth 0 -printf '' >/dev/null 2>&1; then
    find . -type f ! -name '*.part' ! -path '*/.git/*' -printf '%s\t%P\n'
  else
    find . -type f ! -name '*.part' ! -path '*/.git/*' -exec stat -c '%s\t%n' {{}} + | sed 's/\t\.\//\t/'
  fi
fi
"#,
        preferred = preferred
    )
}

fn remote_hash_cmd(root: &str, rels: &[String]) -> String {
    let mut cmd = format!("cd {} && sha256sum --", sh_quote(root));
    for rel in rels {
        cmd.push(' ');
        cmd.push_str(&sh_quote(rel));
    }
    cmd
}

fn same_size(local: u64, remote: Option<&u64>) -> bool {
    remote.copied() == Some(local)
}

async fn remote_skip_set(
    session: &russh::client::Handle<crate::ssh::SshClient>,
    preferred_root: &str,
    files: &[LocalFile],
    app: &AppHandle,
    host_id: &str,
) -> (String, HashSet<String>) {
    let fallback = if preferred_root.trim().is_empty() {
        "/root/update/repo".to_string()
    } else {
        preferred_root.to_string()
    };
    let mut skip = HashSet::new();
    let listing = ssh::execute_on(session, &remote_inventory_cmd(&fallback)).await;
    let (remote_root, sizes) = match listing {
        Ok(out) => (
            parse_inventory_root(&out.stdout, &fallback),
            parse_remote_sizes(&out.stdout),
        ),
        Err(_) => return (fallback, skip),
    };

    let mut hash_candidates = Vec::new();
    for f in files {
        if !same_size(f.size, sizes.get(&f.rel)) {
            continue;
        }
        if f.size > HASH_MAX_BYTES {
            skip.insert(f.rel.clone());
        } else {
            hash_candidates.push(f);
        }
    }

    if hash_candidates.is_empty() {
        return (remote_root, skip);
    }

    emit_progress(
        app,
        &SyncProgress {
            host_id: host_id.to_string(),
            current: 0,
            total: files.len() as u32,
            file: "正在比对小文件…".into(),
            action: "compare".into(),
            bytes_done: 0,
            bytes_total: 0,
            overall_done: 0,
            overall_total: 0,
            remote_path: remote_root.clone(),
        },
    );

    let mut remote_hashes = HashMap::new();
    const BATCH: usize = 80;
    for chunk in hash_candidates.chunks(BATCH) {
        let rels: Vec<String> = chunk.iter().map(|f| f.rel.clone()).collect();
        if let Ok(out) = ssh::execute_on(session, &remote_hash_cmd(&remote_root, &rels)).await {
            // sha256sum exits non-zero if any path is missing; keep the hashes
            // it did print so the rest of the batch can still skip.
            remote_hashes.extend(parse_sha256_list(&out.stdout));
        }
    }

    for f in hash_candidates {
        let Some(remote_hash) = remote_hashes.get(&f.rel) else {
            continue;
        };
        let Ok(local_hash) = crate::repo::sha256_file(&f.abs) else {
            continue;
        };
        if local_hash.eq_ignore_ascii_case(remote_hash) {
            skip.insert(f.rel.clone());
        }
    }
    (remote_root, skip)
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
        return Err("本地没有已拉取完成的软件，请先在「软件仓库」同步软件集".into());
    }
    let sets = collect_set_names(&files);
    let configured = configured_set_names(&state, &data_dir);
    let incomplete_sets = incomplete_set_names(&data_dir, &sets, &configured);

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

    let preferred_root = if binary.trim().is_empty() {
        String::new()
    } else {
        remote_repo_from_binary(&binary)
    };

    emit_progress(
        &app,
        &SyncProgress {
            host_id: host_id.clone(),
            current: 0,
            total: files.len() as u32,
            file: "正在比对远端已有文件…".into(),
            action: "compare".into(),
            bytes_done: 0,
            bytes_total: 0,
            overall_done: 0,
            overall_total: files.iter().map(|f| f.size).sum(),
            remote_path: preferred_root.clone(),
        },
    );
    let (remote_root, skip) =
        remote_skip_set(&session, &preferred_root, &files, &app, &host_id).await;

    let total = files.len() as u32;
    let overall_total: u64 = files.iter().map(|f| f.size).sum();
    if skip.len() == files.len() {
        emit_progress(
            &app,
            &SyncProgress {
                host_id: host_id.clone(),
                current: total,
                total,
                file: "全部文件未改动，已跳过".into(),
                action: "skip".into(),
                bytes_done: overall_total,
                bytes_total: overall_total,
                overall_done: overall_total,
                overall_total,
                remote_path: remote_root.clone(),
            },
        );
        drop(session);
        return Ok(HostSoftwareSyncResult {
            remote_path: remote_root,
            total_files: total,
            uploaded: 0,
            skipped: total,
            failed: 0,
            error: String::new(),
            sets,
            incomplete_sets,
        });
    }

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
    ensure_remote_dir(&sftp, &remote_root).await?;

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

        if skip.contains(&file.rel) {
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
        sets,
        incomplete_sets,
    })
}

#[tauri::command]
pub fn preview_host_software_sync(
    state: State<'_, AppState>,
) -> Result<HostSoftwareSyncPreview, String> {
    let files = collect_local_software(&state.data_dir)?;
    let sets = summarize_sets(&files);
    let names: Vec<String> = sets.iter().map(|s| s.name.clone()).collect();
    let configured = configured_set_names(&state, &state.data_dir);
    let incomplete_sets = incomplete_set_names(&state.data_dir, &names, &configured);
    Ok(HostSoftwareSyncPreview {
        total_files: files.len() as u32,
        total_bytes: files.iter().map(|f| f.size).sum(),
        sets,
        incomplete_sets,
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
        assert_eq!(
            collect_set_names(&files),
            ["cangling-repo".to_string(), "np4".to_string()]
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn collect_local_software_includes_np4_and_git_set() {
        let tmp = std::env::temp_dir().join(format!("ck-sets-{}", uuid::Uuid::new_v4()));
        let root = tmp.join("software-sets");
        std::fs::create_dir_all(root.join("np4/np4-jars/latest/all platform")).unwrap();
        std::fs::create_dir_all(root.join("cangling-repo/linux-x86")).unwrap();
        std::fs::write(
            root.join("np4/np4-jars/latest/all platform/app.jar"),
            b"jar",
        )
        .unwrap();
        std::fs::write(root.join("cangling-repo/linux-x86/pkg.rpm"), b"rpm").unwrap();
        let files = collect_local_software(&tmp).unwrap();
        let rels: Vec<_> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(
            rels,
            [
                "cangling-repo/linux-x86/pkg.rpm",
                "np4/np4-jars/latest/all platform/app.jar",
            ]
        );
        assert_eq!(
            collect_set_names(&files),
            ["cangling-repo".to_string(), "np4".to_string()]
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn collect_skips_part_files_and_reports_incomplete_np4() {
        let tmp = std::env::temp_dir().join(format!("ck-sets-part-{}", uuid::Uuid::new_v4()));
        let root = tmp.join("software-sets");
        std::fs::create_dir_all(root.join("np4/np4-jars/latest/all platform/arm64+x86")).unwrap();
        std::fs::create_dir_all(root.join("cangling-repo/linux-x86")).unwrap();
        std::fs::write(
            root.join("np4/np4-jars/latest/all platform/arm64+x86/cis-map-1.0.0.part"),
            b"partial",
        )
        .unwrap();
        std::fs::write(root.join("cangling-repo/linux-x86/pkg.rpm"), b"rpm").unwrap();
        let files = collect_local_software(&tmp).unwrap();
        assert_eq!(
            files.iter().map(|f| f.rel.as_str()).collect::<Vec<_>>(),
            ["cangling-repo/linux-x86/pkg.rpm"]
        );
        let names = collect_set_names(&files);
        assert_eq!(names, ["cangling-repo".to_string()]);
        assert_eq!(
            incomplete_set_names(&tmp, &names, &["cangling-repo".into(), "np4".into()]),
            ["np4".to_string()]
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn inventory_stays_on_preferred_repo() {
        let cmd = remote_inventory_cmd("/root/update/repo");
        assert!(cmd.contains("best=\"$pref\""));
        assert!(!cmd.contains("for d in"));
        assert!(cmd.contains("pref=/root/update/repo"));
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

    #[test]
    fn parse_remote_sizes_and_hashes() {
        let sizes = parse_remote_sizes(
            "CK_REPO\t/root/update/repo\n12\tnp4/version.txt\n100\tcangling-repo/a.rpm\nbad\n",
        );
        assert_eq!(sizes.get("np4/version.txt").copied(), Some(12));
        assert_eq!(sizes.get("cangling-repo/a.rpm").copied(), Some(100));
        assert_eq!(
            parse_inventory_root(
                "CK_REPO\t/root/update/repo\n12\tnp4/version.txt\n",
                "/fallback"
            ),
            "/root/update/repo"
        );
        let hashes = parse_sha256_list(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  np4/version.txt\n",
        );
        assert_eq!(
            hashes.get("np4/version.txt").map(String::as_str),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }

    #[test]
    fn remote_repo_from_symlink_path() {
        assert_eq!(
            remote_repo_from_binary("/root/update/cangling-update"),
            "/root/update/repo"
        );
        assert_eq!(
            remote_repo_from_binary("/usr/local/bin/cangling-update"),
            "/root/update/repo"
        );
        assert_eq!(
            remote_repo_from_binary("/opt/cangling-update/cangling-update"),
            "/opt/cangling-update/repo"
        );
    }

    #[test]
    fn large_same_size_skips_without_hash() {
        assert!(HASH_MAX_BYTES < 1024 * 1024);
        assert!(same_size(3_000_000_000, Some(&3_000_000_000)));
        assert!(!same_size(3_000_000_000, Some(&2_999_999_999)));
        assert!(!same_size(12, None));
    }
}
