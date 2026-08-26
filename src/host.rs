use serde::{Deserialize, Serialize};

use crate::auth::Auth;

/// A single remote host definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Host {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub hostname: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    /// Remote listen port for `ssh -N -R <port>:<local-proxy>`. Default 7890.
    #[serde(default = "default_inject_remote_port")]
    pub inject_remote_port: u16,
    #[serde(default)]
    pub auth: Auth,
}

fn default_port() -> u16 {
    22
}

fn default_inject_remote_port() -> u16 {
    7890
}

impl Host {
    pub fn inject_remote_port_or_default(&self) -> u16 {
        if self.inject_remote_port == 0 {
            default_inject_remote_port()
        } else {
            self.inject_remote_port
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Name is required".into());
        }
        if self.hostname.trim().is_empty() {
            return Err("Hostname / IP is required".into());
        }
        if self.username.trim().is_empty() {
            return Err("Username is required".into());
        }
        if self.port == 0 {
            return Err("Port must be between 1 and 65535".into());
        }
        if self.inject_remote_port == 0 {
            return Err("Remote proxy port must be between 1 and 65535".into());
        }
        Ok(())
    }
}
