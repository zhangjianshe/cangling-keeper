//! Installs only the embedded public Cangling CA, after explicit UI confirmation
//! and operating-system authorization. No CA/signing private keys are embedded.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

const DER: &[u8] = include_bytes!("../docs/certificates/cangling-root-ca.cer");
const FINGERPRINT: &str = "0285F51BB6BEA21C74AD8C13320FFFB3C940438C54A4D8384A3FF2E78E6EBCD1";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    supported: bool,
    installed: bool,
    fingerprint: &'static str,
    message: String,
}

pub(crate) fn status() -> Result<Status, String> {
    if format!("{:X}", Sha256::digest(DER)) != FINGERPRINT {
        return Err("内置根证书指纹不匹配".into());
    }
    let (supported, installed, message) = platform_status()?;
    Ok(Status {
        supported,
        installed,
        fingerprint: FINGERPRINT,
        message,
    })
}

pub(crate) fn install() -> Result<Status, String> {
    let current = status()?;
    if current.installed {
        return Ok(current);
    }
    if !current.supported {
        return Err(current.message);
    }
    platform_install()?;
    let result = status()?;
    if !result.installed {
        return Err("安装操作已结束，但未检测到目标根证书，请检查系统授权或信任库更新结果".into());
    }
    Ok(result)
}

fn pem() -> String {
    let encoded = STANDARD.encode(DER);
    format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        encoded
            .as_bytes()
            .chunks(64)
            .map(|s| std::str::from_utf8(s).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn run(mut command: Command) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|e| format!("无法启动证书安装程序：{e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    let detail: String = detail.chars().take(1500).collect();
    Err(format!(
        "证书安装失败或授权已取消（退出码 {}）：{}",
        output.status.code().unwrap_or(-1),
        detail.trim()
    ))
}

#[cfg(windows)]
fn powershell() -> Command {
    let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    crate::host_cmd::command(
        PathBuf::from(windows).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    )
}

#[cfg(windows)]
fn platform_status() -> Result<(bool, bool, String), String> {
    let script = format!(
        "$ErrorActionPreference='Stop'; $s=New-Object -TypeName System.Security.Cryptography.X509Certificates.X509Store -ArgumentList @('Root','LocalMachine');try{{$s.Open('ReadOnly');$found=@($s.Certificates | Where-Object {{[Convert]::ToBase64String($_.RawData) -eq '{}'}});if($found.Count -gt 0){{'installed'}}else{{'missing'}}}}finally{{$s.Close()}}",
        STANDARD.encode(DER)
    );
    let output = powershell()
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| format!("无法检查 Windows 证书库：{e}"))?;
    if !output.status.success() {
        return Err("无法读取 Windows 本地计算机根证书库".into());
    }
    let installed = String::from_utf8_lossy(&output.stdout).trim() == "installed";
    Ok((
        true,
        installed,
        "安装到此 Keeper 服务所在电脑的受信任根证书库（所有用户），需要管理员授权。".into(),
    ))
}

#[cfg(windows)]
fn platform_install() -> Result<(), String> {
    // Pass the immutable certificate in the elevated command, never a path to
    // a user-writable file that could be swapped while the UAC dialog is open.
    let inner = format!(
        "$ErrorActionPreference='Stop';try{{$bytes=[Convert]::FromBase64String('{}');$c=New-Object -TypeName System.Security.Cryptography.X509Certificates.X509Certificate2 -ArgumentList (,$bytes);$s=New-Object -TypeName System.Security.Cryptography.X509Certificates.X509Store -ArgumentList @('Root','LocalMachine');try{{$s.Open('ReadWrite');$s.Add($c)}}finally{{$s.Close()}};exit 0}}catch{{exit 1}}",
        STANDARD.encode(DER)
    );
    let encoded = STANDARD.encode(
        inner
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let outer = format!(
        "$ErrorActionPreference='Stop';try{{$p=Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -Verb RunAs -ArgumentList @('-NoProfile','-NonInteractive','-EncodedCommand','{encoded}') -Wait -PassThru;exit $p.ExitCode}}catch{{[Console]::Error.WriteLine('管理员授权已取消或证书安装程序无法启动');exit 1}}"
    );
    let mut command = powershell();
    command.args(["-NoProfile", "-NonInteractive", "-Command", &outer]);
    run(command)
}

#[derive(Clone)]
#[cfg(any(target_os = "linux", test))]
struct LinuxPlan {
    anchor: PathBuf,
    updater: PathBuf,
    extract: bool,
}

#[cfg(any(target_os = "linux", test))]
fn linux_plan_at(root: &Path) -> Option<LinuxPlan> {
    // Root rotations must not overwrite or remove another trusted certificate.
    let filename = format!("cangling-root-ca-{}.crt", FINGERPRINT.to_ascii_lowercase());
    for updater in [
        "usr/sbin/update-ca-certificates",
        "usr/bin/update-ca-certificates",
    ] {
        if root.join(updater).is_file() {
            return Some(LinuxPlan {
                anchor: root.join("usr/local/share/ca-certificates").join(&filename),
                updater: root.join(updater),
                extract: false,
            });
        }
    }
    for updater in ["usr/bin/update-ca-trust", "usr/sbin/update-ca-trust"] {
        if root.join(updater).is_file() {
            return Some(LinuxPlan {
                anchor: root.join("etc/pki/ca-trust/source/anchors").join(&filename),
                updater: root.join(updater),
                extract: true,
            });
        }
    }
    None
}

#[cfg(any(target_os = "linux", test))]
fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

#[cfg(any(target_os = "linux", test))]
fn linux_script(plan: &LinuxPlan) -> String {
    let updater = format!(
        "{}{}",
        quote(&plan.updater),
        if plan.extract { " extract" } else { "" }
    );
    format!(
        r#"set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 022
target={target}
dir={dir}
mkdir -p -- "$dir"
tmp=$(mktemp "$dir/.cangling-root-XXXXXX")
trap 'rm -f -- "$tmp"' EXIT HUP INT TERM
cat > "$tmp" <<'CANGLING_ROOT_CERT'
{pem}CANGLING_ROOT_CERT
created=0
if [ -L "$target" ]; then echo '拒绝覆盖符号链接证书路径' >&2; exit 12; fi
if [ -e "$target" ]; then
  if ! cmp -s "$tmp" "$target"; then echo '目标路径存在其他证书，未覆盖' >&2; exit 12; fi
else
  chmod 644 "$tmp"
  mv -- "$tmp" "$target"
  created=1
fi
if ! {updater}; then
  if [ "$created" = 1 ]; then rm -f -- "$target"; {updater} >/dev/null 2>&1 || true; fi
  echo '更新系统证书信任库失败' >&2
  exit 13
fi
"#,
        target = quote(&plan.anchor),
        dir = quote(plan.anchor.parent().unwrap()),
        pem = pem()
    )
}

#[cfg(target_os = "linux")]
fn platform_status() -> Result<(bool, bool, String), String> {
    let Some(plan) = linux_plan_at(Path::new("/")) else {
        return Ok((false, false, "未找到 update-ca-certificates 或 update-ca-trust；请先安装 ca-certificates 软件包。支持 Ubuntu、麒麟的系统证书库。".into()));
    };
    let anchor_installed = std::fs::read_to_string(&plan.anchor)
        .ok()
        .is_some_and(|s| s == pem());
    let trusted = [
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
        "/etc/pki/tls/certs/ca-bundle.crt",
    ]
    .iter()
    .any(|path| {
        std::fs::read_to_string(path)
            .ok()
            .is_some_and(|bundle| bundle.contains(pem().trim()))
    });
    let installed = anchor_installed && trusted;
    Ok((
        true,
        installed,
        "安装到此 Keeper 服务所在电脑的系统证书信任库，需要系统管理员授权。".into(),
    ))
}

#[cfg(target_os = "linux")]
fn platform_install() -> Result<(), String> {
    let plan = linux_plan_at(Path::new("/")).ok_or("系统未安装 ca-certificates，无法更新信任库")?;
    let script = linux_script(&plan);
    let root = crate::host_cmd::command("/usr/bin/id")
        .arg("-u")
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "0")
        .unwrap_or(false);
    let mut command = if root {
        crate::host_cmd::command("/bin/sh")
    } else {
        let pkexec = ["/usr/bin/pkexec", "/bin/pkexec"].into_iter().find(|p| Path::new(p).is_file()).ok_or("缺少 pkexec，无法显示系统授权框。请安装 polkit/policykit-1，或由管理员导入根证书。")?;
        let mut command = crate::host_cmd::command(pkexec);
        command.arg("/bin/sh");
        command
    };
    command.args(["-c", &script]);
    run(command)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn platform_status() -> Result<(bool, bool, String), String> {
    Ok((
        false,
        false,
        "当前仅支持 Windows、Ubuntu 和麒麟系统。".into(),
    ))
}
#[cfg(not(any(windows, target_os = "linux")))]
fn platform_install() -> Result<(), String> {
    Err("此系统暂不支持自动安装根证书".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_approved_public_root_is_embedded() {
        assert_eq!(format!("{:X}", Sha256::digest(DER)), FINGERPRINT);
        assert!(!pem().contains("PRIVATE KEY"));
    }
    #[test]
    fn linux_uses_native_trust_tool_for_both_distribution_families() {
        let root = std::env::temp_dir().join(format!("keeper-trust-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("usr/sbin")).unwrap();
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        std::fs::write(root.join("usr/sbin/update-ca-certificates"), b"").unwrap();
        assert!(!linux_plan_at(&root).unwrap().extract);
        std::fs::remove_file(root.join("usr/sbin/update-ca-certificates")).unwrap();
        std::fs::write(root.join("usr/bin/update-ca-trust"), b"").unwrap();
        assert!(linux_plan_at(&root).unwrap().extract);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn root_rotation_installs_alongside_legacy_anchor() {
        use std::os::unix::fs::PermissionsExt;
        for (updater, directory) in [
            (
                "usr/sbin/update-ca-certificates",
                "usr/local/share/ca-certificates",
            ),
            ("usr/bin/update-ca-trust", "etc/pki/ca-trust/source/anchors"),
        ] {
            let root =
                std::env::temp_dir().join(format!("keeper-rotate-ca-{}", uuid::Uuid::new_v4()));
            let tool = root.join(updater);
            std::fs::create_dir_all(tool.parent().unwrap()).unwrap();
            std::fs::write(&tool, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
            let legacy = root.join(directory).join("cangling-root-ca.crt");
            std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
            std::fs::write(&legacy, "previous trusted CA").unwrap();
            let plan = linux_plan_at(&root).unwrap();
            assert_ne!(plan.anchor, legacy);
            assert!(
                plan.anchor
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(&FINGERPRINT.to_ascii_lowercase())
            );
            let execute = || {
                Command::new("/bin/sh")
                    .args(["-c", &linux_script(&plan)])
                    .output()
                    .unwrap()
            };
            assert!(execute().status.success());
            assert!(execute().status.success());
            assert_eq!(
                std::fs::read_to_string(&legacy).unwrap(),
                "previous trusted CA"
            );
            assert_eq!(std::fs::read_to_string(&plan.anchor).unwrap(), pem());
            std::fs::remove_file(&plan.anchor).unwrap();
            std::fs::write(&tool, "#!/bin/sh\nexit 1\n").unwrap();
            assert!(!execute().status.success());
            assert!(!plan.anchor.exists());
            assert_eq!(
                std::fs::read_to_string(&legacy).unwrap(),
                "previous trusted CA"
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn install_script_preserves_existing_certificates_and_rolls_back_failed_updates() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("keeper trust '{}'", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let tool = root.join("updater");
        std::fs::write(&tool, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plan = LinuxPlan {
            anchor: root.join("anchors/root.crt"),
            updater: tool.clone(),
            extract: false,
        };
        let execute = || {
            Command::new("/bin/sh")
                .args(["-c", &linux_script(&plan)])
                .output()
                .unwrap()
        };
        assert!(execute().status.success());
        assert_eq!(std::fs::read_to_string(&plan.anchor).unwrap(), pem());
        assert!(execute().status.success());
        std::fs::write(&plan.anchor, "other certificate").unwrap();
        assert!(!execute().status.success());
        assert_eq!(
            std::fs::read_to_string(&plan.anchor).unwrap(),
            "other certificate"
        );
        std::fs::remove_file(&plan.anchor).unwrap();
        std::fs::write(&tool, "#!/bin/sh\nexit 1\n").unwrap();
        assert!(!execute().status.success());
        assert!(!plan.anchor.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
