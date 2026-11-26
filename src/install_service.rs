//! Manual Linux systemd installation, kept separate from normal application startup.
pub(crate) fn handle() -> Option<Result<(), String>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("restart") {
        return Some(restart(&args[1..]));
    }
    let uninstall = match args.first().map(String::as_str) {
        Some("install-service") => false,
        Some("uninstall-service") => true,
        _ => return None,
    };
    Some(install(&args[1..], uninstall))
}

#[cfg(target_os = "linux")]
fn install(args: &[String], uninstall: bool) -> Result<(), String> {
    // The embedded installer validates options and generated units before any writes.
    let script = if uninstall { include_str!("../scripts/uninstall-socket-activation.py") }
        else { include_str!("../scripts/install-socket-activation.py") };
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let check = args.iter().any(|s| s == "--check" || s == "--help" || s == "-h");
    if !check && unsafe { libc::geteuid() } != 0 {
        return Err("需要管理员账户，请使用 sudo cangling-keeper install-service 或 sudo cangling-keeper uninstall-service".into());
    }
    // Preflight without elevation also rejects invalid options before asking for sudo.
    if !check {
        let status = std::process::Command::new("python3").arg("-c").arg(script)
            .arg(&exe).args(args).arg("--check").status().map_err(|e| format!("需要 python3：{e}"))?;
        if !status.success() { return Err("服务配置预检失败".into()); }
    }
    let mut command = std::process::Command::new("python3");
    command.env("KEEPER_DESKTOP_ICON", include_str!("../ui/logo.svg"));
    command.env("KEEPER_DESKTOP_INSTALL_SCRIPT", include_str!("../scripts/install-desktop-linux.py"));
    let status = command.arg("-c").arg(script).arg(exe).args(args).status()
        .map_err(|e| format!("无法运行服务安装器：{e}"))?;
    if status.success() { Ok(()) } else { Err(format!("服务安装器退出：{status}")) }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn install(_: &[String], _: bool) -> Result<(), String> {
    Err("服务安装/卸载命令目前仅支持使用 systemd 的 Linux".into())
}

#[cfg(target_os = "windows")]
fn install(args: &[String], uninstall: bool) -> Result<(), String> {
    if args.iter().any(|s| s == "--help" || s == "-h") {
        println!("install-service [--http-port 80] [--https-port 443] [--check]\nWindows：创建手动启动快捷方式，不注册 Windows 服务。");
        return Ok(());
    }
    let mut http: u16 = 80;
    let mut https: u16 = 443;
    let mut check = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => check = true,
            "--http-port" | "--https-port" if !uninstall => {
                let name = &args[i]; i += 1;
                let port = args.get(i).ok_or("缺少端口")?.parse::<u16>().map_err(|_| "无效端口")?;
                if port == 0 { return Err("端口不能为 0".into()); }
                if name == "--http-port" { http = port; } else { https = port; }
            },
            _ => return Err("未知参数".into()),
        }
        i += 1;
    }
    if http == https { return Err("HTTP 和 HTTPS 端口不能相同".into()); }
    if check { println!("Windows 手动启动快捷方式；HTTP {http}，HTTPS {https}，未修改系统。"); return Ok(()); }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = executable.to_string_lossy().replace('\'', "''");
    let action = if uninstall {
        "if(Test-Path -LiteralPath $path){Remove-Item -LiteralPath $path};Write-Output '快捷方式已移除，用户数据保留'".to_string()
    } else {
        format!("$s=(New-Object -ComObject WScript.Shell).CreateShortcut($path);$s.TargetPath='{exe}';$s.Arguments='--listen 127.0.0.1:{http} --https-listen 127.0.0.1:{https}';$s.WorkingDirectory=Split-Path -LiteralPath '{exe}';$s.IconLocation='{exe},0';$s.Description='Cangling Keeper';$s.Save();Write-Output '桌面快捷方式已安装（手动启动）'")
    };
    let script = format!("$ErrorActionPreference='Stop';if(-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){{Write-Error '需要管理员账户';exit 1}};$path=Join-Path ([Environment]::GetFolderPath('CommonDesktopDirectory')) 'cangling-keeper.lnk';{action}");
    let result = std::process::Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",&script]).status().map_err(|e| e.to_string())?;
    if result.success() { Ok(()) } else { Err("快捷方式安装/卸载失败，需要管理员账户".into()) }
}

fn restart(args: &[String]) -> Result<(), String> {
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        println!("cangling-keeper restart\n重启已安装的 Linux systemd 服务；会中断当前连接和运行任务。");
        return Ok(());
    }
    if !args.is_empty() { return Err("用法：cangling-keeper restart".into()); }
    #[cfg(target_os = "linux")]
    {
        let status = std::process::Command::new("systemctl")
            .args(["--no-ask-password", "restart", "cangling-keeper-local.service"])
            .status().map_err(|e| format!("无法重启服务：{e}"))?;
        if !status.success() {
            return Err("重启失败，请确认已安装服务且当前账户具有服务控制权限".into());
        }
        println!("Keeper 服务已重启。");
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    { Err("restart 目前仅支持已安装 systemd 服务的 Linux".into()) }
}
