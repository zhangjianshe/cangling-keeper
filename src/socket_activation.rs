//! Receive systemd listeners before threads, files, or subprocesses are created.
#[derive(Default)]
pub(crate) struct Listeners {
    pub http: Option<std::net::TcpListener>,
    pub https: Option<std::net::TcpListener>,
}
#[cfg(target_os = "linux")]
pub(crate) fn receive() -> Result<Listeners, String> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let pid = std::env::var("LISTEN_PID").ok();
    if pid.as_deref().and_then(|v| v.parse::<u32>().ok()) != Some(std::process::id()) {
        return Ok(Listeners::default());
    }
    let count = std::env::var("LISTEN_FDS")
        .map_err(|_| "缺少 LISTEN_FDS")?
        .parse::<usize>()
        .map_err(|_| "无效 LISTEN_FDS")?;
    if count != 2 {
        return Err("需要且仅支持两个 systemd socket（http、https）".into());
    }
    // SAFETY: This function is called at entry, before spawning any threads.
    let names = std::env::var("LISTEN_FDNAMES").unwrap_or_default();
    unsafe {
        for key in ["LISTEN_PID", "LISTEN_FDS", "LISTEN_FDNAMES"] {
            std::env::remove_var(key);
        }
    }
    let mut fds = Vec::new();
    for fd in 3..5 {
        // SAFETY: LISTEN_PID identifies this process; systemd transfers ownership of FDs 3 and 4.
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        fds.push(unsafe { OwnedFd::from_raw_fd(fd) });
    }
    let names: Vec<_> = names.split(':').collect();
    if names.len() != 2 || !names.contains(&"http") || !names.contains(&"https") {
        return Err("systemd socket 必须分别命名 http、https".into());
    }
    let mut result = Listeners::default();
    for (name, fd) in names.into_iter().zip(fds) {
        let mut accepting: libc::c_int = 0;
        let mut len = std::mem::size_of_val(&accepting) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_ACCEPTCONN,
                (&mut accepting as *mut libc::c_int).cast(),
                &mut len,
            )
        } != 0
            || accepting != 1
        {
            return Err("继承的描述符不是监听 socket".into());
        }
        let listener = std::net::TcpListener::from(fd);
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        if !addr.ip().is_loopback() {
            return Err("继承的 socket 必须监听本机回环地址".into());
        }
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        if name == "http" {
            result.http = Some(listener)
        } else {
            result.https = Some(listener)
        }
    }
    Ok(result)
}
#[cfg(not(target_os = "linux"))]
pub(crate) fn receive() -> Result<Listeners, String> {
    Ok(Listeners::default())
}

impl Listeners {
    pub fn validate(
        &self,
        http: std::net::SocketAddr,
        https: Option<std::net::SocketAddr>,
    ) -> Result<(), String> {
        for (name, listener, expected) in [
            ("http", &self.http, Some(http)),
            ("https", &self.https, https),
        ] {
            if let Some(listener) = listener {
                if Some(listener.local_addr().map_err(|e| e.to_string())?) != expected {
                    return Err(format!("继承的 {name} socket 与配置地址不一致"));
                }
            }
        }
        Ok(())
    }
}
