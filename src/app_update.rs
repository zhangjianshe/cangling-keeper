use serde::Serialize;
use std::time::Duration;

const VERSION_URL: &str = "https://soft.cangling.cn:22002/software/operator/latest/version.txt";
const DOWNLOAD_BASE: &str = "https://soft.cangling.cn:22002/software/operator/latest";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateStatus {
    current: String,
    latest: String,
    update_available: bool,
    download_url: Option<String>,
}

fn download_url(os: &str, arch: &str) -> Option<String> {
    let filename = match (os, arch) {
        ("linux", "x86_64") => "cangling-keeper-linux-amd64",
        ("linux", "aarch64") => "cangling-keeper-linux-arm64",
        ("windows", "x86_64") => "cangling-keeper-windows-x86_64.exe",
        _ => return None,
    };
    Some(format!("{DOWNLOAD_BASE}/{filename}"))
}

fn parse_version(text: &str) -> Result<String, String> {
    let version = text.trim().trim_start_matches('\u{feff}').trim();
    let numeric = version.strip_prefix('v').unwrap_or(version);
    let parts: Vec<_> = numeric.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_digit())
                || part.parse::<u64>().is_err()
        })
    {
        return Err("版本服务器未返回有效的版本号".into());
    }
    Ok(version.to_owned())
}

async fn fetch_version(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("检查更新失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("检查更新失败：{error}"))?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if bytes.len() + chunk.len() > 128 {
            return Err("版本服务器响应过长".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_version(std::str::from_utf8(&bytes).map_err(|_| "版本号编码错误")?)
}

pub(crate) async fn check() -> Result<UpdateStatus, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?;
    let latest = fetch_version(&client, VERSION_URL).await?;
    let current = format!("v{}", env!("CARGO_PKG_VERSION"));
    Ok(UpdateStatus {
        update_available: crate::host_actions::is_newer(&latest, &current),
        current,
        latest,
        download_url: download_url(std::env::consts::OS, std::env::consts::ARCH),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_error_pages_and_invalid_version_markers() {
        assert_eq!(parse_version("\u{feff}v0.0.94\r\n").unwrap(), "v0.0.94");
        for invalid in [
            "",
            "<html>Bad Gateway</html>",
            "v0.0",
            "1..2",
            "1.2.3.4",
            "1.2.3 error",
        ] {
            assert!(parse_version(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn downloads_match_server_platform() {
        assert!(
            download_url("linux", "x86_64")
                .unwrap()
                .ends_with("cangling-keeper-linux-amd64")
        );
        assert!(
            download_url("linux", "aarch64")
                .unwrap()
                .ends_with("cangling-keeper-linux-arm64")
        );
        assert!(
            download_url("windows", "x86_64")
                .unwrap()
                .ends_with("cangling-keeper-windows-x86_64.exe")
        );
        assert!(download_url("windows", "aarch64").is_none());
    }
}

// The copied helper runs independently of the web server so Windows can release
// its executable lock before replacement. No shell receives file paths or args.
#[derive(serde::Serialize, serde::Deserialize)]
struct InstallPlan {
    pid: u32,
    target: std::path::PathBuf,
    staged: std::path::PathBuf,
    backup: std::path::PathBuf,
    args: Vec<String>,
    cwd: std::path::PathBuf,
    health_url: String,
    latest: String,
    result: std::path::PathBuf,
}
static UPDATING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn helper_mode() -> Option<Result<(), String>> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--self-update-probe") {
        println!("keeper-update-probe:{}", env!("CARGO_PKG_VERSION"));
        return Some(Ok(()));
    }
    if args.get(1).map(String::as_str) == Some("--apply-self-update") {
        return Some(
            args.get(2)
                .ok_or_else(|| "缺少更新计划".to_string())
                .and_then(|path| run_helper(std::path::Path::new(path))),
        );
    }
    None
}

pub(crate) async fn prepare(
    data_dir: &std::path::Path,
    progress: impl Fn(serde_json::Value),
) -> Result<(std::path::PathBuf, String), String> {
    use std::sync::atomic::Ordering;
    if UPDATING.swap(true, Ordering::SeqCst) {
        return Err("程序更新正在进行".into());
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            UPDATING.store(false, Ordering::SeqCst);
        }
    }
    let guard = Guard;
    let result = prepare_inner(data_dir, progress).await;
    if result.is_ok() {
        std::mem::forget(guard);
    }
    result
}

async fn prepare_inner(
    data_dir: &std::path::Path,
    progress: impl Fn(serde_json::Value),
) -> Result<(std::path::PathBuf, String), String> {
    use std::io::Write;
    let status = check().await?;
    if !status.update_available {
        return Err("当前已是最新版本".into());
    }
    let url = status.download_url.ok_or("当前平台没有更新程序")?;
    let target = std::env::current_exe().map_err(|e| e.to_string())?;
    let stage = target
        .parent()
        .ok_or("程序路径无效")?
        .join(format!(".keeper-update-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&stage).map_err(|e| format!("程序目录不可写，无法安装更新：{e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let result = async {
        let staged = stage.join(if cfg!(windows) { "new.exe" } else { "new" });
        let client = reqwest::Client::builder().timeout(Duration::from_secs(600)).build().map_err(|e| e.to_string())?;
        let mut response = client.get(&url).send().await.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?;
        let total = response.content_length().unwrap_or(0);
        if total > 128 * 1024 * 1024 { return Err("更新程序超过大小限制".into()); }
        let mut file = std::fs::File::create(&staged).map_err(|e| e.to_string())?;
        let mut received = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            received += chunk.len() as u64;
            if received > 128 * 1024 * 1024 { return Err("更新程序超过大小限制".into()); }
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            progress(serde_json::json!({"phase":"download","received":received,"total":total,"pct":if total>0 {received*100/total} else {0}}));
        }
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        if total != 0 && received != total { return Err("更新程序下载不完整".into()); }
        validate_program(&std::fs::read(&staged).map_err(|e| e.to_string())?, std::env::consts::OS, std::env::consts::ARCH)?;
        std::fs::set_permissions(&staged, std::fs::metadata(&target).map_err(|e| e.to_string())?.permissions()).map_err(|e| e.to_string())?;
        let probe = tokio::time::timeout(Duration::from_secs(15), tokio::process::Command::new(&staged).arg("--self-update-probe").output()).await
            .map_err(|_| "更新程序启动验证超时")?.map_err(|e| format!("更新程序无法运行：{e}"))?;
        if !probe.status.success() || String::from_utf8_lossy(&probe.stdout).trim() != format!("keeper-update-probe:{}", status.latest.trim_start_matches('v')) {
            return Err("更新程序版本或运行验证失败，原程序保持不变".into());
        }
        if fetch_version(&client, VERSION_URL).await? != status.latest { return Err("下载期间服务器版本发生变化，请重新更新".into()); }
        let result_path = data_dir.join("app-update-result.json");
        let plan = InstallPlan {
            pid: std::process::id(), target: target.clone(), staged,
            backup: stage.join(if cfg!(windows) {"previous.exe"} else {"previous"}),
            args: std::env::args().skip(1).collect(), cwd: std::env::current_dir().map_err(|e| e.to_string())?,
            health_url: format!("{}/api/health", crate::web::home_url_from_args()?),
            latest: status.latest.clone(), result: result_path,
        };
        write_result(&plan, "prepared", "下载验证完成，准备重启");
        let path = stage.join("plan.json");
        std::fs::write(&path, serde_json::to_vec(&plan).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let helper = stage.join(if cfg!(windows) { "helper.exe" } else { "helper" });
        std::fs::copy(&target, &helper).map_err(|e| e.to_string())?;
        progress(serde_json::json!({"phase":"install"}));
        std::process::Command::new(helper).arg("--apply-self-update").arg(&path)
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .spawn().map_err(|e| format!("无法启动更新辅助进程：{e}"))?;
        Ok((path, status.latest))
    }.await;
    if result.is_err() {
        let _ = std::fs::remove_dir_all(stage);
    }
    result
}

fn validate_program(bytes: &[u8], os: &str, arch: &str) -> Result<(), String> {
    let valid = if os == "linux" {
        bytes.len() >= 64
            && bytes.starts_with(b"\x7fELF")
            && bytes[4] == 2
            && bytes[5] == 1
            && u16::from_le_bytes([bytes[18], bytes[19]])
                == if arch == "aarch64" { 183 } else { 62 }
    } else if os == "windows" && bytes.len() >= 64 && bytes.starts_with(b"MZ") {
        let offset = u32::from_le_bytes(bytes[60..64].try_into().unwrap()) as usize;
        bytes
            .get(offset..offset.saturating_add(6))
            .is_some_and(|p| p == b"PE\0\0\x64\x86")
    } else {
        false
    };
    if valid {
        Ok(())
    } else {
        Err("下载内容不是当前平台的有效程序，取消更新".into())
    }
}

fn write_result(plan: &InstallPlan, phase: &str, message: &str) {
    let _ = std::fs::write(
        &plan.result,
        serde_json::json!({"phase":phase,"message":message,"version":plan.latest}).to_string(),
    );
}

fn run_helper(path: &std::path::Path) -> Result<(), String> {
    let plan: InstallPlan =
        serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let result = install_and_restart(&plan);
    if let Err(ref error) = result {
        write_result(&plan, "error", error);
    }
    result
}

fn install_and_restart(plan: &InstallPlan) -> Result<(), String> {
    #[cfg(windows)]
    {
        let script = format!(
            "$p = Get-Process -Id {} -ErrorAction SilentlyContinue; if ($p -and -not $p.WaitForExit(30000)) {{ exit 1 }}; exit 0",
            plan.pid
        );
        let status = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("等待原程序退出超时".into());
        }
    }
    #[cfg(target_os = "linux")]
    {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::path::Path::new(&format!("/proc/{}", plan.pid)).exists() {
            if std::time::Instant::now() >= deadline {
                return Err("等待原程序退出超时".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    write_result(plan, "restarting", "正在替换程序并重启");
    std::fs::rename(&plan.target, &plan.backup).map_err(|e| format!("无法备份原程序：{e}"))?;
    let result = (|| {
        std::fs::rename(&plan.staged, &plan.target).map_err(|e| e.to_string())?;
        let mut child = start_program(plan)?;
        let healthy = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?
            .block_on(async {
                let client = reqwest::Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_secs(1))
                    .build()
                    .map_err(|e| e.to_string())?;
                let mut healthy_checks = 0;
                for _ in 0..60 {
                    if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                        return Ok(false);
                    }
                    let mut good = false;
                    if let Ok(response) = client.get(&plan.health_url).send().await {
                        if let Ok(body) = response.json::<serde_json::Value>().await {
                            if body["pid"].as_u64() == Some(u64::from(child.id()))
                                && body["ok"].as_bool() == Some(true)
                                && body["version"].as_str()
                                    == Some(plan.latest.trim_start_matches('v'))
                            {
                                good = true;
                            }
                        }
                    }
                    healthy_checks = if good { healthy_checks + 1 } else { 0 };
                    if healthy_checks >= 2 {
                        return Ok(true);
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Ok::<_, String>(false)
            });
        if healthy != Ok(true) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("新程序启动验证失败".to_string());
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_file(&plan.target);
        std::fs::rename(&plan.backup, &plan.target)
            .map_err(|e| format!("{error}；恢复原程序失败：{e}"))?;
        start_program(plan)?;
        write_result(plan, "rolled_back", &format!("{error}，已恢复原程序"));
        return Ok(());
    }
    write_result(plan, "complete", "程序更新成功");
    Ok(())
}

fn start_program(plan: &InstallPlan) -> Result<std::process::Child, String> {
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(plan.result.with_file_name("app-update.log"))
        .map_err(|e| e.to_string())?;
    let mut args = plan.args.clone();
    if !args.iter().any(|arg| arg == "--no-open") {
        args.push("--no-open".into());
    }
    std::process::Command::new(&plan.target)
        .args(args)
        .current_dir(&plan.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| format!("启动程序失败：{e}"))
}

#[cfg(test)]
mod install_tests {
    use super::*;

    #[test]
    fn rejects_wrong_platform_and_error_pages() {
        assert!(validate_program(b"<html>Bad Gateway</html>", "linux", "x86_64").is_err());
        let mut elf = vec![0u8; 64];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[18] = 62;
        assert!(validate_program(&elf, "linux", "x86_64").is_ok());
        assert!(validate_program(&elf, "linux", "aarch64").is_err());
        let mut pe = vec![0u8; 128];
        pe[..2].copy_from_slice(b"MZ");
        pe[60] = 64;
        pe[64..70].copy_from_slice(b"PE\0\0\x64\x86");
        assert!(validate_program(&pe, "windows", "x86_64").is_ok());
        pe[60] = 255;
        assert!(validate_program(&pe, "windows", "x86_64").is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn replaces_and_restarts_or_restores_previous_program() {
        use std::io::{Read, Write};
        use std::os::unix::fs::PermissionsExt;
        for fail in [false, true] {
            let root = std::env::temp_dir().join(format!("keeper update {}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            listener.set_nonblocking(true).unwrap();
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stopped = stop.clone();
            let pid_path = root.join("pid");
            let server = std::thread::spawn(move || {
                while !stopped.load(std::sync::atomic::Ordering::SeqCst) {
                    if let Ok((mut socket, _)) = listener.accept() {
                        let _ = socket.set_read_timeout(Some(Duration::from_secs(1)));
                        let mut buf = [0; 4096];
                        let _ = socket.read(&mut buf);
                        let pid = std::fs::read_to_string(&pid_path)
                            .ok()
                            .and_then(|p| p.trim().parse::<u32>().ok())
                            .unwrap_or(0);
                        let body =
                            serde_json::json!({"ok":true,"version":"9.9.9","pid":pid}).to_string();
                        let _ = write!(
                            socket,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                    } else {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            });
            let old = format!(
                "#!/bin/sh\necho $$ > '{}'\necho old > '{}'\nexec sleep 60\n",
                root.join("pid").display(),
                root.join("started").display()
            );
            let new = if fail {
                "#!/bin/sh\nexit 1\n".to_string()
            } else {
                format!(
                    "#!/bin/sh\necho $$ > '{}'\nprintf '%s\\n' \"$@\" > '{}'\npwd > '{}'\necho new > '{}'\nexec sleep 60\n",
                    root.join("pid").display(),
                    root.join("args").display(),
                    root.join("cwd").display(),
                    root.join("started").display()
                )
            };
            let plan = InstallPlan {
                pid: u32::MAX,
                target: root.join("keeper"),
                staged: root.join("new"),
                backup: root.join("previous"),
                args: vec!["--data-dir".into(), "path with spaces".into()],
                cwd: root.clone(),
                health_url: format!("http://{address}/api/health"),
                latest: "v9.9.9".into(),
                result: root.join("result.json"),
            };
            std::fs::write(&plan.target, &old).unwrap();
            std::fs::write(&plan.staged, &new).unwrap();
            for p in [&plan.target, &plan.staged] {
                std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            install_and_restart(&plan).unwrap();
            for _ in 0..100 {
                if root.join("started").exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let result: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&plan.result).unwrap()).unwrap();
            assert_eq!(
                result["phase"],
                if fail { "rolled_back" } else { "complete" }
            );
            assert_eq!(
                std::fs::read_to_string(root.join("started"))
                    .unwrap()
                    .trim(),
                if fail { "old" } else { "new" }
            );
            if !fail {
                assert_eq!(std::fs::read_to_string(&plan.backup).unwrap(), old);
                assert_eq!(
                    std::fs::read_to_string(root.join("args")).unwrap(),
                    "--data-dir\npath with spaces\n--no-open\n"
                );
                assert_eq!(
                    std::fs::read_to_string(root.join("cwd")).unwrap().trim(),
                    root.to_str().unwrap()
                );
            } else {
                assert_eq!(std::fs::read_to_string(&plan.target).unwrap(), old);
            }
            let pid = std::fs::read_to_string(root.join("pid")).unwrap();
            let _ = std::process::Command::new("kill").arg(pid.trim()).status();
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
            server.join().unwrap();
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
