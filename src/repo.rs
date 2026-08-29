use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

use crate::AppState;

/// Git repository that backs the "软件仓库" panel.
const REPO_URL: &str = "https://code.cangling.cn:22002/operation/cangling-repo.git";
const REPO_USER: &str = "cangling-update";
const REPO_TOKEN: &str = "94894bb4dbedb33707c868872081bd6e8c02bc8b";

/// Largest text file we are willing to preview in the UI.
const MAX_PREVIEW_SIZE: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    pub cloned: bool,
    pub local_path: String,
    pub branch: String,
    pub commit: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoFile {
    pub path: String,
    pub size: u64,
    pub content: String,
}

fn repo_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("software-repo")
}

/// Build the clone URL with HTTP basic auth embedded. The token is a read-only
/// "cangling-update" credential used only to fetch this repository.
fn auth_url() -> String {
    REPO_URL.replacen(
        "https://",
        &format!("https://{REPO_USER}:{REPO_TOKEN}@"),
        1,
    )
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = crate::host_cmd::command("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|e| format!("无法运行 git：{e}"))?;

    if !output.status.success() {
        let msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if msg.is_empty() {
            format!(
                "git 命令失败（退出码 {}）",
                output.status.code().unwrap_or(-1)
            )
        } else {
            msg
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn clone_repo(data_dir: &Path) -> Result<(), String> {
    let dest = repo_dir(data_dir);
    if dest.join(".git").is_dir() {
        return Ok(());
    }
    if dest.exists() {
        return Err(format!("目标目录已存在且不是 git 仓库：{}", dest.display()));
    }

    let parent = dest.parent().ok_or_else(|| "仓库路径无效".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
    let name = dest
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("software-repo");
    run_git(parent, &["clone", "--depth", "1", &auth_url(), name])?;
    Ok(())
}

fn update_repo(data_dir: &Path) -> Result<(), String> {
    let dest = repo_dir(data_dir);
    run_git(&dest, &["pull", "--ff-only"])?;
    Ok(())
}

fn current_status(data_dir: &Path) -> RepoStatus {
    let dest = repo_dir(data_dir);
    let cloned = dest.join(".git").is_dir();
    let mut status = RepoStatus {
        cloned,
        local_path: dest.to_string_lossy().into_owned(),
        branch: String::new(),
        commit: String::new(),
        error: String::new(),
    };
    if cloned {
        status.branch =
            run_git(&dest, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
        status.commit = run_git(&dest, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    }
    status
}

/// Resolve a UI-supplied relative path against the repo root, refusing paths
/// that escape the repository directory.
fn resolve_relative(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim_matches('/');
    let base = if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    };

    let canon_root = root.canonicalize().map_err(|e| format!("仓库不存在：{e}"))?;
    let canon_base = base.canonicalize().map_err(|e| format!("路径不存在：{e}"))?;
    if !canon_base.starts_with(&canon_root) {
        return Err("非法路径".into());
    }
    Ok(canon_base)
}

fn relative_path(root: &Path, full: &Path) -> Result<String, String> {
    let canon_root = root.canonicalize().map_err(|e| e.to_string())?;
    let rel = full.strip_prefix(&canon_root).map_err(|e| e.to_string())?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

#[tauri::command]
pub fn repo_status(state: State<'_, AppState>) -> RepoStatus {
    current_status(&state.data_dir)
}

#[tauri::command]
pub async fn clone_or_update_repo(state: State<'_, AppState>) -> Result<RepoStatus, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dest = repo_dir(&data_dir);
        if dest.join(".git").is_dir() {
            update_repo(&data_dir)?;
        } else {
            clone_repo(&data_dir)?;
        }
        Ok::<RepoStatus, String>(current_status(&data_dir))
    })
    .await
    .map_err(|e| format!("后台任务失败：{e}"))?
}

#[tauri::command]
pub fn list_repo_files(state: State<'_, AppState>, path: String) -> Result<Vec<RepoEntry>, String> {
    let root = repo_dir(&state.data_dir);
    let base = resolve_relative(&root, &path)?;

    let mut entries = Vec::new();
    let rd = std::fs::read_dir(&base).map_err(|e| format!("读取目录失败：{e}"))?;
    for item in rd {
        let item = item.map_err(|e| e.to_string())?;
        let file_type = item.file_type().map_err(|e| e.to_string())?;
        let name = item.file_name().to_string_lossy().into_owned();
        let path = relative_path(&root, &item.path())?;
        let size = if file_type.is_dir() {
            0
        } else {
            item.metadata().map(|m| m.len()).unwrap_or(0)
        };
        entries.push(RepoEntry {
            name,
            path,
            is_dir: file_type.is_dir(),
            size,
        });
    }

    // Directories first, then files; each group sorted by name (case-insensitive).
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

#[tauri::command]
pub fn read_repo_file(state: State<'_, AppState>, path: String) -> Result<RepoFile, String> {
    let root = repo_dir(&state.data_dir);
    let full = resolve_relative(&root, &path)?;
    if !full.is_file() {
        return Err("不是文件".into());
    }

    let meta = full.metadata().map_err(|e| e.to_string())?;
    if meta.len() > MAX_PREVIEW_SIZE {
        return Err("文件过大，不支持在线预览".into());
    }

    let bytes = std::fs::read(&full).map_err(|e| format!("读取文件失败：{e}"))?;
    if bytes.contains(&0) {
        return Err("二进制文件不支持预览".into());
    }
    let content =
        String::from_utf8(bytes).map_err(|_| "文件不是 UTF-8 文本，不支持预览".to_string())?;

    Ok(RepoFile {
        path: relative_path(&root, &full)?,
        size: meta.len(),
        content,
    })
}
