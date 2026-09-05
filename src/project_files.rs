use std::path::Path;

use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use serde::Serialize;
use tauri::State;
use tokio::io::AsyncWriteExt;

use crate::{AppState, resolve_auth};

const MAX_TEXT_SIZE: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    path: String,
    size: u64,
    content: String,
    editable: bool,
}

fn validate_root(root: &str) -> Result<String, String> {
    let root = root.trim();
    if !root.starts_with('/') || root.contains('\0') {
        return Err("项目目录必须是远端主机上的绝对路径".into());
    }
    let root = root.trim_end_matches('/');
    Ok(if root.is_empty() { "/".into() } else { root.into() })
}

fn validate_relative(path: &str) -> Result<String, String> {
    let path = path.trim_matches('/');
    if path.contains('\0') || path.split('/').any(|part| part == "..") {
        return Err("文件路径无效".into());
    }
    Ok(path.into())
}

fn join_remote(root: &str, relative: &str) -> String {
    if relative.is_empty() {
        root.to_string()
    } else if root == "/" {
        format!("/{relative}")
    } else {
        format!("{root}/{relative}")
    }
}

fn is_within(root: &str, path: &str) -> bool {
    root == "/" || path == root || path.strip_prefix(root).is_some_and(|tail| tail.starts_with('/'))
}

fn is_editable(path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name == ".env" || name.starts_with(".env.") {
        return true;
    }
    matches!(
        Path::new(&name).extension().and_then(|value| value.to_str()),
        Some(
            "txt" | "yaml" | "yml" | "ini" | "conf" | "cfg" | "json" | "xml"
                | "properties" | "toml" | "md" | "sh" | "service" | "log"
        )
    )
}

async fn connect_sftp(
    state: &State<'_, AppState>,
    host_id: &str,
) -> Result<(russh::client::Handle<crate::ssh::SshClient>, SftpSession), String> {
    let (host, auth) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let host = store.get_host(host_id)?;
        let auth = resolve_auth(&store, &host.auth, &state.data_dir)?;
        (host, auth)
    };
    let mut session = crate::ssh::connect(&host.hostname, host.port).await?;
    crate::ssh::authenticate(&mut session, &host.username, &auth).await?;
    let channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("无法打开 SFTP 会话：{e}"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("远端主机不支持 SFTP：{e}"))?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| format!("无法初始化 SFTP：{e}"))?;
    Ok((session, sftp))
}

async fn resolve_path(sftp: &SftpSession, root: &str, relative: &str) -> Result<String, String> {
    let canonical_root = sftp
        .canonicalize(root)
        .await
        .map_err(|e| format!("无法访问项目目录 {root}：{e}"))?;
    let target = join_remote(&canonical_root, relative);
    let canonical_target = sftp
        .canonicalize(&target)
        .await
        .map_err(|e| format!("无法访问 {target}：{e}"))?;
    if !is_within(&canonical_root, &canonical_target) {
        return Err("目标路径超出项目目录".into());
    }
    Ok(canonical_target)
}

#[tauri::command]
pub async fn list_project_files(
    state: State<'_, AppState>,
    host_id: String,
    root: String,
    path: String,
) -> Result<Vec<ProjectFileEntry>, String> {
    let root = validate_root(&root)?;
    let relative = validate_relative(&path)?;
    let (_session, sftp) = connect_sftp(&state, &host_id).await?;
    let target = resolve_path(&sftp, &root, &relative).await?;
    let mut entries: Vec<_> = sftp
        .read_dir(target)
        .await
        .map_err(|e| format!("无法读取目录：{e}"))?
        .map(|entry| {
            let name = entry.file_name();
            let metadata = entry.metadata();
            let child = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };
            ProjectFileEntry {
                name,
                path: child,
                is_dir: metadata.is_dir(),
                size: metadata.len(),
            }
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });
    Ok(entries)
}

#[tauri::command]
pub async fn read_project_file(
    state: State<'_, AppState>,
    host_id: String,
    root: String,
    path: String,
) -> Result<ProjectFile, String> {
    let root = validate_root(&root)?;
    let relative = validate_relative(&path)?;
    if relative.is_empty() {
        return Err("请选择文件".into());
    }
    let (_session, sftp) = connect_sftp(&state, &host_id).await?;
    let target = resolve_path(&sftp, &root, &relative).await?;
    let metadata = sftp.metadata(&target).await.map_err(|e| e.to_string())?;
    if metadata.is_dir() {
        return Err("目标是目录".into());
    }
    if metadata.len() > MAX_TEXT_SIZE {
        return Err(format!("文件超过 {} MB，无法打开", MAX_TEXT_SIZE / 1024 / 1024));
    }
    let bytes = sftp.read(&target).await.map_err(|e| format!("读取文件失败：{e}"))?;
    let content = String::from_utf8(bytes).map_err(|_| "文件不是 UTF-8 文本，无法预览".to_string())?;
    Ok(ProjectFile {
        path: relative.clone(),
        size: metadata.len(),
        content,
        editable: is_editable(&relative),
    })
}

#[tauri::command]
pub async fn save_project_file(
    state: State<'_, AppState>,
    host_id: String,
    root: String,
    path: String,
    content: String,
) -> Result<(), String> {
    let root = validate_root(&root)?;
    let relative = validate_relative(&path)?;
    if !is_editable(&relative) {
        return Err("该文件类型不允许编辑".into());
    }
    if content.len() as u64 > MAX_TEXT_SIZE {
        return Err(format!("文件超过 {} MB，无法保存", MAX_TEXT_SIZE / 1024 / 1024));
    }
    let (_session, sftp) = connect_sftp(&state, &host_id).await?;
    let target = resolve_path(&sftp, &root, &relative).await?;
    let mut file = sftp
        .open_with_flags(&target, OpenFlags::WRITE | OpenFlags::TRUNCATE)
        .await
        .map_err(|e| format!("无法打开文件进行写入：{e}"))?;
    file.write_all(content.as_bytes())
        .await
        .map_err(|e| format!("保存文件失败：{e}"))?;
    file.shutdown().await.map_err(|e| format!("保存文件失败：{e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_project_paths() {
        assert_eq!(validate_root("/opt/app/").unwrap(), "/opt/app");
        assert!(validate_root("opt/app").is_err());
        assert!(validate_relative("../etc/passwd").is_err());
        assert_eq!(validate_relative("conf/app.yaml").unwrap(), "conf/app.yaml");
    }

    #[test]
    fn editable_extensions_include_dotenv() {
        assert!(is_editable(".env"));
        assert!(is_editable("config/.env.production"));
        assert!(is_editable("config/app.yaml"));
        assert!(is_editable("service/app.conf"));
        assert!(!is_editable("images/app.tar.gz"));
    }
}
