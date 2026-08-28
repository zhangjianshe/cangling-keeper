use rusqlite::{Connection, params};
use std::path::PathBuf;

use crate::auth::Auth;
use crate::certificate::Certificate;
use crate::host::Host;
use crate::proxy::ProxySettings;
use crate::tunnel::Tunnel;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS hosts (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    hostname            TEXT NOT NULL,
    port                INTEGER NOT NULL DEFAULT 22,
    username            TEXT NOT NULL,
    auth_method         TEXT NOT NULL DEFAULT 'password',
    password            TEXT NOT NULL DEFAULT '',
    certificate_id      TEXT NOT NULL DEFAULT '',
    inject_remote_port  INTEGER NOT NULL DEFAULT 7890,
    catalog             TEXT NOT NULL DEFAULT '',
    remote_id           TEXT NOT NULL DEFAULT '',
    is_public           INTEGER NOT NULL DEFAULT 0,
    owned               INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS tunnels (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    local_port     INTEGER NOT NULL,
    remote_host    TEXT NOT NULL,
    remote_port    INTEGER NOT NULL,
    ssh_host       TEXT NOT NULL,
    ssh_port       INTEGER NOT NULL DEFAULT 22,
    username       TEXT NOT NULL,
    auth_method    TEXT NOT NULL DEFAULT 'password',
    password       TEXT NOT NULL DEFAULT '',
    certificate_id TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS certificates (
    id               TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    private_key_path TEXT NOT NULL,
    public_key       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS proxy_settings (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    mode            TEXT NOT NULL DEFAULT '',
    host            TEXT NOT NULL DEFAULT '127.0.0.1',
    port            INTEGER NOT NULL DEFAULT 7890,
    enabled         INTEGER NOT NULL DEFAULT 0,
    last_reachable  INTEGER NOT NULL DEFAULT 0,
    last_http       INTEGER NOT NULL DEFAULT 0,
    last_socks5     INTEGER NOT NULL DEFAULT 0,
    last_message    TEXT NOT NULL DEFAULT '',
    last_checked_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL DEFAULT ''
);
";

/// Single SQLite connection managing hosts, tunnels and certificates.
pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn load(path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        conn.execute("INSERT OR IGNORE INTO proxy_settings (id) VALUES (1)", [])?;
        Ok(Self { conn })
    }

    // ---- hosts -------------------------------------------------------------

    pub fn list_hosts(&self) -> Result<Vec<Host>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, hostname, port, username, auth_method, password, certificate_id,
                        inject_remote_port, catalog, remote_id, is_public, owned
                 FROM hosts ORDER BY name COLLATE NOCASE",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], host_from_row)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn get_host(&self, id: &str) -> Result<Host, String> {
        self.conn
            .query_row(
                "SELECT id, name, hostname, port, username, auth_method, password, certificate_id,
                        inject_remote_port, catalog, remote_id, is_public, owned
                 FROM hosts WHERE id = ?1",
                params![id],
                host_from_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => format!("Host not found: {id}"),
                other => other.to_string(),
            })
    }

    pub fn add_host(&self, host: &Host) -> Result<(), String> {
        let (method, password, certificate_id) = flatten_auth(&host.auth);
        self.conn
            .execute(
                "INSERT INTO hosts (id, name, hostname, port, username, auth_method, password, certificate_id,
                                    inject_remote_port, catalog, remote_id, is_public, owned)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    host.id,
                    host.name,
                    host.hostname,
                    host.port,
                    host.username,
                    method,
                    password,
                    certificate_id,
                    host.inject_remote_port_or_default() as i64,
                    host.catalog,
                    host.remote_id,
                    host.is_public as i64,
                    host.owned as i64
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_host(&self, host: &Host) -> Result<(), String> {
        let (method, password, certificate_id) = flatten_auth(&host.auth);
        let changed = self
            .conn
            .execute(
                "UPDATE hosts SET name=?1, hostname=?2, port=?3, username=?4,
                        auth_method=?5, password=?6, certificate_id=?7,
                        inject_remote_port=?8, catalog=?9, remote_id=?10, is_public=?11, owned=?12
                 WHERE id=?13",
                params![
                    host.name,
                    host.hostname,
                    host.port,
                    host.username,
                    method,
                    password,
                    certificate_id,
                    host.inject_remote_port_or_default() as i64,
                    host.catalog,
                    host.remote_id,
                    host.is_public as i64,
                    host.owned as i64,
                    host.id
                ],
            )
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err(format!("Host not found: {}", host.id));
        }
        Ok(())
    }

    pub fn delete_host(&self, id: &str) -> Result<(), String> {
        let changed = self
            .conn
            .execute("DELETE FROM hosts WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err(format!("Host not found: {id}"));
        }
        Ok(())
    }

    // ---- tunnels -----------------------------------------------------------

    pub fn list_tunnels(&self) -> Result<Vec<Tunnel>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, local_port, remote_host, remote_port, ssh_host, ssh_port,
                        username, auth_method, password, certificate_id
                 FROM tunnels ORDER BY name COLLATE NOCASE",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], tunnel_from_row)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn get_tunnel(&self, id: &str) -> Result<Tunnel, String> {
        self.conn
            .query_row(
                "SELECT id, name, local_port, remote_host, remote_port, ssh_host, ssh_port,
                        username, auth_method, password, certificate_id
                 FROM tunnels WHERE id = ?1",
                params![id],
                tunnel_from_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => format!("Tunnel not found: {id}"),
                other => other.to_string(),
            })
    }

    pub fn add_tunnel(&self, tunnel: &Tunnel) -> Result<(), String> {
        let (method, password, certificate_id) = flatten_auth(&tunnel.auth);
        self.conn
            .execute(
                "INSERT INTO tunnels
                    (id, name, local_port, remote_host, remote_port, ssh_host, ssh_port,
                     username, auth_method, password, certificate_id)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    tunnel.id,
                    tunnel.name,
                    tunnel.local_port,
                    tunnel.remote_host,
                    tunnel.remote_port,
                    tunnel.ssh_host,
                    tunnel.ssh_port,
                    tunnel.username,
                    method,
                    password,
                    certificate_id
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_tunnel(&self, tunnel: &Tunnel) -> Result<(), String> {
        let (method, password, certificate_id) = flatten_auth(&tunnel.auth);
        let changed = self
            .conn
            .execute(
                "UPDATE tunnels SET name=?1, local_port=?2, remote_host=?3, remote_port=?4,
                        ssh_host=?5, ssh_port=?6, username=?7, auth_method=?8, password=?9,
                        certificate_id=?10
                 WHERE id=?11",
                params![
                    tunnel.name,
                    tunnel.local_port,
                    tunnel.remote_host,
                    tunnel.remote_port,
                    tunnel.ssh_host,
                    tunnel.ssh_port,
                    tunnel.username,
                    method,
                    password,
                    certificate_id,
                    tunnel.id
                ],
            )
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err(format!("Tunnel not found: {}", tunnel.id));
        }
        Ok(())
    }

    pub fn delete_tunnel(&self, id: &str) -> Result<(), String> {
        let changed = self
            .conn
            .execute("DELETE FROM tunnels WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err(format!("Tunnel not found: {id}"));
        }
        Ok(())
    }

    // ---- certificates ------------------------------------------------------

    pub fn list_certificates(&self) -> Result<Vec<Certificate>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, private_key_path, public_key
                 FROM certificates ORDER BY name COLLATE NOCASE",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], certificate_from_row)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn get_certificate(&self, id: &str) -> Result<Certificate, String> {
        self.conn
            .query_row(
                "SELECT id, name, private_key_path, public_key FROM certificates WHERE id = ?1",
                params![id],
                certificate_from_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => format!("Certificate not found: {id}"),
                other => other.to_string(),
            })
    }

    pub fn add_certificate(&self, cert: &Certificate) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO certificates (id, name, private_key_path, public_key)
                 VALUES (?1, ?2, ?3, ?4)",
                params![cert.id, cert.name, cert.private_key_path, cert.public_key],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_certificate(&self, id: &str) -> Result<(), String> {
        let hosts_in_use: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM hosts WHERE auth_method = 'certificate' AND certificate_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        let tunnels_in_use: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM tunnels WHERE auth_method = 'certificate' AND certificate_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        if hosts_in_use + tunnels_in_use > 0 {
            return Err("Certificate is in use by hosts/tunnels and cannot be deleted".into());
        }

        let changed = self
            .conn
            .execute("DELETE FROM certificates WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err(format!("Certificate not found: {id}"));
        }
        Ok(())
    }

    // ---- proxy -------------------------------------------------------------

    pub fn get_proxy_settings(&self) -> Result<ProxySettings, String> {
        match self.conn.query_row(
            "SELECT mode, host, port, enabled, last_reachable, last_http, last_socks5,
                    last_message, last_checked_at
             FROM proxy_settings WHERE id = 1",
            [],
            proxy_from_row,
        ) {
            Ok(settings) => Ok(settings),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(ProxySettings::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn save_proxy_settings(&self, settings: &ProxySettings) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO proxy_settings
                    (id, mode, host, port, enabled, last_reachable, last_http, last_socks5,
                     last_message, last_checked_at)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                    mode=excluded.mode,
                    host=excluded.host,
                    port=excluded.port,
                    enabled=excluded.enabled,
                    last_reachable=excluded.last_reachable,
                    last_http=excluded.last_http,
                    last_socks5=excluded.last_socks5,
                    last_message=excluded.last_message,
                    last_checked_at=excluded.last_checked_at",
                params![
                    settings.mode,
                    settings.host,
                    settings.port as i64,
                    settings.enabled as i64,
                    settings.last_reachable as i64,
                    settings.last_http as i64,
                    settings.last_socks5 as i64,
                    settings.last_message,
                    settings.last_checked_at
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ---- settings (key/value) ----------------------------------------------

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        match self.conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        ) {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM settings WHERE key = ?1", params![key])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Add columns introduced after the initial schema to existing databases.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    ensure_column(
        conn,
        "hosts",
        "auth_method",
        "ALTER TABLE hosts ADD COLUMN auth_method TEXT NOT NULL DEFAULT 'password'",
    )?;
    ensure_column(
        conn,
        "hosts",
        "certificate_id",
        "ALTER TABLE hosts ADD COLUMN certificate_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "tunnels",
        "auth_method",
        "ALTER TABLE tunnels ADD COLUMN auth_method TEXT NOT NULL DEFAULT 'password'",
    )?;
    ensure_column(
        conn,
        "tunnels",
        "certificate_id",
        "ALTER TABLE tunnels ADD COLUMN certificate_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "hosts",
        "inject_remote_port",
        "ALTER TABLE hosts ADD COLUMN inject_remote_port INTEGER NOT NULL DEFAULT 7890",
    )?;
    ensure_column(
        conn,
        "hosts",
        "catalog",
        "ALTER TABLE hosts ADD COLUMN catalog TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "hosts",
        "remote_id",
        "ALTER TABLE hosts ADD COLUMN remote_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "hosts",
        "is_public",
        "ALTER TABLE hosts ADD COLUMN is_public INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "hosts",
        "owned",
        "ALTER TABLE hosts ADD COLUMN owned INTEGER NOT NULL DEFAULT 0",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS proxy_settings (
            id              INTEGER PRIMARY KEY CHECK (id = 1),
            mode            TEXT NOT NULL DEFAULT '',
            host            TEXT NOT NULL DEFAULT '127.0.0.1',
            port            INTEGER NOT NULL DEFAULT 7890,
            enabled         INTEGER NOT NULL DEFAULT 0,
            last_reachable  INTEGER NOT NULL DEFAULT 0,
            last_http       INTEGER NOT NULL DEFAULT 0,
            last_socks5     INTEGER NOT NULL DEFAULT 0,
            last_message    TEXT NOT NULL DEFAULT '',
            last_checked_at INTEGER NOT NULL DEFAULT 0
        );",
    )?;
    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, column: &str, ddl: &str) -> rusqlite::Result<()> {
    let exists = {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut found = false;
        for name in rows {
            if name? == column {
                found = true;
                break;
            }
        }
        found
    };
    if !exists {
        conn.execute(ddl, [])?;
    }
    Ok(())
}

fn host_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Host> {
    let auth_method: String = row.get(5)?;
    let password: String = row.get(6)?;
    let certificate_id: String = row.get(7)?;

    let auth = match auth_method.as_str() {
        "certificate" => Auth::Certificate { certificate_id },
        _ => Auth::Password { password },
    };

    let inject_remote_port: i64 = row.get(8)?;
    let catalog: String = row.get(9)?;
    let remote_id: String = row.get(10)?;
    let is_public: i64 = row.get(11)?;
    let owned: i64 = row.get(12)?;
    Ok(Host {
        id: row.get(0)?,
        name: row.get(1)?,
        hostname: row.get(2)?,
        port: row.get(3)?,
        username: row.get(4)?,
        inject_remote_port: inject_remote_port.clamp(0, 65535) as u16,
        auth,
        catalog,
        remote_id,
        is_public: is_public != 0,
        owned: owned != 0,
    })
}

fn tunnel_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Tunnel> {
    let auth_method: String = row.get(8)?;
    let password: String = row.get(9)?;
    let certificate_id: String = row.get(10)?;

    let auth = match auth_method.as_str() {
        "certificate" => Auth::Certificate { certificate_id },
        _ => Auth::Password { password },
    };

    Ok(Tunnel {
        id: row.get(0)?,
        name: row.get(1)?,
        local_port: row.get(2)?,
        remote_host: row.get(3)?,
        remote_port: row.get(4)?,
        ssh_host: row.get(5)?,
        ssh_port: row.get(6)?,
        username: row.get(7)?,
        auth,
    })
}

fn certificate_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Certificate> {
    Ok(Certificate {
        id: row.get(0)?,
        name: row.get(1)?,
        private_key_path: row.get(2)?,
        public_key: row.get(3)?,
    })
}

fn proxy_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProxySettings> {
    let enabled: i64 = row.get(3)?;
    let last_reachable: i64 = row.get(4)?;
    let last_http: i64 = row.get(5)?;
    let last_socks5: i64 = row.get(6)?;
    let port: i64 = row.get(2)?;
    Ok(ProxySettings {
        mode: row.get(0)?,
        host: row.get(1)?,
        port: port.clamp(0, 65535) as u16,
        enabled: enabled != 0,
        last_reachable: last_reachable != 0,
        last_http: last_http != 0,
        last_socks5: last_socks5 != 0,
        last_message: row.get(7)?,
        last_checked_at: row.get(8)?,
    })
}

fn flatten_auth(auth: &Auth) -> (&'static str, &str, &str) {
    match auth {
        Auth::Password { password } => ("password", password.as_str(), ""),
        Auth::Certificate { certificate_id } => ("certificate", "", certificate_id.as_str()),
    }
}
