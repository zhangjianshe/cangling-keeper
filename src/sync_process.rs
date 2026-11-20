//! Own repository subprocess trees; an application exit must not orphan Git/LFS.
use std::{
    io,
    process::{Child, Command},
};

#[cfg(not(windows))]
pub(crate) struct ProcessTree;
#[cfg(not(windows))]
pub(crate) fn spawn(cmd: &mut Command) -> io::Result<(Child, ProcessTree)> {
    cmd.spawn().map(|child| (child, ProcessTree))
}
#[cfg(not(windows))]
pub(crate) fn shutdown() {}

#[cfg(windows)]
pub(crate) use windows::{ProcessTree, shutdown, spawn};

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        mem::{size_of, zeroed},
        os::windows::{io::AsRawHandle, process::CommandExt},
        ptr::null,
        sync::{Arc, Mutex, OnceLock, Weak},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject,
            },
            Threading::{
                CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
            },
        },
    };

    struct Handle(HANDLE);
    // Kernel handles can be used and closed across threads. Arc owns their lifetime.
    unsafe impl Send for Handle {}
    unsafe impl Sync for Handle {}
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    pub(crate) struct ProcessTree(Arc<Handle>);
    #[derive(Default)]
    struct Registry {
        stopping: bool,
        jobs: Vec<Weak<Handle>>,
    }
    fn registry() -> &'static Mutex<Registry> {
        static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
        REGISTRY.get_or_init(Default::default)
    }
    pub(crate) fn shutdown() {
        let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
        registry.stopping = true;
        for job in registry.jobs.drain(..).filter_map(|job| job.upgrade()) {
            unsafe {
                TerminateJobObject(job.0, 1);
            }
        }
    }
    pub(crate) fn spawn(cmd: &mut Command) -> io::Result<(Child, ProcessTree)> {
        // Serialize launch with shutdown. Assign the suspended process before it
        // can create children, so Git's shell/LFS descendants inherit the job.
        let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
        if registry.stopping {
            return Err(io::Error::other("Keeper is shutting down"));
        }
        unsafe {
            let job = CreateJobObjectW(null(), null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Arc::new(Handle(job));
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            cmd.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
            let mut child = cmd.spawn()?;
            let setup = (|| {
                if AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) == 0 {
                    return Err(io::Error::last_os_error());
                }
                resume_initial_thread(child.id())
            })();
            if let Err(error) = setup {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            registry.jobs.retain(|job| job.strong_count() > 0);
            registry.jobs.push(Arc::downgrade(&job));
            Ok((child, ProcessTree(job)))
        }
    }
    unsafe fn resume_initial_thread(pid: u32) -> io::Result<()> {
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let snapshot = Handle(snapshot);
            let mut entry: THREADENTRY32 = zeroed();
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            let mut found = Thread32First(snapshot.0, &mut entry);
            while found != 0 {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if thread.is_null() {
                        return Err(io::Error::last_os_error());
                    }
                    let thread = Handle(thread);
                    return if ResumeThread(thread.0) == u32::MAX {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    };
                }
                found = Thread32Next(snapshot.0, &mut entry);
            }
            Err(io::Error::other(
                "Suspended Git process has no initial thread",
            ))
        }
    }
    impl Drop for ProcessTree {
        fn drop(&mut self) {
            // Closing the last job handle also kills descendants left behind
            // after the direct Git child has exited or an output read failed.
            let _ = &self.0;
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{
        fs,
        path::Path,
        thread,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, SYNCHRONIZATION_SYNCHRONIZE, WaitForSingleObject},
    };

    fn wait_pid(path: &Path) -> u32 {
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            if let Ok(value) = fs::read_to_string(path) {
                if let Ok(pid) = value.trim().parse() {
                    return pid;
                }
            }
            assert!(Instant::now() < until, "child did not start");
            thread::sleep(Duration::from_millis(100));
        }
    }
    fn assert_gone(pid: u32) {
        unsafe {
            let handle = OpenProcess(SYNCHRONIZATION_SYNCHRONIZE, 0, pid);
            if handle.is_null() {
                return;
            }
            let status = WaitForSingleObject(handle, 10000);
            CloseHandle(handle);
            assert_eq!(status, WAIT_OBJECT_0, "process {pid} survived");
        }
    }
    // Run in a separate process: shutdown intentionally prevents future launches.
    #[test]
    #[ignore]
    fn job_helper() {
        let Some(root) = std::env::var_os("KEEPER_JOB_TEST_ROOT") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let pidfile = root.join("grandchild");
        let script = format!(
            "$p=Start-Process ping.exe -ArgumentList '-t','127.0.0.1' -WindowStyle Hidden -PassThru; [IO.File]::WriteAllText('{}',[string]$p.Id); Start-Sleep -Seconds 120",
            pidfile.display().to_string().replace('\'', "''")
        );
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        let (mut child, tree) = spawn(&mut command).unwrap();
        fs::write(root.join("child"), child.id().to_string()).unwrap();
        wait_pid(&pidfile);
        match std::env::var("KEEPER_JOB_TEST_MODE").unwrap().as_str() {
            "drop" => drop(tree),
            "shutdown" => {
                shutdown();
                assert!(spawn(&mut command).is_err());
            }
            "abort" => {
                fs::write(root.join("ready"), b"ready").unwrap();
                thread::sleep(Duration::from_secs(120));
            }
            _ => unreachable!(),
        }
        let _ = child.wait();
    }
    #[test]
    fn windows_jobs_clean_up_on_drop_shutdown_and_forced_keeper_exit() {
        for mode in ["drop", "shutdown", "abort"] {
            let root =
                std::env::temp_dir().join(format!("keeper-job-{}-{mode}", std::process::id()));
            fs::create_dir_all(&root).unwrap();
            let mut helper = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "sync_process::tests::job_helper",
                    "--ignored",
                    "--nocapture",
                ])
                .env("KEEPER_JOB_TEST_ROOT", &root)
                .env("KEEPER_JOB_TEST_MODE", mode)
                .spawn()
                .unwrap();
            let child = wait_pid(&root.join("child"));
            let grandchild = wait_pid(&root.join("grandchild"));
            if mode == "abort" {
                let until = Instant::now() + Duration::from_secs(20);
                while !root.join("ready").exists() {
                    assert!(Instant::now() < until);
                    thread::sleep(Duration::from_millis(50));
                }
                helper.kill().unwrap();
            }
            let status = helper.wait().unwrap();
            if mode != "abort" {
                assert!(status.success());
            }
            assert_gone(child);
            assert_gone(grandchild);
            fs::remove_dir_all(root).unwrap();
        }
    }
}
