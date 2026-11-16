//! Bounded, per-user diagnostics. Never log credentials, signatures or TCP payloads.
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const MAX_BYTES: u64 = 5 * 1024 * 1024;
const BACKUPS: usize = 3;
static LOGGER: OnceLock<Mutex<LogFile>> = OnceLock::new();
struct LogFile {
    path: PathBuf,
    file: Option<File>,
}

pub fn init(data_dir: &Path) -> Result<(), String> {
    let directory = data_dir.join("logs");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let path = directory.join("cangling-keeper.log");
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let _ = LOGGER.set(Mutex::new(LogFile {
        path,
        file: Some(file),
    }));
    std::panic::set_hook(Box::new(|info| error(&format!("程序发生异常：{info}"))));
    info("服务启动");
    Ok(())
}
pub fn info(message: &str) {
    write("INFO", message);
}
pub fn error(message: &str) {
    write("ERROR", message);
}
fn write(level: &str, message: &str) {
    eprintln!("[{level}] {message}");
    let Some(logger) = LOGGER.get() else { return };
    let Ok(mut log) = logger.lock() else { return };
    let line = format!(
        "{} [{level}] {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        message.replace(['\r', '\n'], " ")
    );
    if let Err(error) = log.append(&line) {
        eprintln!("写入日志失败：{error}");
    }
}
impl LogFile {
    fn append(&mut self, line: &str) -> std::io::Result<()> {
        if self
            .file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map(|m| m.len())
            .unwrap_or(0)
            + line.len() as u64
            > MAX_BYTES
        {
            self.file.take(); // Windows requires the handle to close before rename.
            let oldest = self.path.with_extension(format!("log.{BACKUPS}"));
            if oldest.exists() {
                fs::remove_file(oldest)?;
            }
            for index in (1..BACKUPS).rev() {
                let source = self.path.with_extension(format!("log.{index}"));
                if source.exists() {
                    fs::rename(
                        source,
                        self.path.with_extension(format!("log.{}", index + 1)),
                    )?;
                }
            }
            fs::rename(&self.path, self.path.with_extension("log.1"))?;
        }
        if self.file.is_none() {
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)?,
            );
        }
        let file = self.file.as_mut().unwrap();
        file.write_all(line.as_bytes())?;
        file.flush()
    }
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    path: String,
    content: String,
}
pub fn snapshot() -> Result<Snapshot, String> {
    let logger = LOGGER
        .get()
        .ok_or("日志尚未初始化")?
        .lock()
        .map_err(|e| e.to_string())?;
    let mut file = File::open(&logger.path).map_err(|e| e.to_string())?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    file.seek(SeekFrom::Start(size.saturating_sub(64 * 1024)))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let mut content = String::from_utf8_lossy(&bytes).into_owned();
    if size > 64 * 1024 {
        content = content
            .split_once('\n')
            .map(|(_, rest)| rest.to_string())
            .unwrap_or_default();
    }
    Ok(Snapshot {
        path: logger.path.display().to_string(),
        content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotation_closes_handle_and_keeps_writing() {
        let dir = std::env::temp_dir().join(format!("keeper-log-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cangling-keeper.log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        file.set_len(MAX_BYTES).unwrap();
        let mut log = LogFile {
            path: path.clone(),
            file: Some(file),
        };
        log.append("error one\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "error one\n");
        assert_eq!(
            fs::metadata(path.with_extension("log.1")).unwrap().len(),
            MAX_BYTES
        );
        drop(log);
        fs::remove_dir_all(dir).unwrap();
    }
}
