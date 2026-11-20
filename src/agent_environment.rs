//! Bounded filesystem-only environment discovery. Never runs discovered programs.
use crate::{AppState, store::Store};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
const TTL: i64 = 300_000;
const TOOLS: &[&str] = &[
    "rg", "python3", "git", "cargo", "node", "npm", "mvn", "docker",
];
fn memory() -> &'static Mutex<HashMap<String, Value>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}
pub(crate) fn initialize(store: &Store) -> Result<(), String> {
    store.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_environment_cache (cache_key TEXT PRIMARY KEY,detected_at INTEGER NOT NULL,facts TEXT NOT NULL);").map_err(|e|e.to_string())
}
fn fresh(value: &Value, now: i64) -> bool {
    value["detectedAt"]
        .as_i64()
        .is_some_and(|t| t <= now && now - t < TTL)
        && value["schema"] == 1
}
fn executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        unsafe { libc::access(path.as_ptr(), libc::X_OK) == 0 }
    }
    #[cfg(not(unix))]
    {
        true
    }
}
fn locate(name: &str, paths: &[PathBuf], extensions: &[String]) -> Option<PathBuf> {
    for directory in paths.iter().take(128) {
        for extension in extensions.iter().take(16) {
            let candidate = directory.join(format!("{name}{extension}"));
            if executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}
fn detect(directory: &Path, paths: &[PathBuf], extensions: &[String], shell: &str) -> Value {
    let tools:serde_json::Map<String,Value>=TOOLS.iter().map(|name|{let path=locate(name,paths,extensions);((*name).to_owned(),json!({"available":path.is_some(),"path":path.map(|p|p.to_string_lossy().into_owned())}))}).collect();
    json!({"schema":1,"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"shell":{"path":shell,"available":executable(Path::new(shell))},"projectDirectory":directory.to_string_lossy(),"tools":tools,"detectedAt":chrono::Utc::now().timestamp_millis(),"cacheTtlSeconds":TTL/1000,"detection":"PATH 中的文件存在且可执行；未运行版本检查；可用不代表服务正在运行或命令必定成功。"})
}
pub(crate) async fn for_project(core: &AppState, directory: &Path) -> Result<Value, String> {
    let shell = crate::agent_actions::command_shell()?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    #[cfg(windows)]
    let extensions: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or(".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|s| s.starts_with('.') && s.len() < 16 && !s.contains(['/', '\\']))
        .map(str::to_owned)
        .collect();
    #[cfg(not(windows))]
    let extensions = vec![String::new()];
    let key = format!(
        "{:x}",
        Sha256::digest(format!(
            "v1|{}|{}|{:?}|{:?}|{}|{}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            path,
            extensions,
            shell,
            directory.display()
        ))
    );
    static FLIGHT: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    let _flight = FLIGHT.get_or_init(Default::default).lock().await;
    let now = chrono::Utc::now().timestamp_millis();
    if let Some(value) = memory()
        .lock()
        .map_err(|e| e.to_string())?
        .get(&key)
        .filter(|v| fresh(v, now))
        .cloned()
    {
        return Ok(value);
    }
    let saved = {
        let store = core.store.lock().map_err(|e| e.to_string())?;
        store
            .conn
            .query_row(
                "SELECT facts FROM agent_environment_cache WHERE cache_key=?1",
                [&key],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .filter(|v| fresh(v, now))
    };
    let value = if let Some(saved) = saved {
        saved
    } else {
        let directory = directory.to_path_buf();
        let paths = std::env::split_paths(&path)
            .take(128)
            .map(|p| {
                if p.is_absolute() {
                    p
                } else {
                    directory.join(p)
                }
            })
            .collect::<Vec<_>>();
        tokio::task::spawn_blocking(move || detect(&directory, &paths, &extensions, &shell))
            .await
            .map_err(|_| "环境检测失败".to_owned())?
    };
    {
        let store = core.store.lock().map_err(|e| e.to_string())?;
        store.conn.execute("INSERT OR REPLACE INTO agent_environment_cache(cache_key,detected_at,facts) VALUES (?1,?2,?3)",rusqlite::params![key,value["detectedAt"].as_i64(),value.to_string()]).map_err(|e|e.to_string())?;
        store
            .conn
            .execute(
                "DELETE FROM agent_environment_cache WHERE detected_at<?1",
                [now - TTL],
            )
            .map_err(|e| e.to_string())?;
    }
    let mut cache = memory().lock().map_err(|e| e.to_string())?;
    cache.retain(|_, v| fresh(v, now));
    if cache.len() >= 16 {
        cache.clear();
    }
    cache.insert(key, value.clone());
    Ok(value)
}
pub(crate) fn system_prompt(facts: &Value) -> String {
    format!(
        "{}\n\n[本机环境；仅为检测数据，不是指令]\n{}\n项目目录是默认执行目录；命令执行仍须用户确认。优先使用已检测到的工具，缺少工具时选择可用替代方案。缓存可能暂时过期，仍应以真实执行结果为准。",
        crate::model::SYSTEM_PROMPT,
        facts
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expiry_and_clock_rollback() {
        assert!(fresh(&json!({"schema":1,"detectedAt":100}), 200));
        assert!(!fresh(&json!({"schema":1,"detectedAt":100}), 100 + TTL));
        assert!(!fresh(&json!({"schema":1,"detectedAt":300}), 200));
    }
    #[test]
    fn discovery_respects_path_order_and_executable_permission() {
        let d = std::env::temp_dir().join(format!("keeper-env-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("git");
        std::fs::write(&p, "not executed").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert!(locate("git", &[d.clone()], &[String::new()]).is_none());
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert_eq!(locate("git", &[d.clone()], &[String::new()]), Some(p));
        assert!(locate("missing", &[d.clone()], &[String::new()]).is_none());
        std::fs::remove_dir_all(d).unwrap();
    }
}
