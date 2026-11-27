//! Reversible shared-project lifecycle. Session lifecycle flags remain untouched.
use crate::store::Store;
use rusqlite::params;
use serde_json::{Value, json};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
fn sql<T>(r: rusqlite::Result<T>) -> Result<T, String> {
    r.map_err(|e| e.to_string())
}
pub(crate) fn initialize(s: &Store) -> Result<(), String> {
    sql(s.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_project_trash(project_id TEXT PRIMARY KEY,original_directory TEXT NOT NULL,deleted_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS agent_project_purges(project_id TEXT PRIMARY KEY,name TEXT NOT NULL,directory TEXT NOT NULL,original_directory TEXT NOT NULL,deleted_at INTEGER NOT NULL);"))?;
    recover_moves(s)?;
    // These jobs only exist after a confirmed, committed permanent deletion.
    if let Err(e) = finish_purges(s, None) {
        eprintln!("项目文件删除待重试：{e}");
    }
    Ok(())
}
fn journal_directory(s: &Store) -> Option<PathBuf> {
    s.conn
        .path()
        .filter(|p| !p.is_empty())
        .and_then(|p| Path::new(p).parent())
        .map(|p| p.join("project-moves"))
}
fn recover_moves(s: &Store) -> Result<(), String> {
    let Some(dir) = journal_directory(s) else {
        return Ok(());
    };
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let file = entry.map_err(|e| e.to_string())?.path();
        if file.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let record: Value =
            serde_json::from_slice(&std::fs::read(&file).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let project = record["project"].as_str().ok_or("项目移动日志无效")?;
        let old = Path::new(record["old"].as_str().ok_or("项目移动日志无效")?);
        let target = Path::new(record["target"].as_str().ok_or("项目移动日志无效")?);
        let directory: String = sql(s.conn.query_row(
            "SELECT directory FROM agent_projects WHERE id=?1",
            [project],
            |r| r.get(0),
        ))?;
        if Path::new(&directory) == old && target.exists() {
            if old.exists() {
                return Err(format!("项目目录恢复存在冲突：{}", old.display()));
            }
            std::fs::rename(target, old).map_err(|e| format!("恢复中断的项目移动失败：{e}"))?;
        } else if Path::new(&directory) != old && Path::new(&directory) != target {
            return Err("项目移动日志与数据库不匹配".into());
        }
        std::fs::remove_file(file).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn journal_move(
    s: &Store,
    project: &str,
    old: &Path,
    target: &Path,
) -> Result<Option<PathBuf>, String> {
    let Some(dir) = journal_directory(s) else {
        return Ok(None);
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.json", uuid::Uuid::new_v4()));
    let bytes = serde_json::to_vec(&json!({"project":project,"old":old,"target":target}))
        .map_err(|e| e.to_string())?;
    let temp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temp).map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    std::fs::rename(temp, &path).map_err(|e| e.to_string())?;
    Ok(Some(path))
}
pub(crate) fn list(s: &Store) -> Result<Value, String> {
    let mut q=sql(s.conn.prepare("SELECT p.id,p.name,p.directory,t.deleted_at,0 FROM agent_projects p JOIN agent_project_trash t ON t.project_id=p.id UNION ALL SELECT project_id,name,directory,deleted_at,1 FROM agent_project_purges ORDER BY 4 DESC"))?;
    let rows=sql(q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"directory":r.get::<_,String>(2)?,"deletedAt":r.get::<_,i64>(3)?,"pendingPurge":r.get::<_,bool>(4)?}))))?;
    Ok(json!(
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?
    ))
}

// Only remove a single project leaf from Keeper's own recycle-bin bucket.
fn purge_bucket(
    project: &str,
    directory: &str,
    original: &str,
) -> Result<(PathBuf, String), String> {
    uuid::Uuid::parse_str(project).map_err(|_| "项目删除路径无效")?;
    let root = crate::platform_dirs::home_dir()
        .ok_or("无法确定用户主目录")?
        .join("cangling/projects");
    if Path::new(original).parent() != Some(root.as_path()) {
        return Err("拒绝删除项目目录之外的文件".into());
    }
    let trash = root.join(".recycle-bin");
    let bucket = trash.join(project);
    let path = Path::new(directory);
    if path.parent() != Some(bucket.as_path()) {
        return Err("删除目标不在项目回收站中".into());
    }
    let leaf = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("删除目标无效")?;
    for part in [&trash, &bucket, path] {
        match std::fs::symlink_metadata(part) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err("拒绝通过符号链接删除项目目录".into());
            }
            Ok(m) if !m.is_dir() => return Err("项目删除目标必须是目录".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
            _ => {}
        }
    }
    Ok((bucket, leaf.into()))
}
fn finish_purges(s: &Store, only: Option<&[String]>) -> Result<(), String> {
    let mut q = sql(s
        .conn
        .prepare("SELECT project_id,directory,original_directory FROM agent_project_purges"))?;
    let jobs = sql(q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    }))?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|e| e.to_string())?;
    drop(q);
    let mut failures = 0;
    for (id, directory, original) in jobs {
        if only.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        let result = (|| {
            let (bucket, leaf) = purge_bucket(&id, &directory, &original)?;
            let projects = bucket
                .parent()
                .and_then(Path::parent)
                .ok_or("删除路径无效")?;
            let scoped = cap_std::fs::Dir::open_ambient_dir(projects, cap_std::ambient_authority())
                .and_then(|root| root.open_dir(".recycle-bin"))
                .and_then(|trash| trash.open_dir(&id));
            match scoped {
                Ok(root) => match root.remove_dir_all(&leaf) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.to_string()),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
            sql(s.conn.execute(
                "DELETE FROM agent_project_purges WHERE project_id=?1",
                [&id],
            ))?;
            let _ = std::fs::remove_dir(bucket); // only remove an empty wrapper
            Ok::<_, String>(())
        })();
        if let Err(e) = result {
            failures += 1;
            eprintln!("项目文件清理失败（{id}）：{e}");
        }
    }
    if failures > 0 {
        Err(format!(
            "{failures} 个项目的记录已删除，但部分磁盘文件未能删除。请检查权限后在回收站重试；服务重启时也会重试。"
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn purge(s: &Store, args: &Value, all: bool) -> Result<Value, String> {
    if args["confirmed"] != true {
        return Err("彻底删除项目、会话和磁盘文件需要确认，此操作无法撤销。".into());
    }
    recover_moves(s)?;
    let tx = sql(s.conn.unchecked_transaction())?;
    let current = list(s)?.as_array().cloned().unwrap_or_default();
    let targets = if all {
        let expected = args["projects"]
            .as_array()
            .ok_or("缺少已确认的回收站项目列表")?;
        if expected.len() != current.len()
            || current.iter().any(|p| {
                !expected
                    .iter()
                    .any(|e| e["id"] == p["id"] && e["deletedAt"] == p["deletedAt"])
            })
        {
            return Err("回收站内容已改变，请刷新后重新确认清空。".into());
        }
        current
    } else {
        let id = args["projectId"].as_str().ok_or("缺少项目 ID")?;
        let p = current
            .into_iter()
            .find(|p| p["id"] == id)
            .ok_or("项目不在回收站中")?;
        if args["deletedAt"] != p["deletedAt"] {
            return Err("项目回收站状态已改变，请刷新后重新确认。".into());
        }
        vec![p]
    };
    // Validate the complete batch before committing any irreversible work.
    for p in &targets {
        let id = p["id"].as_str().ok_or("项目 ID 无效")?;
        let table = if p["pendingPurge"] == true {
            "agent_project_purges"
        } else {
            "agent_project_trash"
        };
        let original: String = sql(s.conn.query_row(
            &format!("SELECT original_directory FROM {table} WHERE project_id=?1"),
            [id],
            |r| r.get(0),
        ))?;
        purge_bucket(
            id,
            p["directory"].as_str().ok_or("项目目录无效")?,
            &original,
        )?;
        let mut q = sql(s
            .conn
            .prepare("SELECT id FROM agent_sessions WHERE project_id=?1"))?;
        let sessions = sql(q.query_map([id], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?;
        for session in sessions {
            crate::agent_history::require_idle(s, &session)?;
        }
    }
    let ids: Vec<String> = targets
        .iter()
        .map(|p| p["id"].as_str().unwrap().into())
        .collect();
    for p in &targets {
        if p["pendingPurge"] == true {
            continue;
        }
        let id = p["id"].as_str().unwrap();
        sql(s.conn.execute("INSERT INTO agent_project_purges SELECT p.id,p.name,p.directory,t.original_directory,t.deleted_at FROM agent_projects p JOIN agent_project_trash t ON t.project_id=p.id WHERE p.id=?1",[id]))?;
        let mut q = sql(s
            .conn
            .prepare("SELECT id FROM agent_sessions WHERE project_id=?1"))?;
        let sessions = sql(q.query_map([id], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?;
        drop(q);
        for session in sessions {
            crate::agent_history::purge_records(s, &session)?;
        }
        for table in ["agent_earth_images", "agent_earth_models"] {
            let exists: bool = sql(s.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |r| r.get(0),
            ))?;
            if exists {
                sql(s
                    .conn
                    .execute(&format!("DELETE FROM {table} WHERE project=?1"), [id]))?;
            }
        }
        sql(s.conn.execute("DELETE FROM settings WHERE key LIKE 'agent_project_pin:%:'||?1 OR key LIKE 'earth:%:'||?1 OR key LIKE 'earth:%:'||?1||':%'",[id]))?;
        sql(s
            .conn
            .execute("DELETE FROM agent_project_trash WHERE project_id=?1", [id]))?;
        sql(s
            .conn
            .execute("DELETE FROM agent_projects WHERE id=?1", [id]))?;
    }
    sql(tx.commit())?;
    // Persist deletion intent before touching files, so failures/crashes are retryable.
    finish_purges(s, Some(&ids))?;
    Ok(json!({"purged":true,"deletedProjects":ids.len(),"filesDeleted":true}))
}

pub(crate) fn name_component(name: &str) -> String {
    let text: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || "/\\<>:\"|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let text: String = text
        .chars()
        .scan(0, |bytes, c| {
            *bytes += c.len_utf8();
            (*bytes <= 200).then_some(c)
        })
        .collect();
    let text = text.trim_end_matches(['.', ' ']);
    if text.is_empty() {
        "项目".into()
    } else if text == ".recycle-bin" {
        "_recycle-bin".into()
    } else {
        text.into()
    }
}
fn destination(parent: &Path, name: &str, source: Option<&Path>) -> Result<PathBuf, String> {
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    for n in 0..10000 {
        let path = parent.join(if n == 0 {
            name_component(name)
        } else {
            format!("{} ({n})", name_component(name))
        });
        if source == Some(path.as_path()) {
            return Ok(path);
        }
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(path),
            Err(e) => return Err(e.to_string()),
            Ok(_) => {}
        }
    }
    Err("同名项目目录过多".into())
}
pub(crate) fn create(name: &str) -> Result<String, String> {
    let parent = crate::platform_dirs::home_dir()
        .ok_or("无法确定用户主目录")?
        .join("cangling/projects");
    loop {
        let path = destination(&parent, name, None)?;
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path.to_string_lossy().into_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
}
pub(crate) fn move_project(
    s: &Store,
    project: &str,
    name: Option<&str>,
    restore: bool,
) -> Result<Value, String> {
    if project == "general" {
        return Err("通用项目不支持移动或删除".into());
    }
    recover_moves(s)?;
    let tx = sql(s.conn.unchecked_transaction())?;
    let (old_name, old): (String, String) = sql(s.conn.query_row(
        "SELECT name,directory FROM agent_projects WHERE id=?1",
        [project],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    let trashed: bool = sql(s.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_project_trash WHERE project_id=?1)",
        [project],
        |r| r.get(0),
    ))?;
    if restore != trashed {
        return Err(if trashed {
            "项目已在回收站"
        } else {
            "项目不在回收站"
        }
        .into());
    }
    let sessions = sql(sql(s
        .conn
        .prepare("SELECT id FROM agent_sessions WHERE project_id=?1"))?
    .query_map([project], |r| r.get::<_, String>(0)))?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|e| e.to_string())?;
    for session in &sessions {
        crate::agent_history::require_idle(s, session)?;
    }
    let old_path = Path::new(&old);
    if !old_path.is_absolute()
        || std::fs::symlink_metadata(old_path)
            .map_err(|e| format!("项目目录不可访问：{e}"))?
            .file_type()
            .is_symlink()
        || !old_path.is_dir()
    {
        return Err("项目目录必须为真实目录".into());
    }
    let parent = if restore {
        let original: String = sql(s.conn.query_row(
            "SELECT original_directory FROM agent_project_trash WHERE project_id=?1",
            [project],
            |r| r.get(0),
        ))?;
        PathBuf::from(original)
            .parent()
            .ok_or("项目目录无效")?
            .to_path_buf()
    } else if name.is_some() {
        old_path.parent().ok_or("项目目录无效")?.to_path_buf()
    } else {
        old_path
            .parent()
            .ok_or("项目目录无效")?
            .join(".recycle-bin")
            .join(project)
    };
    let new_name = name.unwrap_or(&old_name);
    let target = destination(&parent, new_name, Some(old_path))?;
    let moved = target != old_path;
    let journal = if moved {
        journal_move(s, project, old_path, &target)?
    } else {
        None
    };
    if moved {
        if let Err(e) = std::fs::rename(old_path, &target) {
            if let Some(file) = &journal {
                let _ = std::fs::remove_file(file);
            }
            return Err(format!("移动项目目录失败：{e}"));
        }
    }
    let result = (|| {
        if restore {
            sql(s.conn.execute(
                "DELETE FROM agent_project_trash WHERE project_id=?1",
                [project],
            ))?;
        } else if name.is_none() {
            sql(s.conn.execute(
                "INSERT INTO agent_project_trash VALUES (?1,?2,?3)",
                params![project, old, chrono::Utc::now().timestamp_millis()],
            ))?;
        }
        sql(s.conn.execute(
            "UPDATE agent_projects SET name=?2,directory=?3 WHERE id=?1",
            params![project, new_name, target.to_string_lossy()],
        ))?;
        // Tool undo/review paths must follow the project after restoration or renaming.
        sql(s.conn.execute("UPDATE agent_tasks SET project_directory=?2 WHERE session_id IN (SELECT id FROM agent_sessions WHERE project_id=?1)",params![project,target.to_string_lossy()]))?;
        sql(tx.commit())
    })();
    if let Err(e) = result {
        if moved {
            std::fs::rename(&target, old_path).map_err(|rollback| {
                format!(
                    "{e}；目录回滚失败：{rollback}，文件位于 {}",
                    target.display()
                )
            })?;
        }
        if let Some(file) = &journal {
            let _ = std::fs::remove_file(file);
        }
        return Err(e);
    }
    if let Some(file) = &journal {
        let _ = std::fs::remove_file(file);
    }
    Ok(
        json!({"id":project,"name":new_name,"directory":target,"deleted":name.is_none() && !restore,"restored":restore,"recycled":name.is_none() && !restore,"filesPreserved":true,"sessionCount":sessions.len()}),
    )
}

// Migrate only Keeper's legacy UUID directories, never arbitrary imported paths.
pub(crate) fn migrate_names(s: &Store) -> Result<(), String> {
    let root = crate::platform_dirs::home_dir()
        .ok_or("无法确定用户主目录")?
        .join("cangling/projects");
    let mut q=sql(s.conn.prepare("SELECT id,name,directory FROM agent_projects WHERE id!='general' AND NOT EXISTS(SELECT 1 FROM agent_project_trash t WHERE t.project_id=agent_projects.id)"))?;
    let projects = sql(q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    }))?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|e| e.to_string())?;
    drop(q);
    for (id, name, directory) in projects {
        let path = Path::new(&directory);
        if path.parent() == Some(root.as_path())
            && path.file_name().and_then(|v| v.to_str()) == Some(id.as_str())
            && path.is_dir()
        {
            if let Err(e) = move_project(s, &id, Some(&name), false) {
                eprintln!("项目目录名称迁移暂未完成（{id}）：{e}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_are_safe_and_fit_filesystem_limits() {
        assert_eq!(name_component("../test\\file"), ".._test_file");
        assert_eq!(name_component(".."), "项目");
        assert_eq!(name_component(".recycle-bin"), "_recycle-bin");
        assert!(name_component(&"🌍".repeat(80)).len() <= 200);
    }
}
