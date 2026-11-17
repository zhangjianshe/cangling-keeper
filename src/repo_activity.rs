//! Synchronization and self-update are mutually exclusive. This avoids exiting
//! the parent while its Git/LFS children are still using the repository index.
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
#[derive(Default)]
struct Activity {
    repositories: HashSet<PathBuf>,
    paused: bool,
}
fn registry() -> &'static Mutex<Activity> {
    static STATE: OnceLock<Mutex<Activity>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(Activity::default()))
}
pub(crate) struct SyncGuard(PathBuf);
impl Drop for SyncGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = registry().lock() {
            state.repositories.remove(&self.0);
        }
    }
}
pub(crate) fn begin_sync(path: &Path) -> Result<SyncGuard, String> {
    let mut state = registry().lock().map_err(|e| e.to_string())?;
    if state.paused {
        return Err("程序正在更新或退出，暂时不能启动软件仓库同步".into());
    }
    let key = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    if !state.repositories.insert(key.clone()) {
        return Err("此软件仓库已有同步任务，请等待当前任务结束".into());
    }
    Ok(SyncGuard(key))
}
pub(crate) struct PauseGuard;
impl Drop for PauseGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = registry().lock() {
            state.paused = false;
        }
    }
}
pub(crate) fn pause_if_idle() -> Result<PauseGuard, String> {
    let mut state = registry().lock().map_err(|e| e.to_string())?;
    if state.paused {
        return Err("程序更新或退出已在进行".into());
    }
    if !state.repositories.is_empty() {
        return Err("软件仓库仍在同步，Git/LFS 子进程尚未结束。请等待同步完成或失败后再更新/退出，避免遗留 index.lock。".into());
    }
    state.paused = true;
    Ok(PauseGuard)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synchronization_blocks_update_and_same_repository_reentry() {
        let path = std::env::temp_dir().join("keeper-activity-test");
        let sync = begin_sync(&path).unwrap();
        assert!(begin_sync(&path).is_err());
        assert!(pause_if_idle().is_err());
        drop(sync);
        let pause = pause_if_idle().unwrap();
        assert!(begin_sync(&path).is_err());
        drop(pause);
        assert!(begin_sync(&path).is_ok());
    }
}
