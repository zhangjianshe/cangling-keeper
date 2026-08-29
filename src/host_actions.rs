use serde::{Deserialize, Serialize};

const PROBE_SCRIPT: &str = include_str!("scripts/probe-cangling-update.sh");
const APPLY_SCRIPT: &str = include_str!("scripts/apply-cangling-update.sh");
const SET_ROLE_SCRIPT: &str = include_str!("scripts/set-cangling-role.sh");
const CHECK_SCRIPT: &str = include_str!("scripts/check-cangling-version.sh");
const CHECK_SSH_ENV_SCRIPT: &str = include_str!("scripts/check-ssh-env.sh");

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
    pub role: String,
    pub token_set: bool,
    pub cluster_token: String,
    pub master: String,
    /// Listen port of the remote `cangling-update` service. `0` if unknown.
    pub port: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplyResult {
    pub action: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_status: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshEnvCheck {
    pub status: String,
    pub changed: bool,
    pub allow_tcp_forwarding: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetRoleResult {
    pub role: String,
    pub active: bool,
    pub token_set: bool,
    pub master: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_status: i32,
}

pub fn normalize_role(role: &str) -> Result<&'static str, String> {
    match role.trim().to_ascii_lowercase().as_str() {
        "standalone" | "独立" | "独立模式" => Ok("standalone"),
        "master" => Ok("master"),
        "worker" => Ok("worker"),
        other => Err(format!(
            "未知运行模式 {other}，可选 standalone / master / worker"
        )),
    }
}

pub fn wrap_probe_command() -> String {
    bash_c(PROBE_SCRIPT, &[])
}

pub fn wrap_check_ssh_env_command() -> String {
    bash_c(CHECK_SSH_ENV_SCRIPT, &[])
}

pub fn wrap_apply_command(action: &str, arch: &str, proxy: &str) -> String {
    bash_c(APPLY_SCRIPT, &[action, arch, proxy])
}

pub fn wrap_set_role_command(role: &str, token: &str, master: &str) -> String {
    bash_c(SET_ROLE_SCRIPT, &[role, token, master])
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

/// cangling-update listens on 5400 unless `--port` / `CANGLING_PORT` is set.
pub const DEFAULT_CONSOLE_PORT: u16 = 5400;

pub fn console_remote_port(probe_port: u16) -> u16 {
    if probe_port > 0 {
        probe_port
    } else {
        DEFAULT_CONSOLE_PORT
    }
}

pub fn console_url(local_port: u16) -> String {
    format!("http://127.0.0.1:{local_port}/console")
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
    let mut role = String::new();
    let mut token_set = false;
    let mut cluster_token = String::new();
    let mut master = String::new();
    let mut port = 0u16;

    // `token=` is last on the marker line so the value may contain '|' or '='.
    let fields = if let Some(idx) = line.find("|token=") {
        cluster_token = line[idx + "|token=".len()..].to_string();
        &line[..idx]
    } else {
        line
    };

    for part in fields.split('|').skip(1) {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        match k {
            "installed" => installed = v == "1" || v.eq_ignore_ascii_case("true"),
            "arch" => arch = v.to_string(),
            "active" => active = v == "1" || v.eq_ignore_ascii_case("true"),
            "binary" => binary = v.to_string(),
            "version" => version = v.to_string(),
            "role" => role = v.to_string(),
            "token_set" => token_set = v == "1" || v.eq_ignore_ascii_case("true"),
            "master" => master = v.to_string(),
            "port" => {
                port = v
                    .parse()
                    .ok()
                    .filter(|n| (1..=65535).contains(n))
                    .unwrap_or(0);
            }
            _ => {}
        }
    }

    if arch.is_empty() {
        return Err(format!("probe missing arch: {line}"));
    }
    let supported = arch == "amd64" || arch == "arm64";
    if role.is_empty() {
        role = if installed {
            "standalone".into()
        } else {
            String::new()
        };
    }
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
        role,
        token_set: token_set || !cluster_token.is_empty(),
        cluster_token,
        master,
        port,
    })
}

pub fn parse_set_role(stdout: &str) -> Result<(String, bool, bool, String), String> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.starts_with("CK_ROLE|"))
        .ok_or_else(|| {
            let tail = stdout.trim();
            let tail = if tail.len() > 400 {
                &tail[tail.len() - 400..]
            } else {
                tail
            };
            format!("set-role produced no CK_ROLE line: {tail}")
        })?;

    let mut role = String::new();
    let mut active = false;
    let mut token_set = false;
    let mut master = String::new();

    for part in line.split('|').skip(1) {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        match k {
            "role" => role = v.to_string(),
            "active" => active = v == "1" || v.eq_ignore_ascii_case("true"),
            "token_set" => token_set = v == "1" || v.eq_ignore_ascii_case("true"),
            "master" => master = v.to_string(),
            _ => {}
        }
    }
    if role.is_empty() {
        return Err(format!("set-role missing role: {line}"));
    }
    Ok((role, active, token_set, master))
}

pub fn parse_check_ssh_env(stdout: &str) -> Result<SshEnvCheck, String> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.starts_with("CK_SSH_ENV|"))
        .ok_or_else(|| {
            let tail = stdout.trim();
            let tail = if tail.len() > 400 {
                &tail[tail.len() - 400..]
            } else {
                tail
            };
            format!("环境检查未返回 CK_SSH_ENV 行: {tail}")
        })?;

    let mut status = String::new();
    let mut changed = false;
    let mut allow_tcp_forwarding = String::new();
    let mut message = String::new();

    for part in line.split('|').skip(1) {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        match k {
            "status" => status = v.to_string(),
            "changed" => changed = v == "1" || v.eq_ignore_ascii_case("true"),
            "allow_tcp_forwarding" => allow_tcp_forwarding = v.to_string(),
            "message" => message = v.to_string(),
            _ => {}
        }
    }

    Ok(SshEnvCheck {
        status,
        changed,
        allow_tcp_forwarding,
        message,
    })
}

fn bash_c(script: &str, args: &[&str]) -> String {
    // The shell scripts are embedded with include_str!(). On Windows checkouts
    // git may rewrite them to CRLF, which makes the remote Linux shell choke on
    // `set -u\r` ("set: Illegal option -"). Normalize line endings so the
    // scripts run identically no matter how they were checked out.
    let script = script.replace('\r', "");
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
        let out = "noise\nCK_PROBE|installed=1|arch=arm64|active=1|binary=/usr/local/bin/cangling-update|version=v1.2.3|role=master|token_set=1|master=http://10.0.0.1:80|port=80|token=secret|tok\n";
        let p = parse_probe(out).unwrap();
        assert!(p.installed);
        assert!(p.supported);
        assert_eq!(p.arch, "arm64");
        assert!(p.active);
        assert_eq!(p.binary, "/usr/local/bin/cangling-update");
        assert_eq!(p.version, "v1.2.3");
        assert_eq!(p.role, "master");
        assert!(p.token_set);
        assert_eq!(p.cluster_token, "secret|tok");
        assert_eq!(p.master, "http://10.0.0.1:80");
        assert_eq!(p.port, 80);
    }

    #[test]
    fn probe_port_defaults_when_missing() {
        let p =
            parse_probe("CK_PROBE|installed=1|arch=amd64|active=1|binary=|version=v1\n").unwrap();
        assert_eq!(p.port, 0);
        assert_eq!(console_remote_port(p.port), DEFAULT_CONSOLE_PORT);
    }

    #[test]
    fn console_url_goes_to_cangling_update_console() {
        assert_eq!(console_url(15400), "http://127.0.0.1:15400/console");
        assert_eq!(console_remote_port(5400), 5400);
        assert_eq!(console_remote_port(0), 5400);
        assert_eq!(console_remote_port(80), 80);
    }

    #[test]
    fn probe_ignores_invalid_port() {
        let p = parse_probe(
            "CK_PROBE|installed=1|arch=amd64|active=1|binary=|version=v1|port=not-a-port\n",
        )
        .unwrap();
        assert_eq!(p.port, 0);
    }

    #[test]
    fn probe_defaults_role_when_missing() {
        let p = parse_probe("CK_PROBE|installed=1|arch=amd64|active=0|binary=|version=\n").unwrap();
        assert_eq!(p.role, "standalone");
        assert!(!p.token_set);
        assert!(p.cluster_token.is_empty());
        assert!(p.master.is_empty());
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
        assert!(probe.contains("|port=%s|token=%s"));
        assert_eq!(inject_proxy_url(1080), "http://127.0.0.1:1080");
    }

    #[test]
    fn strips_carriage_returns_from_embedded_script() {
        let crlf = "#!/bin/bash\r\nset -eu\r\necho hi\r\n";
        let cmd = bash_c(crlf, &[]);
        assert!(!cmd.contains('\r'));
        assert!(cmd.contains("set -eu"));
        assert!(cmd.contains("echo hi"));
    }

    #[test]
    fn wrap_set_role_includes_args() {
        let cmd = wrap_set_role_command("worker", "tok en", "http://10.0.0.1:80");
        assert!(cmd.contains("CK_ROLE"));
        assert!(cmd.contains("standalone|master|worker"));
        assert!(cmd.ends_with("ck worker 'tok en' http://10.0.0.1:80"));
    }

    #[test]
    fn parses_set_role_line() {
        let (role, active, token_set, master) = parse_set_role(
            "noise\nCK_ROLE|role=worker|active=1|token_set=1|master=http://10.0.0.2:80\n",
        )
        .unwrap();
        assert_eq!(role, "worker");
        assert!(active);
        assert!(token_set);
        assert_eq!(master, "http://10.0.0.2:80");
    }

    #[test]
    fn normalizes_role_aliases() {
        assert_eq!(normalize_role("独立模式").unwrap(), "standalone");
        assert_eq!(normalize_role("Master").unwrap(), "master");
        assert!(normalize_role("foo").is_err());
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
