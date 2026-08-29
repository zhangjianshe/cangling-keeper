use futures_util::StreamExt;
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};

use crate::AppState;
use crate::store::Store;
use crate::sync::SETTING_SERVER_URL;

const SETTING_SOFTWARE_SET: &str = "software_set";
const SETTING_SOFTWARE_SETS: &str = "software_sets";
const SETTING_SOFTWARE_SETS_SEED: &str = "software_sets_seed";
const SOFTWARE_SETS_SEED: &str = "2";
const DEFAULT_SET_NAME: &str = "np4";
const DEFAULT_GIT_SET_NAME: &str = "cangling-repo";
const DEFAULT_GIT_URL: &str = "https://code.cangling.cn:22002/operation/cangling-repo.git";
const DEFAULT_GIT_USER: &str = "cangling-update";
const DEFAULT_GIT_TOKEN: &str = "94894bb4dbedb33707c868872081bd6e8c02bc8b";
const KIND_MANIFEST: &str = "manifest";
const KIND_GIT: &str = "git";

/// Largest text file we are willing to preview in the UI.
const MAX_PREVIEW_SIZE: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    pub cloned: bool,
    pub local_path: String,
    pub set_name: String,
    pub kind: String,
    pub git_url: String,
    pub git_branch: String,
    pub branch: String,
    pub commit: String,
    pub total_files: u32,
    pub downloaded: u32,
    pub skipped: u32,
    pub failed: u32,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoftwareSetInfo {
    pub name: String,
    pub kind: String,
    pub cloned: bool,
    pub local_path: String,
    pub git_url: String,
    pub git_username: String,
    pub git_token: String,
    pub git_branch: String,
    pub branch: String,
    pub commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SoftwareSetRecord {
    name: String,
    #[serde(default = "default_kind")]
    kind: String,
    #[serde(default)]
    git_url: String,
    #[serde(default)]
    git_username: String,
    #[serde(default)]
    git_token: String,
    #[serde(default)]
    git_branch: String,
}

fn default_kind() -> String {
    KIND_MANIFEST.to_string()
}

fn default_np4() -> SoftwareSetRecord {
    SoftwareSetRecord {
        name: DEFAULT_SET_NAME.to_string(),
        kind: KIND_MANIFEST.to_string(),
        git_url: String::new(),
        git_username: String::new(),
        git_token: String::new(),
        git_branch: String::new(),
    }
}

fn default_git_repo() -> SoftwareSetRecord {
    SoftwareSetRecord {
        name: DEFAULT_GIT_SET_NAME.to_string(),
        kind: KIND_GIT.to_string(),
        git_url: DEFAULT_GIT_URL.to_string(),
        git_username: DEFAULT_GIT_USER.to_string(),
        git_token: DEFAULT_GIT_TOKEN.to_string(),
        git_branch: String::new(),
    }
}

fn default_sets() -> Vec<SoftwareSetRecord> {
    vec![default_np4(), default_git_repo()]
}

fn ensure_named(records: &mut Vec<SoftwareSetRecord>, rec: SoftwareSetRecord, index: usize) {
    if let Some(existing) = records
        .iter_mut()
        .find(|r| r.name.eq_ignore_ascii_case(&rec.name))
    {
        if existing.kind == KIND_GIT
            && existing.git_url.trim().is_empty()
            && !rec.git_url.is_empty()
        {
            existing.kind = rec.kind;
            existing.git_url = rec.git_url;
            existing.git_username = rec.git_username;
            existing.git_token = rec.git_token;
            if existing.git_branch.is_empty() {
                existing.git_branch = rec.git_branch;
            }
        }
        return;
    }
    let index = index.min(records.len());
    records.insert(index, rec);
}

fn apply_default_sets(records: &mut Vec<SoftwareSetRecord>) {
    ensure_named(records, default_np4(), 0);
    let git_index = if records.first().map(|r| r.name.as_str()) == Some(DEFAULT_SET_NAME) {
        1
    } else {
        0
    };
    ensure_named(records, default_git_repo(), git_index);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncProgress {
    set_name: String,
    current: u32,
    total: u32,
    file: String,
    action: String,
    bytes_done: u64,
    bytes_total: u64,
    overall_done: u64,
    overall_total: u64,
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope {
    #[serde(default)]
    code: i32,
    #[serde(default)]
    message: String,
    #[serde(default)]
    success: bool,
    #[serde(default)]
    data: Option<SoftwareSetManifest>,
}

fn empty_on_null<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn zero_on_null<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<u64>::deserialize(deserializer)?.unwrap_or(0))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SoftwareSetManifest {
    #[serde(default, deserialize_with = "empty_on_null")]
    #[allow(dead_code)]
    name: String,
    #[serde(default)]
    softwares: Vec<SoftwareManifest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SoftwareManifest {
    #[serde(default, deserialize_with = "empty_on_null")]
    id: String,
    #[serde(default, deserialize_with = "empty_on_null")]
    name: String,
    #[serde(default, deserialize_with = "empty_on_null")]
    code: String,
    #[serde(default)]
    files: Vec<SoftwareFileManifest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SoftwareFileManifest {
    #[serde(default, deserialize_with = "empty_on_null")]
    name: String,
    #[serde(default, deserialize_with = "empty_on_null")]
    version: String,
    #[serde(default, deserialize_with = "empty_on_null")]
    os: String,
    #[serde(default, deserialize_with = "empty_on_null")]
    arch: String,
    #[serde(default, deserialize_with = "zero_on_null")]
    size: u64,
    #[serde(default, deserialize_with = "empty_on_null")]
    hash: String,
    #[serde(default, deserialize_with = "empty_on_null")]
    url: String,
}

pub(crate) fn sets_root(data_dir: &Path) -> PathBuf {
    data_dir.join("software-sets")
}

fn set_dir(data_dir: &Path, set_name: &str) -> Result<PathBuf, String> {
    let name = sanitize_component(set_name)?;
    Ok(sets_root(data_dir).join(name))
}

fn sanitize_component(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("名称不能为空".into());
    }
    if value.contains("..") || value.contains('/') || value.contains('\\') {
        return Err("名称不合法".into());
    }
    let cleaned: String = value
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            other => other,
        })
        .collect();
    if cleaned.is_empty() {
        return Err("名称不合法".into());
    }
    Ok(cleaned)
}

fn sanitize_filename(value: &str) -> Result<String, String> {
    let name = value.replace('\\', "/");
    let name = name.rsplit('/').next().unwrap_or("").trim();
    sanitize_component(name)
}

fn normalize_set_name(value: &str) -> Result<String, String> {
    sanitize_component(value)
}

fn normalize_kind(value: &str) -> Result<String, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "manifest" => Ok(KIND_MANIFEST.to_string()),
        "git" => Ok(KIND_GIT.to_string()),
        _ => Err("软件集类型必须是 manifest 或 git".into()),
    }
}

fn load_set_records(store: &Store) -> Vec<SoftwareSetRecord> {
    let raw = store
        .get_setting(SETTING_SOFTWARE_SETS)
        .ok()
        .flatten()
        .unwrap_or_default();
    if raw.trim().is_empty() {
        return Vec::new();
    }
    if let Ok(records) = serde_json::from_str::<Vec<SoftwareSetRecord>>(&raw) {
        return records
            .into_iter()
            .filter_map(|mut rec| {
                rec.name = normalize_set_name(&rec.name).ok()?;
                rec.kind = normalize_kind(&rec.kind).unwrap_or_else(|_| KIND_MANIFEST.to_string());
                rec.git_url = rec.git_url.trim().to_string();
                rec.git_username = rec.git_username.trim().to_string();
                rec.git_branch = rec.git_branch.trim().to_string();
                Some(rec)
            })
            .collect();
    }
    serde_json::from_str::<Vec<String>>(&raw)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| {
            Some(SoftwareSetRecord {
                name: normalize_set_name(&name).ok()?,
                kind: KIND_MANIFEST.to_string(),
                git_url: String::new(),
                git_username: String::new(),
                git_token: String::new(),
                git_branch: String::new(),
            })
        })
        .collect()
}

fn save_set_records(store: &Store, records: &[SoftwareSetRecord]) -> Result<(), String> {
    let json = serde_json::to_string(records).map_err(|e| e.to_string())?;
    store.set_setting(SETTING_SOFTWARE_SETS, &json)
}

fn discover_set_records(data_dir: &Path) -> Vec<SoftwareSetRecord> {
    let root = sets_root(data_dir);
    let Ok(rd) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for item in rd.flatten() {
        let path = item.path();
        if !path.is_dir() {
            continue;
        }
        let name = item.file_name().to_string_lossy().into_owned();
        let Ok(cleaned) = normalize_set_name(&name) else {
            continue;
        };
        let kind = if path.join(".git").is_dir() {
            KIND_GIT
        } else {
            KIND_MANIFEST
        };
        records.push(SoftwareSetRecord {
            name: cleaned,
            kind: kind.to_string(),
            git_url: String::new(),
            git_username: String::new(),
            git_token: String::new(),
            git_branch: String::new(),
        });
    }
    records
}

fn unique_push_record(records: &mut Vec<SoftwareSetRecord>, rec: SoftwareSetRecord) {
    if !records
        .iter()
        .any(|r| r.name.eq_ignore_ascii_case(&rec.name))
    {
        records.push(rec);
    }
}

fn ensure_set_records(store: &Store, data_dir: &Path) -> Result<Vec<SoftwareSetRecord>, String> {
    let initialized = store
        .get_setting(SETTING_SOFTWARE_SETS)
        .ok()
        .flatten()
        .is_some();
    let seed = store
        .get_setting(SETTING_SOFTWARE_SETS_SEED)
        .ok()
        .flatten()
        .unwrap_or_default();
    let mut records = load_set_records(store);
    for rec in discover_set_records(data_dir) {
        unique_push_record(&mut records, rec);
    }
    if !initialized || seed != SOFTWARE_SETS_SEED {
        apply_default_sets(&mut records);
        store.set_setting(SETTING_SOFTWARE_SETS_SEED, SOFTWARE_SETS_SEED)?;
    }
    if records.is_empty() {
        records.extend(default_sets());
    }
    save_set_records(store, &records)?;

    let current = store
        .get_setting(SETTING_SOFTWARE_SET)
        .ok()
        .flatten()
        .and_then(|s| normalize_set_name(&s).ok())
        .unwrap_or_default();
    if current.is_empty() || !records.iter().any(|r| r.name == current) {
        store.set_setting(SETTING_SOFTWARE_SET, &records[0].name)?;
    }
    Ok(records)
}

fn git_head(dest: &Path) -> (String, String) {
    if !dest.join(".git").is_dir() {
        return (String::new(), String::new());
    }
    let branch = run_git(dest, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let commit = run_git(dest, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    (branch, commit)
}

fn set_cloned(dest: &Path, kind: &str) -> bool {
    if kind == KIND_GIT {
        dest.join(".git").is_dir()
    } else {
        dest.is_dir()
    }
}

fn set_info(data_dir: &Path, rec: &SoftwareSetRecord) -> SoftwareSetInfo {
    let dest = set_dir(data_dir, &rec.name).unwrap_or_else(|_| sets_root(data_dir).join(&rec.name));
    let (branch, commit) = git_head(&dest);
    SoftwareSetInfo {
        name: rec.name.clone(),
        kind: rec.kind.clone(),
        cloned: set_cloned(&dest, &rec.kind),
        local_path: dest.to_string_lossy().into_owned(),
        git_url: rec.git_url.clone(),
        git_username: rec.git_username.clone(),
        git_token: rec.git_token.clone(),
        git_branch: rec.git_branch.clone(),
        branch,
        commit,
    }
}

fn list_set_infos(store: &Store, data_dir: &Path) -> Result<Vec<SoftwareSetInfo>, String> {
    let records = ensure_set_records(store, data_dir)?;
    Ok(records
        .into_iter()
        .map(|rec| set_info(data_dir, &rec))
        .collect())
}

fn current_set_name(state: &AppState) -> String {
    let Ok(store) = state.store.lock() else {
        return String::new();
    };
    let _ = ensure_set_records(&store, &state.data_dir);
    store
        .get_setting(SETTING_SOFTWARE_SET)
        .ok()
        .flatten()
        .and_then(|s| normalize_set_name(&s).ok())
        .unwrap_or_else(|| DEFAULT_SET_NAME.to_string())
}

fn current_record(state: &AppState) -> Option<SoftwareSetRecord> {
    let Ok(store) = state.store.lock() else {
        return None;
    };
    let records = ensure_set_records(&store, &state.data_dir).ok()?;
    let current = store
        .get_setting(SETTING_SOFTWARE_SET)
        .ok()
        .flatten()
        .and_then(|s| normalize_set_name(&s).ok())
        .unwrap_or_default();
    records
        .iter()
        .find(|r| r.name == current)
        .cloned()
        .or_else(|| records.first().cloned())
}

fn current_status(state: &AppState) -> RepoStatus {
    let rec = current_record(state).unwrap_or_else(default_np4);
    let dest = set_dir(&state.data_dir, &rec.name).unwrap_or_else(|_| sets_root(&state.data_dir));
    let (branch, commit) = git_head(&dest);
    RepoStatus {
        cloned: set_cloned(&dest, &rec.kind) && !rec.name.trim().is_empty(),
        local_path: dest.to_string_lossy().into_owned(),
        set_name: rec.name,
        kind: rec.kind,
        git_url: rec.git_url,
        git_branch: rec.git_branch,
        branch,
        commit,
        total_files: 0,
        downloaded: 0,
        skipped: 0,
        failed: 0,
        error: String::new(),
    }
}

fn resolve_relative(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim_matches('/');
    let base = if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    };

    let canon_root = root
        .canonicalize()
        .map_err(|e| format!("仓库不存在：{e}"))?;
    let canon_base = base
        .canonicalize()
        .map_err(|e| format!("路径不存在：{e}"))?;
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

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("读取本地文件失败：{e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

fn software_code(software: &SoftwareManifest) -> String {
    if software.code.trim().is_empty() {
        software.id.clone()
    } else {
        software.code.clone()
    }
}

fn local_rel_path(
    software: &SoftwareManifest,
    file: &SoftwareFileManifest,
) -> Result<PathBuf, String> {
    let code = sanitize_component(&software_code(software))?;
    let version = sanitize_component(if file.version.trim().is_empty() {
        "_"
    } else {
        file.version.trim()
    })?;
    let os = sanitize_component(if file.os.trim().is_empty() {
        "_"
    } else {
        file.os.trim()
    })?;
    let arch = sanitize_component(if file.arch.trim().is_empty() {
        "_"
    } else {
        file.arch.trim()
    })?;
    let name = sanitize_filename(&file.name)?;
    Ok(PathBuf::from(code)
        .join(version)
        .join(os)
        .join(arch)
        .join(name))
}

fn file_url(server_url: &str, url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("文件缺少下载地址".into());
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return Ok(url.to_string());
    }
    let base = server_url.trim_end_matches('/');
    if url.starts_with('/') {
        Ok(format!("{base}{url}"))
    } else {
        Ok(format!("{base}/{url}"))
    }
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30 * 60))
        .build()
        .map_err(|e| e.to_string())
}

async fn fetch_manifest(server_url: &str, set_name: &str) -> Result<SoftwareSetManifest, String> {
    let url = format!(
        "{}/api/v1/software/manifest",
        server_url.trim_end_matches('/')
    );
    let resp = http_client()?
        .get(&url)
        .query(&[("set", set_name)])
        .send()
        .await
        .map_err(|e| format!("查询软件集 manifest 失败: {e}"))?;
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let env: ApiEnvelope =
        serde_json::from_str(&text).map_err(|e| format!("解析 manifest 失败: {e}"))?;
    if env.code != 200 && !env.success {
        let msg = if env.message.is_empty() {
            format!("服务器错误 code={}", env.code)
        } else {
            env.message
        };
        return Err(msg);
    }
    env.data
        .ok_or_else(|| "服务器返回空的软件集信息".to_string())
}

struct DownloadProgressCtx<'a> {
    app: &'a AppHandle,
    set_name: &'a str,
    current: u32,
    total: u32,
    file: &'a str,
    overall_done: u64,
    overall_total: u64,
}

fn emit_sync_progress(
    ctx: &DownloadProgressCtx<'_>,
    action: &str,
    bytes_done: u64,
    bytes_total: u64,
) {
    let _ = ctx.app.emit(
        "repo-sync-progress",
        SyncProgress {
            set_name: ctx.set_name.to_string(),
            current: ctx.current,
            total: ctx.total,
            file: ctx.file.to_string(),
            action: action.to_string(),
            bytes_done,
            bytes_total,
            overall_done: ctx.overall_done.saturating_add(bytes_done),
            overall_total: ctx.overall_total,
        },
    );
}

async fn download_file(
    url: &str,
    dest: &Path,
    expected_size: u64,
    progress: Option<&DownloadProgressCtx<'_>>,
) -> Result<String, String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
    }
    let tmp = dest.with_extension("part");
    let resp = http_client()?
        .get(url)
        .send()
        .await
        .map_err(|e| format!("下载失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败：HTTP {}", resp.status()));
    }
    let bytes_total = resp.content_length().unwrap_or(expected_size);
    if let Some(ctx) = progress {
        emit_sync_progress(ctx, "download", 0, bytes_total);
    }
    let mut file = std::fs::File::create(&tmp).map_err(|e| format!("写入临时文件失败：{e}"))?;
    let mut hasher = Sha256::new();
    let mut stream = resp.bytes_stream();
    let mut bytes_done = 0u64;
    let mut last_emit = Instant::now();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                drop(file);
                let _ = std::fs::remove_file(&tmp);
                return Err(format!("下载中断：{e}"));
            }
        };
        hasher.update(&chunk);
        if let Err(e) = file.write_all(&chunk) {
            drop(file);
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("写入文件失败：{e}"));
        }
        bytes_done += chunk.len() as u64;
        if let Some(ctx) = progress {
            let now = Instant::now();
            if now.duration_since(last_emit) >= Duration::from_millis(200) {
                emit_sync_progress(ctx, "download", bytes_done, bytes_total);
                last_emit = now;
            }
        }
    }
    drop(file);
    std::fs::rename(&tmp, dest).map_err(|e| format!("保存文件失败：{e}"))?;
    if let Some(ctx) = progress {
        emit_sync_progress(ctx, "download", bytes_done, bytes_total.max(bytes_done));
    }
    Ok(hex_encode(&hasher.finalize()))
}

fn local_matches(path: &Path, file: &SoftwareFileManifest) -> bool {
    if !path.is_file() {
        return false;
    }
    let meta_ok = path
        .metadata()
        .map(|m| m.len() == file.size)
        .unwrap_or(false);
    if !file.hash.trim().is_empty() {
        return sha256_file(path)
            .map(|h| h.eq_ignore_ascii_case(file.hash.trim()))
            .unwrap_or(false);
    }
    meta_ok
}

fn prune_extra_files(root: &Path, expected: &HashSet<String>) -> Result<(), String> {
    if !root.is_dir() {
        return Ok(());
    }
    fn walk(dir: &Path, root: &Path, expected: &HashSet<String>) -> Result<(), String> {
        let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
        for item in rd {
            let item = item.map_err(|e| e.to_string())?;
            let path = item.path();
            if path.is_dir() {
                walk(&path, root, expected)?;
                if path
                    .read_dir()
                    .map(|mut i| i.next().is_none())
                    .unwrap_or(false)
                {
                    let _ = std::fs::remove_dir(&path);
                }
            } else if path.is_file() {
                let rel = relative_path(root, &path).unwrap_or_default();
                if !expected.contains(&rel) {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        Ok(())
    }
    walk(root, root, expected)
}

fn encode_userinfo(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn git_auth_url(url: &str, user: &str, token: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("请填写 Git 仓库地址".into());
    }
    if user.is_empty() && token.is_empty() {
        return Ok(url.to_string());
    }
    let cred = if token.is_empty() {
        encode_userinfo(user)
    } else if user.is_empty() {
        format!(":{}", encode_userinfo(token))
    } else {
        format!("{}:{}", encode_userinfo(user), encode_userinfo(token))
    };
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
        ("https://", rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        ("http://", rest)
    } else {
        return Ok(url.to_string());
    };
    let host_path = if let Some((_, after)) = rest.rsplit_once('@') {
        after
    } else {
        rest
    };
    Ok(format!("{scheme}{cred}@{host_path}"))
}

fn redact_git_text(text: &str, rec: &SoftwareSetRecord) -> String {
    let mut out = text.to_string();
    if !rec.git_token.is_empty() {
        out = out.replace(&rec.git_token, "***");
        out = out.replace(&encode_userinfo(&rec.git_token), "***");
    }
    if !rec.git_username.is_empty() {
        out = out.replace(&rec.git_username, rec.git_username.as_str());
    }
    out
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = crate::host_cmd::command("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
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

fn parse_git_percent(line: &str) -> Option<u32> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i > 0 {
            let mut j = i;
            while j > 0 && bytes[j - 1].is_ascii_digit() {
                j -= 1;
            }
            if j < i {
                if let Ok(n) = line[j..i].parse::<u32>() {
                    return Some(n.min(100));
                }
            }
        }
        i += 1;
    }
    None
}

fn parse_git_bytes(line: &str) -> Option<u64> {
    let before_speed = line.split('|').next().unwrap_or(line);
    let mut last: Option<u64> = None;
    let parts: Vec<&str> = before_speed.split_whitespace().collect();
    let mut i = 0;
    while i + 1 < parts.len() {
        let num = parts[i].trim_end_matches(',');
        let unit = parts[i + 1].trim_end_matches(',');
        if let Ok(v) = num.parse::<f64>() {
            let mul = match unit {
                "B" | "bytes" => 1.0,
                "KiB" | "KB" => 1024.0,
                "MiB" | "MB" => 1024.0 * 1024.0,
                "GiB" | "GB" => 1024.0 * 1024.0 * 1024.0,
                _ => {
                    i += 1;
                    continue;
                }
            };
            last = Some((v * mul) as u64);
        }
        i += 1;
    }
    last
}

fn emit_git_progress(app: &AppHandle, set_name: &str, phase: &str, line: &str, last_pct: &mut u32) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    if let Some(pct) = parse_git_percent(line) {
        *last_pct = pct;
    }
    let bytes_done = parse_git_bytes(line).unwrap_or(0);
    let _ = app.emit(
        "repo-sync-progress",
        SyncProgress {
            set_name: set_name.to_string(),
            current: *last_pct,
            total: 100,
            file: line.to_string(),
            action: phase.to_string(),
            bytes_done,
            bytes_total: 0,
            overall_done: u64::from(*last_pct),
            overall_total: 100,
        },
    );
}

fn run_git_progress(
    app: &AppHandle,
    set_name: &str,
    rec: &SoftwareSetRecord,
    cwd: Option<&Path>,
    args: &[&str],
    phase: &str,
) -> Result<(), String> {
    let mut cmd = crate::host_cmd::command("git");
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_FLUSH", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("无法运行 git：{e}"))?;
    let mut last_pct = 0u32;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut stderr_text = String::new();
    if let Some(stderr) = child.stderr.take() {
        let mut reader = stderr;
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match reader.read(&mut byte) {
                Ok(0) => {
                    if !buf.is_empty() {
                        let line = String::from_utf8_lossy(&buf);
                        stderr_text.push_str(&line);
                        stderr_text.push('\n');
                        emit_git_progress(
                            app,
                            set_name,
                            phase,
                            &redact_git_text(&line, rec),
                            &mut last_pct,
                        );
                    }
                    break;
                }
                Ok(_) => {
                    if byte[0] == b'\n' || byte[0] == b'\r' {
                        if !buf.is_empty() {
                            let line = String::from_utf8_lossy(&buf);
                            stderr_text.push_str(&line);
                            stderr_text.push('\n');
                            let now = Instant::now();
                            if now.duration_since(last_emit) >= Duration::from_millis(120)
                                || parse_git_percent(&line).is_some_and(|p| p == 100)
                            {
                                emit_git_progress(
                                    app,
                                    set_name,
                                    phase,
                                    &redact_git_text(&line, rec),
                                    &mut last_pct,
                                );
                                last_emit = now;
                            }
                            buf.clear();
                        }
                    } else if buf.len() < 2048 {
                        buf.push(byte[0]);
                    }
                }
                Err(e) => return Err(format!("读取 git 输出失败：{e}")),
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        let msg = redact_git_text(stderr_text.trim(), rec);
        return Err(if msg.is_empty() {
            format!("git 命令失败（退出码 {}）", status.code().unwrap_or(-1))
        } else {
            msg
        });
    }
    Ok(())
}

fn sync_git_set(
    app: &AppHandle,
    data_dir: &Path,
    rec: &SoftwareSetRecord,
) -> Result<RepoStatus, String> {
    let dest = set_dir(data_dir, &rec.name)?;
    let auth_url = git_auth_url(&rec.git_url, &rec.git_username, &rec.git_token)?;
    let parent = dest.parent().ok_or_else(|| "仓库路径无效".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;

    let updating = dest.join(".git").is_dir();
    emit_git_progress(
        app,
        &rec.name,
        if updating { "git-fetch" } else { "git-clone" },
        if updating {
            "正在从远端拉取…"
        } else {
            "正在克隆 Git 仓库…"
        },
        &mut 0,
    );

    if updating {
        let _ = run_git(&dest, &["remote", "set-url", "origin", &auth_url]);
        let branch = rec.git_branch.trim().to_string();
        let mut fetch_args = vec![
            "fetch".to_string(),
            "--progress".to_string(),
            "--prune".to_string(),
            "origin".to_string(),
        ];
        if !branch.is_empty() {
            fetch_args.push(branch.clone());
        }
        let fetch_refs: Vec<&str> = fetch_args.iter().map(|s| s.as_str()).collect();
        run_git_progress(app, &rec.name, rec, Some(&dest), &fetch_refs, "git-fetch")?;
        if !branch.is_empty() {
            let _ = run_git(&dest, &["checkout", &branch]);
        }
        emit_git_progress(app, &rec.name, "git-fetch", "正在合并远端更新…", &mut 100);
        run_git(&dest, &["merge", "--ff-only", "FETCH_HEAD"])
            .or_else(|_| run_git(&dest, &["merge", "--ff-only"]))?;
    } else {
        if dest.exists() {
            let empty = dest
                .read_dir()
                .map(|mut i| i.next().is_none())
                .unwrap_or(false);
            if empty {
                let _ = std::fs::remove_dir(&dest);
            } else {
                return Err(format!("目标目录已存在且不是 git 仓库：{}", dest.display()));
            }
        }
        let dest_s = dest.to_string_lossy().into_owned();
        let branch = rec.git_branch.trim().to_string();
        let mut args = vec!["clone".to_string(), "--progress".to_string()];
        if !branch.is_empty() {
            args.push("--branch".into());
            args.push(branch);
        }
        args.push(auth_url);
        args.push(dest_s);
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        run_git_progress(app, &rec.name, rec, Some(parent), &arg_refs, "git-clone")?;
    }

    let (branch, commit) = git_head(&dest);
    Ok(RepoStatus {
        cloned: dest.join(".git").is_dir(),
        local_path: dest.to_string_lossy().into_owned(),
        set_name: rec.name.clone(),
        kind: KIND_GIT.to_string(),
        git_url: rec.git_url.clone(),
        git_branch: rec.git_branch.clone(),
        branch,
        commit,
        total_files: 0,
        downloaded: 0,
        skipped: 0,
        failed: 0,
        error: String::new(),
    })
}

fn build_record(
    set_name: String,
    kind: String,
    git_url: String,
    git_username: String,
    git_token: String,
    git_branch: String,
) -> Result<SoftwareSetRecord, String> {
    let name = normalize_set_name(&set_name)?;
    let kind = normalize_kind(&kind)?;
    let git_url = git_url.trim().to_string();
    let git_username = git_username.trim().to_string();
    let git_branch = git_branch.trim().to_string();
    if kind == KIND_GIT && git_url.is_empty() {
        return Err("请填写 Git 仓库地址".into());
    }
    Ok(SoftwareSetRecord {
        name,
        kind,
        git_url,
        git_username,
        git_token,
        git_branch,
    })
}

#[tauri::command]
pub fn repo_status(state: State<'_, AppState>) -> RepoStatus {
    current_status(&state)
}

#[tauri::command]
pub fn list_software_sets(state: State<'_, AppState>) -> Result<Vec<SoftwareSetInfo>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    list_set_infos(&store, &state.data_dir)
}

#[tauri::command]
pub fn add_software_set(
    state: State<'_, AppState>,
    set_name: String,
    kind: String,
    git_url: String,
    git_username: String,
    git_token: String,
    git_branch: String,
) -> Result<Vec<SoftwareSetInfo>, String> {
    let rec = build_record(set_name, kind, git_url, git_username, git_token, git_branch)?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let mut records = ensure_set_records(&store, &state.data_dir)?;
    if records
        .iter()
        .any(|r| r.name.eq_ignore_ascii_case(&rec.name))
    {
        return Err(format!("软件集 {} 已存在", rec.name));
    }
    let name = rec.name.clone();
    records.push(rec);
    save_set_records(&store, &records)?;
    store.set_setting(SETTING_SOFTWARE_SET, &name)?;
    list_set_infos(&store, &state.data_dir)
}

#[tauri::command]
pub fn update_software_set(
    state: State<'_, AppState>,
    set_name: String,
    kind: String,
    git_url: String,
    git_username: String,
    git_token: String,
    git_branch: String,
) -> Result<Vec<SoftwareSetInfo>, String> {
    let rec = build_record(set_name, kind, git_url, git_username, git_token, git_branch)?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let mut records = ensure_set_records(&store, &state.data_dir)?;
    let Some(existing) = records
        .iter_mut()
        .find(|r| r.name.eq_ignore_ascii_case(&rec.name))
    else {
        return Err(format!("软件集 {} 不存在", rec.name));
    };
    *existing = rec.clone();
    save_set_records(&store, &records)?;
    store.set_setting(SETTING_SOFTWARE_SET, &rec.name)?;
    list_set_infos(&store, &state.data_dir)
}

#[tauri::command]
pub fn remove_software_set(
    state: State<'_, AppState>,
    set_name: String,
) -> Result<Vec<SoftwareSetInfo>, String> {
    let name = normalize_set_name(&set_name)?;
    let dest = set_dir(&state.data_dir, &name)?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let mut records = ensure_set_records(&store, &state.data_dir)?;
    records.retain(|r| !r.name.eq_ignore_ascii_case(&name));
    if records.is_empty() {
        records.extend(default_sets());
    }
    save_set_records(&store, &records)?;
    let current = store
        .get_setting(SETTING_SOFTWARE_SET)
        .ok()
        .flatten()
        .unwrap_or_default();
    if current == name || !records.iter().any(|r| r.name == current) {
        store.set_setting(SETTING_SOFTWARE_SET, &records[0].name)?;
    }
    drop(store);
    if dest.is_dir() {
        std::fs::remove_dir_all(&dest).map_err(|e| format!("删除本地文件失败：{e}"))?;
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    list_set_infos(&store, &state.data_dir)
}

#[tauri::command]
pub fn select_software_set(
    state: State<'_, AppState>,
    set_name: String,
) -> Result<RepoStatus, String> {
    let name = normalize_set_name(&set_name)?;
    {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let records = ensure_set_records(&store, &state.data_dir)?;
        if !records.iter().any(|r| r.name == name) {
            return Err(format!("软件集 {name} 不存在"));
        }
        store.set_setting(SETTING_SOFTWARE_SET, &name)?;
    }
    Ok(current_status(&state))
}

#[tauri::command]
pub async fn sync_software_set(
    app: AppHandle,
    state: State<'_, AppState>,
    set_name: String,
) -> Result<RepoStatus, String> {
    let set_name = normalize_set_name(&set_name)?;
    let rec = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let records = ensure_set_records(&store, &state.data_dir)?;
        let rec = records
            .into_iter()
            .find(|r| r.name == set_name)
            .ok_or_else(|| format!("软件集 {set_name} 不存在"))?;
        store.set_setting(SETTING_SOFTWARE_SET, &set_name)?;
        rec
    };

    if rec.kind == KIND_GIT {
        let data_dir = state.data_dir.clone();
        let app2 = app.clone();
        return tauri::async_runtime::spawn_blocking(move || sync_git_set(&app2, &data_dir, &rec))
            .await
            .map_err(|e| format!("后台任务失败：{e}"))?;
    }

    let server_url = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store
            .get_setting(SETTING_SERVER_URL)
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "请先登录维护中心，以便获取服务器地址".to_string())?
    };
    let data_dir = state.data_dir.clone();
    let dest = set_dir(&data_dir, &set_name)?;
    std::fs::create_dir_all(&dest).map_err(|e| format!("创建目录失败：{e}"))?;

    let manifest = fetch_manifest(&server_url, &set_name).await?;
    let mut expected = HashSet::new();
    let mut jobs = Vec::new();
    for software in &manifest.softwares {
        for file in &software.files {
            let rel = local_rel_path(software, file)?;
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            expected.insert(rel_str.clone());
            jobs.push((rel, rel_str, software.name.clone(), file.clone()));
        }
    }

    let total = jobs.len() as u32;
    let overall_total: u64 = jobs.iter().map(|(_, _, _, file)| file.size).sum();
    let mut downloaded = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;
    let mut last_error = String::new();
    let mut overall_done = 0u64;

    for (index, (rel, rel_str, software_name, file)) in jobs.into_iter().enumerate() {
        let target = dest.join(&rel);
        let display = if software_name.is_empty() {
            rel_str.clone()
        } else {
            format!("{software_name}/{rel_str}")
        };
        let current = (index as u32) + 1;
        let ctx = DownloadProgressCtx {
            app: &app,
            set_name: &set_name,
            current,
            total,
            file: &display,
            overall_done,
            overall_total,
        };
        if local_matches(&target, &file) {
            skipped += 1;
            overall_done = overall_done.saturating_add(file.size);
            emit_sync_progress(&ctx, "skip", file.size, file.size);
        } else {
            match file_url(&server_url, &file.url) {
                Ok(url) => match download_file(&url, &target, file.size, Some(&ctx)).await {
                    Ok(hash) => {
                        if !file.hash.trim().is_empty()
                            && !hash.eq_ignore_ascii_case(file.hash.trim())
                        {
                            failed += 1;
                            last_error = format!("{} 哈希不匹配", file.name);
                            emit_sync_progress(&ctx, "fail", 0, file.size);
                        } else {
                            downloaded += 1;
                            overall_done = overall_done.saturating_add(file.size);
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        last_error = format!("{}: {e}", file.name);
                        emit_sync_progress(&ctx, "fail", 0, file.size);
                    }
                },
                Err(e) => {
                    failed += 1;
                    last_error = e;
                    emit_sync_progress(&ctx, "fail", 0, file.size);
                }
            }
        }
    }

    prune_extra_files(&dest, &expected)?;

    if failed > 0 && downloaded == 0 && skipped == 0 {
        return Err(if last_error.is_empty() {
            "同步失败".into()
        } else {
            last_error
        });
    }

    Ok(RepoStatus {
        cloned: true,
        local_path: dest.to_string_lossy().into_owned(),
        set_name,
        kind: KIND_MANIFEST.to_string(),
        git_url: String::new(),
        git_branch: String::new(),
        branch: String::new(),
        commit: String::new(),
        total_files: total,
        downloaded,
        skipped,
        failed,
        error: last_error,
    })
}

#[tauri::command]
pub fn list_repo_files(state: State<'_, AppState>, path: String) -> Result<Vec<RepoEntry>, String> {
    let set_name = current_set_name(&state);
    if set_name.trim().is_empty() {
        return Err("尚未同步软件集".into());
    }
    let root = set_dir(&state.data_dir, &set_name)?;
    let base = resolve_relative(&root, &path)?;

    let mut entries = Vec::new();
    let rd = std::fs::read_dir(&base).map_err(|e| format!("读取目录失败：{e}"))?;
    for item in rd {
        let item = item.map_err(|e| e.to_string())?;
        let file_type = item.file_type().map_err(|e| e.to_string())?;
        let name = item.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
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

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

#[tauri::command]
pub fn read_repo_file(state: State<'_, AppState>, path: String) -> Result<RepoFile, String> {
    let set_name = current_set_name(&state);
    if set_name.trim().is_empty() {
        return Err("尚未同步软件集".into());
    }
    let root = set_dir(&state.data_dir, &set_name)?;
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
