use serde::Serialize;
use tauri::AppHandle;

/// Update server base URL (same server the GitHub workflow pushes bundles to).
const UPDATE_BASE_URL: &str = "https://soft.cangling.cn:22002";
/// Software id for cangling-keeper on the update server. This id is bound to
/// the upload token `63cef07f4ee54823b25c652b0268f71d` used in release.yml.
const SOFTWARE_ID: &str = "1def62c273cd47eea1495c098ba34496";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateStatus {
    pub current: String,
    pub latest: String,
    pub update_available: bool,
    pub error: String,
}

fn client() -> Result<reqwest::Client, String> {
    // The update server uses a self-signed certificate (the existing shell
    // scripts use `curl -k`), so accept it the same way here.
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())
}

fn version_url() -> String {
    format!("{UPDATE_BASE_URL}/software/{SOFTWARE_ID}/latest/version.txt")
}

/// Stable filename the workflow uploads for each platform.
fn bundle_file() -> Result<&'static str, String> {
    if cfg!(target_os = "windows") {
        Ok("cangling-keeper-setup.exe")
    } else if cfg!(target_os = "linux") {
        Ok("cangling-keeper.AppImage")
    } else if cfg!(target_os = "macos") {
        Ok("cangling-keeper.dmg")
    } else {
        Err("unsupported platform".into())
    }
}

fn bundle_url() -> Result<String, String> {
    Ok(format!(
        "{UPDATE_BASE_URL}/software/{SOFTWARE_ID}/latest/{}",
        bundle_file()?
    ))
}

async fn fetch_latest() -> Result<String, String> {
    let resp = client()?
        .get(version_url())
        .send()
        .await
        .map_err(|e| format!("网络错误: {e}"))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("服务器返回 {status}: {}", body.trim()));
    }
    let v = body.trim().to_string();
    if v.is_empty() {
        return Err("服务器返回空版本".into());
    }
    Ok(v)
}

#[tauri::command]
pub async fn check_app_update(app: AppHandle) -> Result<AppUpdateStatus, String> {
    let current = app.package_info().version.to_string();
    match fetch_latest().await {
        Ok(latest) => Ok(AppUpdateStatus {
            update_available: crate::host_actions::is_newer(&latest, &current),
            current,
            latest,
            error: String::new(),
        }),
        Err(error) => Ok(AppUpdateStatus {
            update_available: false,
            current,
            latest: String::new(),
            error,
        }),
    }
}

#[tauri::command]
pub async fn apply_app_update(app: AppHandle) -> Result<(), String> {
    let url = bundle_url()?;
    let filename = bundle_file()?;
    let dest = std::env::temp_dir().join(filename);

    let bytes = client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载失败: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    if bytes.is_empty() {
        return Err("下载到的文件为空".into());
    }
    std::fs::write(&dest, &bytes).map_err(|e| format!("写入临时文件失败: {e}"))?;

    launch_installer(&dest)?;
    app.exit(0);
    Ok(())
}

#[cfg(target_os = "windows")]
fn launch_installer(dest: &std::path::Path) -> Result<(), String> {
    std::process::Command::new(dest)
        .spawn()
        .map_err(|e| format!("启动安装程序失败: {e}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn launch_installer(dest: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    // When running as an AppImage, replace the running AppImage file in place
    // instead of launching the freshly downloaded copy from /tmp. This keeps
    // the existing launcher / app-center entry pointing at the same file, so
    // every update no longer creates a brand-new desktop entry.
    if let Ok(appimage) = std::env::var("APPIMAGE") {
        let target = std::path::PathBuf::from(&appimage);
        if target.is_file() {
            match replace_appimage(dest, &target) {
                Ok(()) => {
                    std::process::Command::new(&target)
                        .spawn()
                        .map_err(|e| format!("启动更新失败: {e}"))?;
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("原地更新 AppImage 失败 ({e})，改为从临时目录启动");
                }
            }
        }
    }

    // Fallback: run the downloaded AppImage from the temp directory.
    std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("设置可执行权限失败: {e}"))?;
    std::process::Command::new(dest)
        .spawn()
        .map_err(|e| format!("启动更新失败: {e}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn replace_appimage(src: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = target.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "AppImage 路径没有父目录")
    })?;
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("cangling-keeper");
    // Write the new file next to the target so the final rename is atomic on
    // the same filesystem, then swap it over the currently running AppImage.
    let tmp = dir.join(format!(".{name}.update"));
    std::fs::copy(src, &tmp)?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&tmp, target)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn launch_installer(dest: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(dest)
        .spawn()
        .map_err(|e| format!("启动更新失败: {e}"))?;
    Ok(())
}
