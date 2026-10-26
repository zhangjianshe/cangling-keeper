use std::path::{Path, PathBuf};

const APP_NAME: &str = "cangling-keeper";

pub fn ensure_exists() -> Result<Option<PathBuf>, String> {
    let desktop = crate::platform_dirs::desktop_dir().ok_or("无法确定桌面目录")?;
    std::fs::create_dir_all(&desktop)
        .map_err(|error| format!("无法创建桌面目录 {}：{error}", desktop.display()))?;
    ensure_platform_shortcut(&desktop)
}

#[cfg(target_os = "windows")]
fn ensure_platform_shortcut(desktop: &Path) -> Result<Option<PathBuf>, String> {
    let shortcut = desktop.join(format!("{APP_NAME}.lnk"));
    if shortcut.exists() {
        return Ok(None);
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let working_directory = executable.parent().unwrap_or(Path::new("."));
    let quote = |value: &Path| value.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.WorkingDirectory='{}';$s.IconLocation='{},0';$s.Description='Cangling Keeper';$s.Save()",
        quote(&shortcut),
        quote(&executable),
        quote(working_directory),
        quote(&executable),
    );
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .map_err(|error| format!("无法创建桌面快捷方式：{error}"))?;
    if !status.success() || !shortcut.exists() {
        return Err("PowerShell 未能创建桌面快捷方式".into());
    }
    Ok(Some(shortcut))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn ensure_platform_shortcut(desktop: &Path) -> Result<Option<PathBuf>, String> {
    use std::os::unix::fs::PermissionsExt;

    let shortcut = desktop.join(format!("{APP_NAME}.desktop"));
    if shortcut.exists() {
        return Ok(None);
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let executable = quote_desktop_exec(&executable);
    let content = format!(
        "[Desktop Entry]\nType=Application\nName=Cangling Keeper\nComment=Open Cangling Keeper\nExec={executable}\nTerminal=false\nCategories=Utility;Network;\nStartupNotify=true\n"
    );
    std::fs::write(&shortcut, content)
        .map_err(|error| format!("无法写入桌面快捷方式 {}：{error}", shortcut.display()))?;
    let mut permissions = std::fs::metadata(&shortcut)
        .map_err(|error| error.to_string())?
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&shortcut, permissions)
        .map_err(|error| format!("无法设置桌面快捷方式权限：{error}"))?;
    // GNOME-based desktops may otherwise display the launcher as an
    // untrusted text file even though it is executable. Other desktops and
    // systems without `gio` simply ignore this best-effort metadata step.
    let _ = std::process::Command::new("gio")
        .args(["set", shortcut.to_string_lossy().as_ref(), "metadata::trusted", "true"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    Ok(Some(shortcut))
}

#[cfg(target_os = "macos")]
fn ensure_platform_shortcut(_desktop: &Path) -> Result<Option<PathBuf>, String> {
    Ok(None)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn quote_desktop_exec(path: &Path) -> String {
    let escaped = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn quotes_linux_desktop_exec_paths() {
        let quoted = super::quote_desktop_exec(std::path::Path::new("/opt/Cangling Keeper/app"));
        assert_eq!(quoted, "\"/opt/Cangling Keeper/app\"");
    }
}
