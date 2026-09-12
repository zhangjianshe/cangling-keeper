use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

pub(crate) const DATABASE_NAME: &str = ".cangling-software-fingerprints.sqlite3";
const SCHEMA_VERSION: &str = "1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileFingerprint {
    pub(crate) path: String,
    pub(crate) size: u64,
    pub(crate) modified_ns: i64,
    pub(crate) sha256: String,
}

#[derive(Debug)]
pub(crate) struct FingerprintDatabase {
    pub(crate) files: Vec<FileFingerprint>,
}

pub(crate) fn database_path(repository_root: &Path) -> PathBuf {
    repository_root.join(DATABASE_NAME)
}

fn is_database_file(name: &str) -> bool {
    name == DATABASE_NAME
        || name.starts_with(&format!("{DATABASE_NAME}-"))
        || name.starts_with(".cangling-fingerprint-build-")
}

fn collect_repository_files(
    root: &Path,
    directory: &Path,
    output: &mut Vec<(String, PathBuf, u64, i64)>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("读取软件仓库目录 {} 失败：{error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" || name.ends_with(".part") || is_database_file(&name) {
            continue;
        }
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        if metadata.is_dir() {
            collect_repository_files(root, &path, output)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if relative.is_empty() {
            continue;
        }
        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .and_then(|value| i64::try_from(value.as_nanos()).ok())
            .unwrap_or(0);
        output.push((relative, path, metadata.len(), modified_ns));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn read_database(path: &Path) -> Result<Vec<FileFingerprint>, String> {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("打开指纹数据库 {} 失败：{error}", path.display()))?;
    let mut statement = connection
        .prepare("SELECT path, size, modified_ns, sha256 FROM files ORDER BY path")
        .map_err(|error| format!("读取指纹数据库失败：{error}"))?;
    let rows = statement
        .query_map([], |row| {
            let size: i64 = row.get(1)?;
            Ok(FileFingerprint {
                path: row.get(0)?,
                size: u64::try_from(size).unwrap_or(0),
                modified_ns: row.get(2)?,
                sha256: row.get(3)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let mut files = Vec::new();
    for row in rows {
        let fingerprint = row.map_err(|error| error.to_string())?;
        if fingerprint.path.is_empty()
            || fingerprint.sha256.len() != 64
            || !fingerprint
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("指纹数据库中存在无效记录".into());
        }
        files.push(fingerprint);
    }
    Ok(files)
}

fn read_existing(path: &Path) -> HashMap<String, FileFingerprint> {
    read_database(path)
        .unwrap_or_default()
        .into_iter()
        .map(|file| (file.path.clone(), file))
        .collect()
}

pub(crate) fn write_database(path: &Path, files: &[FileFingerprint]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "指纹数据库目录无效".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(
        ".cangling-fingerprint-build-{}.sqlite3",
        uuid::Uuid::new_v4()
    ));
    let result = (|| -> Result<(), String> {
        let mut connection = Connection::open(&temporary).map_err(|error| error.to_string())?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=OFF;
                 PRAGMA synchronous=FULL;
                 CREATE TABLE metadata (
                     key TEXT PRIMARY KEY,
                     value TEXT NOT NULL
                 );
                 CREATE TABLE files (
                     path TEXT PRIMARY KEY,
                     size INTEGER NOT NULL,
                     modified_ns INTEGER NOT NULL,
                     sha256 TEXT NOT NULL
                 );",
            )
            .map_err(|error| error.to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO metadata(key, value) VALUES ('schema_version', ?1)",
                [SCHEMA_VERSION],
            )
            .map_err(|error| error.to_string())?;
        {
            let mut insert = transaction
                .prepare(
                    "INSERT INTO files(path, size, modified_ns, sha256) VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(|error| error.to_string())?;
            for file in files {
                let size = i64::try_from(file.size)
                    .map_err(|_| format!("文件过大，无法记录：{}", file.path))?;
                insert
                    .execute(params![file.path, size, file.modified_ns, file.sha256])
                    .map_err(|error| error.to_string())?;
            }
        }
        transaction.commit().map_err(|error| error.to_string())?;
        connection.close().map_err(|(_, error)| error.to_string())?;
        std::fs::rename(&temporary, path).map_err(|error| error.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn upsert_fingerprint(path: &Path, fingerprint: &FileFingerprint) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "指纹数据库目录无效".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let connection = Connection::open(path).map_err(|error| error.to_string())?;
    connection
        .execute_batch(
            "PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS metadata (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS files (
                 path TEXT PRIMARY KEY,
                 size INTEGER NOT NULL,
                 modified_ns INTEGER NOT NULL,
                 sha256 TEXT NOT NULL
             );",
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO metadata(key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [SCHEMA_VERSION],
        )
        .map_err(|error| error.to_string())?;
    let size = i64::try_from(fingerprint.size)
        .map_err(|_| format!("文件过大，无法记录：{}", fingerprint.path))?;
    connection
        .execute(
            "INSERT INTO files(path, size, modified_ns, sha256) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET
                 size=excluded.size,
                 modified_ns=excluded.modified_ns,
                 sha256=excluded.sha256",
            params![
                fingerprint.path,
                size,
                fingerprint.modified_ns,
                fingerprint.sha256
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn record_file(
    repository_root: &Path,
    absolute_path: &Path,
    verified_sha256: Option<&str>,
) -> Result<FileFingerprint, String> {
    let metadata = absolute_path
        .metadata()
        .map_err(|error| format!("读取文件信息失败：{error}"))?;
    if !metadata.is_file() {
        return Err(format!("不是普通文件：{}", absolute_path.display()));
    }
    let relative = absolute_path
        .strip_prefix(repository_root)
        .map_err(|_| format!("文件不在软件仓库中：{}", absolute_path.display()))?
        .to_string_lossy()
        .replace('\\', "/");
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .and_then(|value| i64::try_from(value.as_nanos()).ok())
        .unwrap_or(0);
    let verified_sha256 = verified_sha256
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(|value| value.to_ascii_lowercase());
    let sha256 = match verified_sha256 {
        Some(value) => value,
        None => sha256_file(absolute_path)?,
    };
    let fingerprint = FileFingerprint {
        path: relative,
        size: metadata.len(),
        modified_ns,
        sha256,
    };

    upsert_fingerprint(&database_path(repository_root), &fingerprint)?;
    Ok(fingerprint)
}

/// Refresh the repository fingerprint database. Files whose size and mtime
/// still match the previous row reuse their SHA-256; only changed files are read.
pub(crate) fn refresh_database(repository_root: &Path) -> Result<FingerprintDatabase, String> {
    refresh_database_with_progress(repository_root, |_, _, _, _| {})
}

pub(crate) fn refresh_database_with_progress(
    repository_root: &Path,
    mut on_progress: impl FnMut(usize, usize, &str, bool),
) -> Result<FingerprintDatabase, String> {
    std::fs::create_dir_all(repository_root).map_err(|error| error.to_string())?;
    let path = database_path(repository_root);
    let previous = read_existing(&path);
    let mut discovered = Vec::new();
    collect_repository_files(repository_root, repository_root, &mut discovered)?;
    discovered.sort_by(|left, right| left.0.cmp(&right.0));

    let mut files = Vec::with_capacity(discovered.len());
    let total = discovered.len();
    for (index, (relative, absolute, size, modified_ns)) in discovered.into_iter().enumerate() {
        let cached = previous
            .get(&relative)
            .filter(|saved| saved.size == size && saved.modified_ns == modified_ns)
            .map(|saved| saved.sha256.clone());
        let hashed = cached.is_none();
        let sha256 = match cached {
            Some(sha256) => sha256,
            None => sha256_file(&absolute)?,
        };
        on_progress(index + 1, total, &relative, hashed);
        files.push(FileFingerprint {
            path: relative,
            size,
            modified_ns,
            sha256,
        });
    }
    on_progress(total, total, "正在保存 SQLite 指纹数据库…", false);
    write_database(&path, &files)?;
    Ok(FingerprintDatabase { files })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_records_sha256_and_removes_deleted_files() {
        let root = std::env::temp_dir().join(format!("ck-fingerprints-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("np4/.git")).unwrap();
        std::fs::write(root.join("np4/app.jar"), b"first").unwrap();
        std::fs::write(root.join("np4/partial.part"), b"partial").unwrap();
        std::fs::write(root.join("np4/.git/config"), b"ignored").unwrap();

        let mut progress = Vec::new();
        let first = refresh_database_with_progress(&root, |current, total, path, hashed| {
            progress.push((current, total, path.to_string(), hashed));
        })
        .unwrap();
        assert_eq!(first.files.len(), 1);
        assert_eq!(progress.len(), 2);
        assert_eq!(progress[0], (1, 1, "np4/app.jar".to_string(), true));
        assert_eq!(progress[1].2, "正在保存 SQLite 指纹数据库…");
        assert_eq!(first.files[0].path, "np4/app.jar");
        assert_eq!(
            first.files[0].sha256,
            sha256_file(&root.join("np4/app.jar")).unwrap()
        );
        assert_eq!(read_database(&database_path(&root)).unwrap(), first.files);

        std::fs::remove_file(root.join("np4/app.jar")).unwrap();
        let second = refresh_database(&root).unwrap();
        assert!(second.files.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn record_file_commits_each_file_immediately() {
        let root =
            std::env::temp_dir().join(format!("ck-fingerprint-row-{}", uuid::Uuid::new_v4()));
        let file = root.join("np4/app.jar");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"v1").unwrap();

        let first = record_file(&root, &file, None).unwrap();
        assert_eq!(
            read_database(&database_path(&root)).unwrap(),
            vec![first.clone()]
        );

        std::fs::write(&file, b"v2").unwrap();
        let second = record_file(&root, &file, None).unwrap();
        let saved = read_database(&database_path(&root)).unwrap();
        assert_eq!(saved, vec![second.clone()]);
        assert_eq!(saved.len(), 1);
        assert_ne!(saved[0].sha256, first.sha256);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn upsert_fingerprint_creates_parent_and_updates_one_row() {
        let root =
            std::env::temp_dir().join(format!("ck-host-fingerprint-row-{}", uuid::Uuid::new_v4()));
        let database = root.join("host/fingerprints.sqlite3");
        let mut fingerprint = FileFingerprint {
            path: "np4/app.jar".into(),
            size: 10,
            modified_ns: 0,
            sha256: "a".repeat(64),
        };
        upsert_fingerprint(&database, &fingerprint).unwrap();
        fingerprint.size = 20;
        fingerprint.sha256 = "b".repeat(64);
        upsert_fingerprint(&database, &fingerprint).unwrap();

        assert_eq!(read_database(&database).unwrap(), vec![fingerprint]);
        let _ = std::fs::remove_dir_all(root);
    }
}
