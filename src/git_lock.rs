//! Recovery is conservative: never kill Git or remove an index lock while any
//! Git/LFS process is visible. An unknown process state also prevents cleanup.
use std::{
    path::Path,
    time::{Duration, SystemTime},
};

pub(crate) fn recover_stale_index(dest: &Path) -> Result<bool, String> {
    recover_with_probe(dest, Duration::from_secs(30), git_processes_running)
}
fn recover_with_probe(
    dest: &Path,
    min_age: Duration,
    probe: impl Fn() -> Result<bool, String>,
) -> Result<bool, String> {
    let path = dest.join(".git/index.lock");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("无法检查 Git 索引锁：{e}")),
    };
    if !metadata.file_type().is_file() {
        return Err("index.lock 不是普通文件，未自动清理，请手动检查仓库".into());
    }
    if probe()? {
        return Err("检测到 Git/LFS 进程仍在运行，未删除 index.lock。请等待任务结束；如是旧版本遗留进程，请确认归属后结束该进程，再重新同步。".into());
    }
    let modified = metadata.modified().map_err(|e| e.to_string())?;
    if SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default()
        < min_age
    {
        return Err("index.lock 刚刚创建，无法确认已失效，未自动清理。请稍后重新同步。".into());
    }
    // Repeat both checks: a new process or a replaced lock must not be mistaken
    // for the old stale file between the initial inspection and removal.
    std::thread::sleep(Duration::from_millis(200));
    if probe()? {
        return Err("检查期间检测到 Git/LFS 进程启动，未清理索引锁，请等待任务结束".into());
    }
    let current = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !current.file_type().is_file()
        || current.len() != metadata.len()
        || current.modified().ok() != Some(modified)
    {
        return Err("index.lock 正在变化，未自动清理，请等待 Git 操作结束".into());
    }
    std::fs::remove_file(&path).map_err(|e| format!("无法清理失效的 index.lock：{e}"))?;
    Ok(true)
}
#[cfg(windows)]
fn git_processes_running() -> Result<bool, String> {
    let script = "$ErrorActionPreference='Stop';try{$p=@(Get-WmiObject Win32_Process -Filter \"Name='git.exe' OR Name='git-lfs.exe' OR Name='git-remote-http.exe' OR Name='git-remote-https.exe'\" -ErrorAction Stop);if($p.Count -gt 0){'busy'}else{'idle'}}catch{exit 1}";
    let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let out = crate::host_cmd::command(
        std::path::PathBuf::from(windows).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    )
    .args(["-NoProfile", "-NonInteractive", "-Command", script])
    .output()
    .map_err(|_| "无法检查 Windows Git 进程，未清理锁文件")?;
    if !out.status.success() {
        return Err("无法确认 Windows Git 进程状态，未清理锁文件".into());
    }
    match String::from_utf8_lossy(&out.stdout).trim() {
        "busy" => Ok(true),
        "idle" => Ok(false),
        _ => Err("Git 进程检查未返回有效结果，未清理锁文件".into()),
    }
}
#[cfg(target_os = "linux")]
fn git_processes_running() -> Result<bool, String> {
    for entry in std::fs::read_dir("/proc").map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        match std::fs::read_to_string(entry.path().join("comm")) {
            Ok(name) if name.trim().starts_with("git") => return Ok(true),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("无法确认全部 Git 进程状态，未清理锁文件".into()),
        }
    }
    Ok(false)
}
#[cfg(not(any(windows, target_os = "linux")))]
fn git_processes_running() -> Result<bool, String> {
    Err("当前系统无法核实 Git 进程，未自动清理索引锁".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("keeper-lock-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/index.lock"), b"").unwrap();
        root
    }
    #[test]
    fn active_or_unknown_processes_never_lose_their_lock() {
        let root = fixture();
        assert!(recover_with_probe(&root, Duration::ZERO, || Ok(true)).is_err());
        assert!(root.join(".git/index.lock").exists());
        assert!(recover_with_probe(&root, Duration::ZERO, || Err("unknown".into())).is_err());
        assert!(root.join(".git/index.lock").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn recent_locks_are_preserved_and_verified_stale_locks_are_removed() {
        let root = fixture();
        assert!(recover_with_probe(&root, Duration::from_secs(30), || Ok(false)).is_err());
        assert!(root.join(".git/index.lock").exists());
        assert!(recover_with_probe(&root, Duration::ZERO, || Ok(false)).unwrap());
        assert!(!root.join(".git/index.lock").exists());
        assert!(!recover_with_probe(&root, Duration::ZERO, || Ok(false)).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn process_starting_during_recovery_keeps_lock() {
        let root = fixture();
        let count = std::sync::atomic::AtomicUsize::new(0);
        assert!(
            recover_with_probe(&root, Duration::ZERO, || Ok(count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                > 0))
            .is_err()
        );
        assert!(root.join(".git/index.lock").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
