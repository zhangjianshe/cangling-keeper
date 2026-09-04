use serde::{Deserialize, Serialize};

use crate::auth::Auth;

/// An SSH tunnel parsed from `ssh -N -L ...` or `ssh -N -R ...`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tunnel {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default = "default_direction")]
    pub direction: String,
    #[serde(default = "default_local_host")]
    pub local_host: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub ssh_host: String,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    pub username: String,
    #[serde(default)]
    pub auth: Auth,
}

fn default_ssh_port() -> u16 {
    22
}

fn default_direction() -> String {
    "local".into()
}

fn default_local_host() -> String {
    "127.0.0.1".into()
}

impl Tunnel {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Name is required".into());
        }
        if !matches!(self.direction.as_str(), "local" | "remote") {
            return Err("Tunnel direction must be local or remote".into());
        }
        if self.local_host.trim().is_empty() {
            return Err("Local host / IP is required".into());
        }
        if self.local_port == 0 {
            return Err("Local port must be between 1 and 65535".into());
        }
        if self.remote_host.trim().is_empty() {
            return Err("Remote host is required".into());
        }
        if self.remote_port == 0 {
            return Err("Remote port must be between 1 and 65535".into());
        }
        if self.ssh_host.trim().is_empty() {
            return Err("SSH host is required".into());
        }
        if self.ssh_port == 0 {
            return Err("SSH port must be between 1 and 65535".into());
        }
        if self.username.trim().is_empty() {
            return Err("Username is required".into());
        }
        Ok(())
    }
}

/// A tunnel plus its live connection state (used in list responses).
#[derive(Debug, Clone, Serialize)]
pub struct TunnelInfo {
    #[serde(flatten)]
    pub tunnel: Tunnel,
    pub active: bool,
}

/// Parse an SSH command such as:
///   `ssh -N -L 12345:10.1.1.1:22 root@123.12.3.1`
///   `ssh -p 22001 -N -R 12345:localhost:12345 root@lc.cangling.cn`
///   `ssh -N -L 12345:10.1.1.1:22 -p 2222 -i ~/.ssh/id_ed25519 root@123.12.3.1`
///
/// Returns a `Tunnel` with empty id/name that the caller can fill in.
pub fn parse_ssh_command(cmd: &str) -> Result<Tunnel, String> {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    if tokens.is_empty() {
        return Err("Command is empty".into());
    }

    // Optional bind address, listening port, target host, target port.
    let mut forward: Option<(Option<String>, u16, String, u16)> = None;
    let mut direction = "local".to_string();
    let mut ssh_port: u16 = 22;
    let mut destination: Option<String> = None;

    let mut i = if tokens[0] == "ssh" { 1 } else { 0 };

    while i < tokens.len() {
        let token = tokens[i];
        if token == "-L" {
            i += 1;
            if i >= tokens.len() {
                return Err("Missing value after -L".into());
            }
            forward = Some(parse_forward(tokens[i], "-L")?);
        } else if token.starts_with("-L") && token.len() > 2 {
            let spec = token[2..].trim_start_matches('=').to_string();
            forward = Some(parse_forward(&spec, "-L")?);
        } else if token == "-R" {
            direction = "remote".into();
            i += 1;
            if i >= tokens.len() {
                return Err("Missing value after -R".into());
            }
            // The form stores a port, target host, and target port.  Those
            // fields map directly to a reverse-forward specification too:
            // remote-listen-port:local-host:local-port.
            forward = Some(parse_forward(tokens[i], "-R")?);
        } else if token.starts_with("-R") && token.len() > 2 {
            direction = "remote".into();
            let spec = token[2..].trim_start_matches('=').to_string();
            forward = Some(parse_forward(&spec, "-R")?);
        } else if token == "-p" {
            i += 1;
            if i >= tokens.len() {
                return Err("Missing value after -p".into());
            }
            ssh_port = tokens[i]
                .parse::<u16>()
                .map_err(|_| format!("Invalid SSH port: {}", tokens[i]))?;
        } else if token == "-i" {
            // Certificate auth is managed via the certificate panel; the key
            // path from `-i` is accepted but ignored here (select a certificate
            // in the form instead).
            i += 1;
            if i >= tokens.len() {
                return Err("Missing value after -i".into());
            }
        } else if token.starts_with('-') {
            // ignore other flags (-N, -f, -n, -o ...)
        } else {
            destination = Some(token.to_string());
        }
        i += 1;
    }

    let (bind_host, local_port, target_host, remote_port) =
        forward.ok_or("No SSH forward found (expected -L or -R port:host:port)")?;

    let destination = destination.ok_or("No SSH destination found (expected user@host)")?;
    let (username, ssh_host, dest_port) = parse_destination(&destination);
    if dest_port != 22 {
        ssh_port = dest_port;
    }
    let (local_host, remote_host) = if direction == "remote" {
        // -R [bind_address:]port:host:hostport: remote_host is the address
        // listened on by the SSH server, while local_host is the local target.
        (
            target_host,
            bind_host.unwrap_or_else(|| "localhost".to_string()),
        )
    } else {
        // -L [bind_address:]port:host:hostport.
        (bind_host.unwrap_or_else(default_local_host), target_host)
    };

    Ok(Tunnel {
        id: String::new(),
        name: String::new(),
        direction,
        local_host,
        local_port,
        remote_host,
        remote_port,
        ssh_host,
        ssh_port,
        username,
        auth: Auth::default(),
    })
}

fn parse_forward(spec: &str, kind: &str) -> Result<(Option<String>, u16, String, u16), String> {
    let parts: Vec<&str> = spec.split(':').collect();
    let (bind_host, local, remote_host, remote_port) = match parts.len() {
        3 => (None, parts[0], parts[1], parts[2]),
        4 => (Some(parts[0].to_string()), parts[1], parts[2], parts[3]),
        _ => {
            return Err(format!(
                "Invalid {kind} spec '{spec}' (expected port:host:port)"
            ));
        }
    };
    let local_port = local
        .parse::<u16>()
        .map_err(|_| format!("Invalid local port: {local}"))?;
    let remote_port = remote_port
        .parse::<u16>()
        .map_err(|_| format!("Invalid remote port: {remote_port}"))?;
    Ok((bind_host, local_port, remote_host.to_string(), remote_port))
}

fn parse_destination(dest: &str) -> (String, String, u16) {
    let (user, hostport) = match dest.split_once('@') {
        Some((u, h)) => (u.to_string(), h.to_string()),
        None => (String::new(), dest.to_string()),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(22)),
        None => (hostport, 22),
    };
    (user, host, port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_forward() {
        let t = parse_ssh_command("ssh -N -L 12345:10.1.1.1:22 root@123.12.3.1").unwrap();
        assert_eq!(t.local_port, 12345);
        assert_eq!(t.local_host, "127.0.0.1");
        assert_eq!(t.remote_host, "10.1.1.1");
        assert_eq!(t.remote_port, 22);
        assert_eq!(t.ssh_host, "123.12.3.1");
        assert_eq!(t.ssh_port, 22);
        assert_eq!(t.username, "root");
        assert_eq!(t.direction, "local");
        assert!(matches!(t.auth, Auth::Password { .. }));
    }

    #[test]
    fn parses_with_port_and_key() {
        let t = parse_ssh_command(
            "ssh -N -L 8080:db:3306 -p 2222 -i ~/.ssh/id_ed25519 admin@db.example.com",
        )
        .unwrap();
        assert_eq!(t.local_port, 8080);
        assert_eq!(t.remote_host, "db");
        assert_eq!(t.remote_port, 3306);
        assert_eq!(t.ssh_host, "db.example.com");
        assert_eq!(t.ssh_port, 2222);
        assert_eq!(t.username, "admin");
        assert!(matches!(t.auth, Auth::Password { .. }));
    }

    #[test]
    fn parses_reverse_forward_with_ssh_port() {
        let t = parse_ssh_command("ssh -p 22001 -N -R 12345:localhost:12345 root@lc.cangling.cn")
            .unwrap();
        assert_eq!(t.local_port, 12345);
        assert_eq!(t.local_host, "localhost");
        assert_eq!(t.remote_host, "localhost");
        assert_eq!(t.remote_port, 12345);
        assert_eq!(t.ssh_host, "lc.cangling.cn");
        assert_eq!(t.ssh_port, 22001);
        assert_eq!(t.username, "root");
        assert_eq!(t.direction, "remote");
    }

    #[test]
    fn parses_reverse_forward_with_remote_bind_address() {
        let t = parse_ssh_command(
            "ssh -p 22001 -N -R 0.0.0.0:12345:localhost:12345 root@lc.cangling.cn",
        )
        .unwrap();
        assert_eq!(t.direction, "remote");
        assert_eq!(t.remote_host, "0.0.0.0");
        assert_eq!(t.local_port, 12345);
        assert_eq!(t.local_host, "localhost");
        assert_eq!(t.remote_port, 12345);
    }

    #[test]
    fn parses_local_forward_with_bind_address() {
        let t = parse_ssh_command("ssh -N -L 0.0.0.0:8080:db:3306 root@gateway").unwrap();
        assert_eq!(t.direction, "local");
        assert_eq!(t.local_host, "0.0.0.0");
        assert_eq!(t.local_port, 8080);
        assert_eq!(t.remote_host, "db");
        assert_eq!(t.remote_port, 3306);
    }
}
