use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use russh_sftp::{client::SftpSession, protocol::OpenFlags};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::ssh;
use crate::{AppState, resolve_auth};

const SKIP_DIRS: &[&str] = &[".git"];
const REMOTE_REPO_NAME: &str = "repo";
const RESUMABLE_UPLOAD_MIN_BYTES: u64 = 10 * 1024 * 1024;

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

fn remote_repository_root_cmd(preferred: &str) -> String {
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
"#,
        preferred = preferred
    )
}

fn parse_remote_sizes(stdout: &str) -> Vec<(String, u64)> {
    let mut files = Vec::new();
    for line in stdout.lines() {
        let Some((size, relative)) = line.split_once('\t') else {
            continue;
        };
        let Ok(size) = size.trim().parse::<u64>() else {
            continue;
        };
        let relative = relative.trim().trim_start_matches("./").replace('\\', "/");
        if !relative.is_empty() {
            files.push((relative, size));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn parse_sha256_list(stdout: &str) -> HashMap<String, String> {
    let mut hashes = HashMap::new();
    for line in stdout.lines() {
        if line.len() < 66 {
            continue;
        }
        let hash = line[..64].trim();
        if !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let relative = line[64..]
            .trim()
            .trim_start_matches('*')
            .trim()
            .trim_start_matches("./")
            .replace('\\', "/");
        if !relative.is_empty() {
            hashes.insert(relative, hash.to_ascii_lowercase());
        }
    }
    hashes
}

fn remote_fingerprint_inventory_cmd(root: &str) -> String {
    format!(
        r#"cd {root} || exit 1
if find . -maxdepth 0 -printf '' >/dev/null 2>&1; then
  find . -type f ! -name '*.part' ! -path '*/.git/*' ! -name '{database}*' -printf '%s\t%P\n'
else
  find . -type f ! -name '*.part' ! -path '*/.git/*' ! -name '{database}*' -exec stat -c '%s\t%n' {{}} + | sed 's/\t\.\//\t/'
fi"#,
        root = sh_quote(root),
        database = crate::fingerprints::DATABASE_NAME,
    )
}

fn remote_hash_cmd(root: &str, relative_paths: &[String]) -> String {
    let mut command = format!("cd {} && sha256sum --", sh_quote(root));
    for relative in relative_paths {
        command.push(' ');
        command.push_str(&sh_quote(relative));
    }
    command
}

fn fingerprint_skip_set(
    files: &[LocalFile],
    local: &HashMap<String, crate::fingerprints::FileFingerprint>,
    remote: &HashMap<String, crate::fingerprints::FileFingerprint>,
) -> HashSet<String> {
    files
        .iter()
        .filter(|file| {
            let Some(local) = local.get(&file.rel) else {
                return false;
            };
            remote
                .get(&file.rel)
                .is_some_and(|remote| remote.sha256.eq_ignore_ascii_case(&local.sha256))
        })
        .map(|file| file.rel.clone())
        .collect()
}

async fn read_remote_fingerprint_database(
    sftp: &SftpSession,
    remote_path: &str,
) -> Result<Option<HashMap<String, crate::fingerprints::FileFingerprint>>, String> {
    if !sftp.try_exists(remote_path).await.unwrap_or(false) {
        return Ok(None);
    }
    let mut remote = sftp
        .open(remote_path)
        .await
        .map_err(|error| format!("打开主机指纹数据库失败：{error}"))?;
    let mut value = Vec::new();
    remote
        .read_to_end(&mut value)
        .await
        .map_err(|error| format!("读取主机指纹数据库失败：{error}"))?;
    if value.len() > 64 * 1024 * 1024 {
        return Err("主机指纹数据库异常（超过 64 MiB）".into());
    }
    let temporary = std::env::temp_dir().join(format!(
        "cangling-remote-fingerprints-{}.sqlite3",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&temporary, value).map_err(|error| error.to_string())?;
    let result = crate::fingerprints::read_database(&temporary).map(|files| {
        files
            .into_iter()
            .map(|file| (file.path.clone(), file))
            .collect()
    });
    let _ = std::fs::remove_file(&temporary);
    result.map(Some)
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

fn uses_resumable_upload(size: u64) -> bool {
    size > RESUMABLE_UPLOAD_MIN_BYTES
}

async fn upload_software_file(
    session: &russh::client::Handle<crate::ssh::SshClient>,
    sftp: &SftpSession,
    local: &Path,
    remote: &str,
    size: u64,
    expected_sha256: &str,
    mut on_bytes: impl FnMut(u64),
) -> Result<(), String> {
    if !uses_resumable_upload(size) {
        return upload_file(sftp, local, remote, size, on_bytes).await;
    }
    if let Some(parent) = unix_parent(remote) {
        ensure_remote_dir(sftp, parent).await?;
    }

    let remote_part = format!("{remote}.part");
    let mut offset = match sftp.metadata(&remote_part).await {
        Ok(metadata) => metadata.size.unwrap_or(0),
        Err(_) => 0,
    };
    if offset > size {
        let _ = sftp.remove_file(&remote_part).await;
        offset = 0;
    }

    if offset < size {
        let mut source =
            std::fs::File::open(local).map_err(|error| format!("读取本地文件失败：{error}"))?;
        source
            .seek(SeekFrom::Start(offset))
            .map_err(|error| format!("定位本地续传位置失败：{error}"))?;
        let mut destination = sftp
            .open_with_flags(&remote_part, OpenFlags::CREATE | OpenFlags::WRITE)
            .await
            .map_err(|error| format!("打开远端续传文件失败：{error}"))?;
        destination
            .seek(SeekFrom::Start(offset))
            .await
            .map_err(|error| format!("定位远端续传位置失败：{error}"))?;
        on_bytes(offset);
        let mut buffer = vec![0_u8; 64 * 1024];
        let mut done = offset;
        loop {
            let count = source
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            destination
                .write_all(&buffer[..count])
                .await
                .map_err(|error| format!("写入远端续传文件失败：{error}"))?;
            done += count as u64;
            on_bytes(done);
        }
        destination
            .shutdown()
            .await
            .map_err(|error| format!("关闭远端续传文件失败：{error}"))?;
    } else {
        on_bytes(offset);
    }

    let verify_command = format!("sha256sum -- {}", sh_quote(&remote_part));
    let output = ssh::execute_on(session, &verify_command)
        .await
        .map_err(|error| format!("校验远端续传文件失败：{error}"))?;
    let actual_sha256 = output.stdout.split_whitespace().next().unwrap_or_default();
    if output.exit_status != 0 || !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
        let _ = sftp.remove_file(&remote_part).await;
        return Err(format!(
            "远端文件 SHA-256 校验失败，已清除损坏分片：{remote}"
        ));
    }
    let replace_command = format!("mv -f -- {} {}", sh_quote(&remote_part), sh_quote(remote));
    let output = ssh::execute_on(session, &replace_command)
        .await
        .map_err(|error| format!("完成远端断点续传失败：{error}"))?;
    if output.exit_status != 0 {
        return Err(format!("完成远端断点续传失败：{}", output.stderr.trim()));
    }
    Ok(())
}

async fn publish_remote_fingerprint_database(
    session: &russh::client::Handle<crate::ssh::SshClient>,
    sftp: &SftpSession,
    local_path: &Path,
    remote_path: &str,
) -> Result<(), String> {
    let remote_part = format!("{remote_path}.part");
    let database_size = local_path
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    upload_file(sftp, local_path, &remote_part, database_size, |_| {}).await?;
    let replace_command = format!(
        "mv -f -- {} {}",
        sh_quote(&remote_part),
        sh_quote(remote_path)
    );
    let output = ssh::execute_on(session, &replace_command)
        .await
        .map_err(|error| format!("更新主机指纹数据库失败：{error}"))?;
    if output.exit_status != 0 {
        return Err(format!("更新主机指纹数据库失败：{}", output.stderr.trim()));
    }
    Ok(())
}

async fn initialize_remote_fingerprint_database(
    app: &AppHandle,
    host_id: &str,
    session: &russh::client::Handle<crate::ssh::SshClient>,
    sftp: &SftpSession,
    remote_root: &str,
    local_working_path: &Path,
    remote_database_path: &str,
) -> Result<HashMap<String, crate::fingerprints::FileFingerprint>, String> {
    let inventory = ssh::execute_on(session, &remote_fingerprint_inventory_cmd(remote_root))
        .await
        .map_err(|error| format!("读取旧主机软件目录失败：{error}"))?;
    if inventory.exit_status != 0 {
        return Err(format!(
            "读取旧主机软件目录失败：{}",
            inventory.stderr.trim()
        ));
    }
    let files = parse_remote_sizes(&inventory.stdout);
    let total = files.len() as u32;
    let overall_total: u64 = files.iter().map(|(_, size)| *size).sum();
    emit_progress(
        app,
        &SyncProgress {
            host_id: host_id.to_string(),
            current: 0,
            total,
            file: "旧主机首次同步，正在初始化软件指纹库…".into(),
            action: "index".into(),
            bytes_done: 0,
            bytes_total: 0,
            overall_done: 0,
            overall_total,
            remote_path: remote_root.to_string(),
        },
    );

    let sizes: HashMap<&str, u64> = files
        .iter()
        .map(|(path, size)| (path.as_str(), *size))
        .collect();
    let mut completed = 0_usize;
    let mut overall_done = 0_u64;
    let mut fingerprints = HashMap::new();
    const HASH_BATCH_SIZE: usize = 40;
    for chunk in files.chunks(HASH_BATCH_SIZE) {
        let relative_paths: Vec<String> = chunk.iter().map(|(path, _)| path.clone()).collect();
        let output = ssh::execute_on(session, &remote_hash_cmd(remote_root, &relative_paths))
            .await
            .map_err(|error| format!("初始化主机指纹失败：{error}"))?;
        let hashes = parse_sha256_list(&output.stdout);
        for relative in &relative_paths {
            let sha256 = hashes
                .get(relative)
                .cloned()
                .ok_or_else(|| format!("无法计算旧主机文件指纹：{relative}"))?;
            let size = sizes.get(relative.as_str()).copied().unwrap_or(0);
            fingerprints.insert(
                relative.clone(),
                crate::fingerprints::FileFingerprint {
                    path: relative.clone(),
                    size,
                    modified_ns: 0,
                    sha256,
                },
            );
            completed += 1;
            overall_done = overall_done.saturating_add(size);
        }
        emit_progress(
            app,
            &SyncProgress {
                host_id: host_id.to_string(),
                current: completed as u32,
                total,
                file: format!("正在初始化主机软件指纹库（{completed}/{}）", files.len()),
                action: "index".into(),
                bytes_done: 0,
                bytes_total: 0,
                overall_done,
                overall_total,
                remote_path: remote_root.to_string(),
            },
        );
    }

    let mut rows: Vec<_> = fingerprints.values().cloned().collect();
    rows.sort_by(|left, right| left.path.cmp(&right.path));
    crate::fingerprints::write_database(local_working_path, &rows)
        .map_err(|error| format!("创建主机指纹数据库失败：{error}"))?;
    publish_remote_fingerprint_database(session, sftp, local_working_path, remote_database_path)
        .await
        .map_err(|error| format!("创建主机指纹数据库失败：{error}"))?;
    Ok(fingerprints)
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
    let repository_root = crate::repo::sets_root(&data_dir);
    let local_database = tauri::async_runtime::spawn_blocking(move || {
        crate::fingerprints::refresh_database(&repository_root)
    })
    .await
    .map_err(|error| format!("生成本地指纹数据库失败：{error}"))??;
    let local_fingerprints: HashMap<String, crate::fingerprints::FileFingerprint> = local_database
        .files
        .iter()
        .cloned()
        .map(|file| (file.path.clone(), file))
        .collect();
    if files
        .iter()
        .any(|file| !local_fingerprints.contains_key(&file.rel))
    {
        return Err("本地指纹数据库不完整，请重新同步软件集".into());
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

    let preferred_root = if binary.trim().is_empty() {
        String::new()
    } else {
        remote_repo_from_binary(&binary)
    };

    let fallback_root = if preferred_root.trim().is_empty() {
        "/root/update/repo".to_string()
    } else {
        preferred_root.clone()
    };
    let remote_root =
        match ssh::execute_on(&session, &remote_repository_root_cmd(&fallback_root)).await {
            Ok(output) => parse_inventory_root(&output.stdout, &fallback_root),
            Err(_) => fallback_root,
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
            remote_path: remote_root.clone(),
        },
    );

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
    let remote_database_path = join_remote(&remote_root, crate::fingerprints::DATABASE_NAME);
    let remote_working_database = std::env::temp_dir().join(format!(
        "cangling-host-fingerprints-{}.sqlite3",
        uuid::Uuid::new_v4()
    ));
    let mut remote_fingerprints =
        match read_remote_fingerprint_database(&sftp, &remote_database_path).await {
            Ok(Some(fingerprints)) => fingerprints,
            Ok(None) | Err(_) => {
                initialize_remote_fingerprint_database(
                    &app,
                    &host_id,
                    &session,
                    &sftp,
                    &remote_root,
                    &remote_working_database,
                    &remote_database_path,
                )
                .await?
            }
        };
    let skip = fingerprint_skip_set(&files, &local_fingerprints, &remote_fingerprints);

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
        let fingerprint = local_fingerprints
            .get(&file.rel)
            .cloned()
            .ok_or_else(|| format!("缺少 {} 的本地指纹", file.rel))?;
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let upload = upload_software_file(
            &session,
            &sftp,
            &file.abs,
            &remote,
            file.size,
            &fingerprint.sha256,
            |done| {
                let now = Instant::now();
                if now.duration_since(last_emit) >= Duration::from_millis(200) || done >= file.size
                {
                    last_emit = now;
                    progress.bytes_done = done;
                    progress.overall_done = overall_done.saturating_add(done);
                    emit_progress(&app, &progress);
                }
            },
        )
        .await;
        match upload {
            Ok(()) => {
                remote_fingerprints.insert(file.rel.clone(), fingerprint);
                let mut remote_rows: Vec<_> = remote_fingerprints.values().cloned().collect();
                remote_rows.sort_by(|left, right| left.path.cmp(&right.path));
                crate::fingerprints::write_database(&remote_working_database, &remote_rows)
                    .map_err(|error| format!("更新主机指纹数据库失败：{error}"))?;
                publish_remote_fingerprint_database(
                    &session,
                    &sftp,
                    &remote_working_database,
                    &remote_database_path,
                )
                .await?;
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

    if failed == 0 {
        publish_remote_fingerprint_database(
            &session,
            &sftp,
            &local_database.path,
            &remote_database_path,
        )
        .await
        .map_err(|error| format!("同步主机指纹数据库失败：{error}"))?;
    }
    let _ = std::fs::remove_file(&remote_working_database);

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
    fn repository_root_stays_on_preferred_repo() {
        let cmd = remote_repository_root_cmd("/root/update/repo");
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
    fn parses_remote_repository_root() {
        assert_eq!(
            parse_inventory_root("CK_REPO\t/root/update/repo\n", "/fallback"),
            "/root/update/repo"
        );
        assert_eq!(parse_inventory_root("unexpected", "/fallback"), "/fallback");
    }

    #[test]
    fn parses_legacy_host_files_for_fingerprint_initialization() {
        let files = parse_remote_sizes("7\tnp4/version.txt\n3000000000\tnp4/image.tar.gz\n");
        assert_eq!(
            files,
            vec![
                ("np4/image.tar.gz".to_string(), 3_000_000_000),
                ("np4/version.txt".to_string(), 7),
            ]
        );
        let hashes = parse_sha256_list(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  np4/version.txt\n",
        );
        assert_eq!(
            hashes.get("np4/version.txt").map(String::as_str),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        let command = remote_fingerprint_inventory_cmd("/root/update/repo");
        assert!(command.contains("! -path '*/.git/*'"));
        assert!(command.contains(crate::fingerprints::DATABASE_NAME));
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
    fn fingerprint_comparison_detects_same_size_content_change() {
        let files = vec![LocalFile {
            rel: "np4/app.jar".into(),
            abs: PathBuf::from("/unused/app.jar"),
            size: 3_000_000_000,
        }];
        let local_file = crate::fingerprints::FileFingerprint {
            path: files[0].rel.clone(),
            size: files[0].size,
            modified_ns: 1,
            sha256: "a".repeat(64),
        };
        let local = HashMap::from([(local_file.path.clone(), local_file.clone())]);
        let identical = HashMap::from([(local_file.path.clone(), local_file.clone())]);
        assert!(fingerprint_skip_set(&files, &local, &identical).contains(&files[0].rel));

        let changed_file = crate::fingerprints::FileFingerprint {
            sha256: "b".repeat(64),
            ..local_file
        };
        let changed = HashMap::from([(changed_file.path.clone(), changed_file)]);
        assert!(!fingerprint_skip_set(&files, &local, &changed).contains(&files[0].rel));
    }

    #[test]
    fn files_over_ten_mib_use_resumable_upload() {
        assert!(!uses_resumable_upload(RESUMABLE_UPLOAD_MIN_BYTES - 1));
        assert!(!uses_resumable_upload(RESUMABLE_UPLOAD_MIN_BYTES));
        assert!(uses_resumable_upload(RESUMABLE_UPLOAD_MIN_BYTES + 1));
    }
}
