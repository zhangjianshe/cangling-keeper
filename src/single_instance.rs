#[cfg(unix)]
mod platform {
    use std::fs::{File, OpenOptions};
    use std::io::{self, Write};
    use std::os::fd::AsRawFd;
    use std::path::Path;

    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;

    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }

    pub struct SingleInstanceGuard {
        _file: File,
    }

    fn acquire_path(path: &Path) -> Result<SingleInstanceGuard, String> {
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| format!("无法打开单实例锁 {}：{e}", path.display()))?;

        // SAFETY: `file` owns a valid descriptor for the duration of this call.
        let result = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::WouldBlock {
                return Err("cangling-keeper 已经在运行，本次启动已取消".into());
            }
            return Err(format!("无法获取 cangling-keeper 单实例锁：{error}"));
        }

        file.set_len(0)
            .and_then(|_| write!(file, "{}\n", std::process::id()))
            .map_err(|e| format!("无法记录 cangling-keeper 进程号：{e}"))?;
        Ok(SingleInstanceGuard { _file: file })
    }

    pub fn acquire() -> Result<SingleInstanceGuard, String> {
        acquire_path(&std::env::temp_dir().join("cangling-keeper.lock"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn rejects_a_second_live_lock_and_recovers_after_drop() {
            let path = std::env::temp_dir()
                .join(format!("cangling-keeper-test-{}.lock", std::process::id()));
            let first = acquire_path(&path).expect("first lock");
            assert!(acquire_path(&path).is_err());
            drop(first);
            let second = acquire_path(&path).expect("lock after first process exits");
            drop(second);
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use std::io;

    const ERROR_ALREADY_EXISTS: u32 = 183;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(
            attributes: *mut c_void,
            initial_owner: i32,
            name: *const u16,
        ) -> *mut c_void;
        fn GetLastError() -> u32;
        fn CloseHandle(object: *mut c_void) -> i32;
    }

    pub struct SingleInstanceGuard {
        handle: *mut c_void,
    }

    impl Drop for SingleInstanceGuard {
        fn drop(&mut self) {
            // SAFETY: `handle` was returned by CreateMutexW and is closed once.
            unsafe { CloseHandle(self.handle) };
        }
    }

    pub fn acquire() -> Result<SingleInstanceGuard, String> {
        let name: Vec<u16> = "Global\\cangling-keeper-single-instance\0"
            .encode_utf16()
            .collect();
        // SAFETY: the name is a valid NUL-terminated UTF-16 string and the
        // returned handle is retained by the guard.
        let handle = unsafe { CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(format!(
                "无法创建 cangling-keeper 单实例锁：{}",
                io::Error::last_os_error()
            ));
        }
        // SAFETY: GetLastError has no preconditions and must be read directly
        // after CreateMutexW.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: the returned handle is valid even when the mutex exists.
            unsafe { CloseHandle(handle) };
            return Err("cangling-keeper 已经在运行，本次启动已取消".into());
        }
        Ok(SingleInstanceGuard { handle })
    }
}

#[cfg(not(any(unix, windows)))]
compile_error!("single-instance support is required for this platform");

pub use platform::acquire;
