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
