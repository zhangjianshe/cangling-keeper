use serde::{Deserialize, Serialize};

const PROBE_SCRIPT: &str = include_str!("scripts/probe-cangling-update.sh");
const APPLY_SCRIPT: &str = include_str!("scripts/apply-cangling-update.sh");
const CHECK_SCRIPT: &str = include_str!("scripts/check-cangling-version.sh");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProbe {
    pub installed: bool,
    pub arch: String,
    pub supported: bool,
    pub active: bool,
    pub binary: String,
    pub version: String,
    pub latest: String,
    pub update_available: bool,
    pub version_error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplyResult {
    pub action: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_status: i32,
}

pub fn wrap_probe_command() -> String {
    bash_c(PROBE_SCRIPT, &[])
}

pub fn wrap_apply_command(action: &str, arch: &str, proxy: &str) -> String {
    bash_c(APPLY_SCRIPT, &[action, arch, proxy])
}

pub fn wrap_check_command(proxy: &str) -> String {
    bash_c(CHECK_SCRIPT, &[proxy])
}

pub fn parse_latest_version(stdout: &str) -> Result<String, String> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.starts_with("CK_VERSION|"))
        .ok_or_else(|| {
            let tail = stdout.trim();
            let tail = if tail.len() > 400 {
                &tail[tail.len() - 400..]
            } else {
                tail
            };
            format!("version check produced no CK_VERSION line: {tail}")
        })?;

    for part in line.split('|').skip(1) {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        if k == "latest" {
            let latest = v.trim();
            if latest.is_empty() {
                return Err("version check returned an empty latest version".into());
            }
            return Ok(latest.to_string());
        }
    }
    Err(format!("version check missing latest field: {line}"))
}

/// Compare two version strings such as "v0.1.52" / "0.1.51" / "1.2.3-beta".
/// Leading "v" is ignored; numeric components are compared left to right and a
/// release (no prerelease suffix) is newer than a prerelease with equal numbers.
pub fn is_newer(latest: &str, installed: &str) -> bool {
    let (l_nums, l_pre) = parse_version(latest);
    let (i_nums, i_pre) = parse_version(installed);

    let len = l_nums.len().max(i_nums.len());
    for i in 0..len {
        let l = l_nums.get(i).copied().unwrap_or(0);
        let r = i_nums.get(i).copied().unwrap_or(0);
        if l != r {
            return l > r;
        }
    }

    // Numeric parts are equal: a released version beats a prerelease.
    if l_pre.is_empty() && !i_pre.is_empty() {
        return true;
    }
    if !l_pre.is_empty() && i_pre.is_empty() {
        return false;
    }
    l_pre > i_pre
}

fn parse_version(s: &str) -> (Vec<u64>, String) {
    let t = s.trim();
    let t = t
        .strip_prefix('v')
        .or_else(|| t.strip_prefix('V'))
        .unwrap_or(t);
    let (core, pre) = match t.split_once('-') {
        Some((c, p)) => (c, p),
        None => (t, ""),
    };
    let nums = core
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect();
    (nums, pre.to_string())
}

pub fn inject_proxy_url(remote_port: u16) -> String {
    format!("http://127.0.0.1:{remote_port}")
}

pub fn parse_probe(stdout: &str) -> Result<UpdateProbe, String> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.starts_with("CK_PROBE|"))
        .ok_or_else(|| {
            let tail = stdout.trim();
            let tail = if tail.len() > 400 {
                &tail[tail.len() - 400..]
            } else {
                tail
            };
            format!("probe script produced no CK_PROBE line: {tail}")
        })?;

    let mut installed = false;
    let mut arch = String::new();
    let mut active = false;
    let mut binary = String::new();
    let mut version = String::new();

    for part in line.split('|').skip(1) {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        match k {
            "installed" => installed = v == "1" || v.eq_ignore_ascii_case("true"),
            "arch" => arch = v.to_string(),
            "active" => active = v == "1" || v.eq_ignore_ascii_case("true"),
            "binary" => binary = v.to_string(),
            "version" => version = v.to_string(),
            _ => {}
        }
    }

    if arch.is_empty() {
        return Err(format!("probe missing arch: {line}"));
    }
    let supported = arch == "amd64" || arch == "arm64";
    Ok(UpdateProbe {
        installed,
        arch,
        supported,
        active,
        binary,
        version,
        latest: String::new(),
        update_available: false,
        version_error: String::new(),
    })
}

fn bash_c(script: &str, args: &[&str]) -> String {
    let escaped = script.replace('\'', "'\"'\"'");
    let mut cmd = format!("/bin/bash -c '{escaped}' ck");
    for arg in args {
        cmd.push(' ');
        cmd.push_str(&shell_quote(arg));
    }
    cmd
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b":/.-_=+".contains(&b))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_probe_line() {
        let out = "noise\nCK_PROBE|installed=1|arch=arm64|active=1|binary=/usr/local/bin/cangling-update|version=v1.2.3\n";
        let p = parse_probe(out).unwrap();
        assert!(p.installed);
        assert!(p.supported);
        assert_eq!(p.arch, "arm64");
        assert!(p.active);
        assert_eq!(p.binary, "/usr/local/bin/cangling-update");
        assert_eq!(p.version, "v1.2.3");
    }

    #[test]
    fn unsupported_arch() {
        let p = parse_probe("CK_PROBE|installed=0|arch=unsupported|active=0|binary=|version=\n")
            .unwrap();
        assert!(!p.supported);
        assert!(!p.installed);
    }

    #[test]
    fn wrap_includes_urls_and_args() {
        let cmd = wrap_apply_command("install", "amd64", "http://127.0.0.1:7890");
        assert!(cmd.contains(
            "https://soft.cangling.cn:22002/software/a59ff5999a0d4404a257cf7aa16ca10b/latest"
        ));
        assert!(cmd.contains("cangling-update-linux-amd64"));
        assert!(cmd.contains("cangling-update-linux-arm64"));
        assert!(!cmd.contains("/upload/"));
        assert!(cmd.contains("install-service"));
        assert!(cmd.ends_with("ck install amd64 http://127.0.0.1:7890"));
        let probe = wrap_probe_command();
        assert!(probe.contains("CK_PROBE"));
        assert_eq!(inject_proxy_url(1080), "http://127.0.0.1:1080");
    }

    #[test]
    fn wraps_check_command() {
        let cmd = wrap_check_command("http://127.0.0.1:7890");
        assert!(cmd.contains(
            "https://soft.cangling.cn:22002/software/a59ff5999a0d4404a257cf7aa16ca10b/latest/version.txt"
        ));
        assert!(cmd.contains("CK_VERSION"));
        assert!(cmd.ends_with("ck http://127.0.0.1:7890"));
    }

    #[test]
    fn parses_latest_version() {
        let out = "noise\nCK_VERSION|latest=v0.1.52\n";
        assert_eq!(parse_latest_version(out).unwrap(), "v0.1.52");
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.1.52", "v0.1.51"));
        assert!(is_newer("0.1.52", "0.1.51"));
        assert!(is_newer("v1.0.0", "v0.9.99"));
        assert!(is_newer("v0.2", "v0.1.9"));
        assert!(is_newer("v0.1.52", "v0.1.52-beta"));
        assert!(!is_newer("v0.1.51", "v0.1.52"));
        assert!(!is_newer("v0.1.52", "v0.1.52"));
        assert!(!is_newer("v0.1.52-beta", "v0.1.52"));
    }
}
