//! Capability-scoped, bounded read-only project tools. Never exposes ambient host paths.
use cap_std::fs::Dir;
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(crate) fn definitions() -> Vec<Value> {
    [
        ("list_directory", "列出当前项目内目录，返回名称、类型和大小。path 默认为 .，只允许项目相对路径。", json!({"path":{"type":"string"}}), json!([])),
        ("read_file", "读取项目 UTF-8 文本文件，返回带行号内容；可指定 startLine 和最多 200 行的 lineCount。", json!({"path":{"type":"string"},"startLine":{"type":"integer","minimum":1},"lineCount":{"type":"integer","minimum":1,"maximum":200}}), json!(["path"])),
        ("search_files", "在项目文件名和 UTF-8 文本中搜索字面字符串，递归搜索指定 path（默认 .）；跳过链接、私密文件和构建缓存。", json!({"path":{"type":"string"},"query":{"type":"string","minLength":1,"maxLength":200},"caseSensitive":{"type":"boolean"}}), json!(["query"])),
    ].into_iter().map(|(name,description,properties,required)|json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})).chain(crate::agent_actions::definitions()).chain(std::iter::once(crate::agent_skills::definition())).collect()
}
pub(crate) fn title(name: &str) -> &str {
    match name {
        "list_directory" => "列出项目目录",
        "read_file" => "读取项目文件",
        "search_files" => "搜索项目文件",
        "load_skill" => "加载项目技能",
        _ => "未知工具",
    }
}
pub(crate) fn excluded(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".ssh"
            | ".aws"
            | ".gnupg"
            | "node_modules"
            | "target"
            | "build"
            | "dist"
            | ".venv"
            | "__pycache__"
    ) || name == ".env"
        || name.starts_with(".env.")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
}
struct Scope {
    dir: Dir,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    visited: usize,
    bytes: usize,
}
impl Scope {
    fn check(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::Relaxed) {
            Err("工具已停止".into())
        } else if Instant::now() > self.deadline {
            Err("工具超过执行时限".into())
        } else {
            Ok(())
        }
    }
    fn path(&self, value: &str) -> Result<PathBuf, String> {
        if value.len() > 2048 || value.contains('\\') || value.contains(':') || value.contains('\0')
        {
            return Err("无效的项目相对路径".into());
        }
        let path = Path::new(value);
        let mut clean = PathBuf::new();
        for part in path.components() {
            match part {
                Component::Normal(name) => {
                    if excluded(&name.to_string_lossy()) {
                        return Err("该路径属于私密文件或构建缓存，未开放读取".into());
                    }
                    clean.push(name);
                }
                Component::CurDir => {}
                _ => return Err("只允许项目内相对路径，不允许 .. 或绝对路径".into()),
            }
        }
        if clean.as_os_str().is_empty() {
            clean.push(".");
        }
        // Reject links as well as using a capability dir that cannot escape the project.
        let mut part = PathBuf::new();
        for name in clean.components() {
            part.push(name);
            let m = self
                .dir
                .symlink_metadata(&part)
                .map_err(|_| "项目路径不存在或不可访问")?;
            if m.file_type().is_symlink() {
                return Err("不读取符号链接".into());
            }
        }
        Ok(clean)
    }
    fn text(&mut self, path: &Path) -> Result<String, String> {
        self.check()?;
        let metadata = self
            .dir
            .symlink_metadata(path)
            .map_err(|_| "文件不存在或不可访问")?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("只支持普通文本文件".into());
        }
        if metadata.len() > 1024 * 1024 {
            return Err("文件超过 1 MiB，请读取较小的文本文件".into());
        }
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
        }
        let mut file = self
            .dir
            .open_with(path, &options)
            .map_err(|_| "无法读取项目文件")?;
        if !file.metadata().map_err(|_| "无法检查文件类型")?.is_file() {
            return Err("只支持普通文本文件".into());
        }
        let mut data = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            self.check()?;
            let n = file.read(&mut chunk).map_err(|_| "读取文件失败")?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(&chunk[..n]);
            self.bytes += n;
            if data.len() > 1024 * 1024 || self.bytes > 16 * 1024 * 1024 {
                return Err("已达到文件读取大小限制".into());
            }
        }
        if data.contains(&0) {
            return Err("跳过二进制文件".into());
        }
        String::from_utf8(data).map_err(|_| "只支持 UTF-8 文本文件".into())
    }
    fn entries(&mut self, path: &Path) -> Result<Vec<(String, PathBuf, bool, u64)>, String> {
        self.check()?;
        let mut result = Vec::new();
        for item in self
            .dir
            .read_dir(path)
            .map_err(|_| "目录不存在或不可访问")?
        {
            self.check()?;
            self.visited += 1;
            if self.visited > 3000 {
                return Err("已达到 3000 个目录条目的扫描限制，请缩小搜索范围".into());
            }
            let item = item.map_err(|_| "读取目录失败")?;
            let Ok(name) = item.file_name().into_string() else {
                continue;
            };
            if excluded(&name) {
                continue;
            }
            let child = path.join(&name);
            let metadata = self
                .dir
                .symlink_metadata(&child)
                .map_err(|_| "读取条目信息失败")?;
            if metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
                continue;
            }
            result.push((name, child, metadata.is_dir(), metadata.len()));
        }
        result.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
        Ok(result)
    }
}
pub(crate) fn execute(
    root: &Dir,
    name: &str,
    args: &Value,
    cancel: Arc<AtomicBool>,
    mut progress: impl FnMut(Value),
) -> Result<Value, String> {
    let properties = definitions()
        .into_iter()
        .find(|d| d["function"]["name"] == name)
        .ok_or("模型请求了未开放的工具")?["function"]["parameters"]["properties"]
        .clone();
    let object = args.as_object().ok_or("工具参数必须是 JSON 对象")?;
    if object.keys().any(|k| properties.get(k).is_none()) {
        return Err("工具参数包含未定义的字段".into());
    }
    if name=="load_skill" {return crate::agent_skills::load(root,args,cancel);}
    let dir = root.try_clone().map_err(|_| "无法打开当前项目目录")?;
    let mut scope = Scope {
        dir,
        cancel,
        deadline: Instant::now() + Duration::from_secs(8),
        visited: 0,
        bytes: 0,
    };
    let raw = match args.get("path") {
        Some(v) => v.as_str().ok_or("path 必须是字符串")?,
        None => ".",
    };
    let path = scope.path(raw)?;
    match name {
        "list_directory" => {
            let entries = scope.entries(&path)?;
            let truncated = entries.len() > 200;
            Ok(
                json!({"path":raw,"entries":entries.into_iter().take(200).map(|(name,_,directory,size)|json!({"name":name,"type":if directory{"directory"}else{"file"},"size":size})).collect::<Vec<_>>(),"truncated":truncated}),
            )
        }
        "read_file" => {
            let number = |key: &str, default: u64| -> Result<u64, String> {
                match args.get(key) {
                    None => Ok(default),
                    Some(v) => v
                        .as_u64()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| format!("{key} 必须是正整数")),
                }
            };
            let start = number("startLine", 1)?;
            let count = number("lineCount", 200)?;
            if count > 200 {
                return Err("lineCount 最多 200".into());
            }
            let text = scope.text(&path)?;
            let mut lines = Vec::new();
            let mut size = 0;
            let mut total = 0;
            let mut truncated = false;
            for (i, line) in text.lines().enumerate() {
                scope.check()?;
                total = i + 1;
                if (i as u64) + 1 < start {
                    continue;
                }
                if lines.len() >= count as usize || size >= 12000 {
                    truncated = true;
                    continue;
                }
                let clipped = &line[..line.floor_char_boundary(1000)];
                size += clipped.len();
                truncated |= clipped.len() != line.len();
                lines.push(json!({"line":i+1,"text":clipped}));
            }
            Ok(
                json!({"path":raw,"startLine":start,"lines":lines,"totalLines":total,"truncated":truncated}),
            )
        }
        "search_files" => {
            let query = args["query"]
                .as_str()
                .filter(|q| !q.is_empty() && q.len() <= 200)
                .ok_or("query 必须为 1–200 字节的字符串")?;
            let sensitive = match args.get("caseSensitive") {
                None => false,
                Some(v) => v.as_bool().ok_or("caseSensitive 必须为布尔值")?,
            };
            let needle = if sensitive {
                query.to_owned()
            } else {
                query.to_lowercase()
            };
            let matches = |s: &str| {
                if sensitive {
                    s.contains(&needle)
                } else {
                    s.to_lowercase().contains(&needle)
                }
            };
            let mut stack = vec![(path, 0)];
            let mut found = Vec::new();
            let mut skipped = 0;
            let mut files = 0;
            let mut truncated = false;
            'scan: while let Some((path, depth)) = stack.pop() {
                scope.check()?;
                if depth > 24 {
                    skipped += 1;
                    truncated = true;
                    continue;
                }
                let metadata = scope
                    .dir
                    .symlink_metadata(&path)
                    .map_err(|_| "路径不可访问")?;
                if metadata.is_dir() {
                    let mut entries = scope.entries(&path)?;
                    entries.reverse();
                    for (_, child, _, _) in entries {
                        stack.push((child, depth + 1));
                    }
                    continue;
                }
                files += 1;
                if files % 20 == 0 {
                    progress(json!({"scannedFiles":files,"matches":found.len()}));
                }
                let label = path.to_string_lossy().trim_start_matches("./").to_owned();
                if matches(&label) {
                    found.push(json!({"path":label,"kind":"filename"}));
                }
                match scope.text(&path) {
                    Ok(text) => {
                        for (i, line) in text.lines().enumerate() {
                            scope.check()?;
                            if matches(line) {
                                found.push(json!({"path":label,"line":i+1,"text":&line[..line.floor_char_boundary(300)]}));
                                if found.len() >= 50 {
                                    truncated = true;
                                    break 'scan;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        scope.check()?;
                        if scope.bytes > 16 * 1024 * 1024 {
                            return Err(e);
                        }
                        skipped += 1;
                    }
                }
                if found.len() >= 50 {
                    truncated = true;
                    break;
                }
            }
            Ok(
                json!({"query":query,"matches":found,"scannedFiles":files,"skippedFiles":skipped,"truncated":truncated}),
            )
        }
        _ => Err("未开放的工具".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, Dir) {
        let root = std::env::temp_dir().join(format!("keeper-readtools-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("README.md"), "# Demo\nproject needle\nthird line").unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() { /* needle */ }").unwrap();
        let dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        (root, dir)
    }
    fn run(dir: &Dir, name: &str, args: Value) -> Result<Value, String> {
        execute(dir, name, &args, Arc::new(AtomicBool::new(false)), |_| {})
    }
    #[test]
    fn lists_reads_and_searches_with_line_numbers() {
        let (root, dir) = fixture();
        let entries = run(&dir, "list_directory", json!({})).unwrap();
        assert_eq!(entries["entries"][0]["name"], "src");
        let file = run(
            &dir,
            "read_file",
            json!({"path":"README.md","startLine":2,"lineCount":1}),
        )
        .unwrap();
        assert_eq!(file["lines"][0], json!({"line":2,"text":"project needle"}));
        assert_eq!(file["truncated"], true);
        let found = run(&dir, "search_files", json!({"query":"NEEDLE"})).unwrap();
        assert_eq!(found["matches"].as_array().unwrap().len(), 2);
        drop(dir);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_ambient_paths_secrets_binary_and_invalid_arguments() {
        let (root, dir) = fixture();
        std::fs::write(root.join(".env"), "secret").unwrap();
        std::fs::write(root.join("binary"), [0, 1, 2]).unwrap();
        for path in ["../outside", "/etc/passwd", "C:\\Windows", ".env", "binary"] {
            assert!(
                run(&dir, "read_file", json!({"path":path})).is_err(),
                "{path}"
            );
        }
        assert!(run(&dir, "run_command", json!({"command":"uname"})).is_err());
        assert!(run(&dir, "list_directory", json!({"unknown":1})).is_err());
        assert!(
            run(
                &dir,
                "read_file",
                json!({"path":"README.md","lineCount":201})
            )
            .is_err()
        );
        assert!(run(&dir, "search_files", json!({"query":""})).is_err());
        drop(dir);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cancellation_and_large_output_are_bounded() {
        let (root, dir) = fixture();
        let flag = Arc::new(AtomicBool::new(true));
        assert!(
            execute(
                &dir,
                "search_files",
                &json!({"query":"needle"}),
                flag,
                |_| {}
            )
            .is_err()
        );
        std::fs::write(root.join("long"), "x".repeat(20000)).unwrap();
        let file = run(&dir, "read_file", json!({"path":"long"})).unwrap();
        assert_eq!(file["lines"][0]["text"].as_str().unwrap().len(), 1000);
        assert_eq!(file["truncated"], true);
        drop(dir);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_never_escape_project_or_enter_search() {
        let (root, dir) = fixture();
        std::os::unix::fs::symlink("/etc", root.join("outside")).unwrap();
        std::os::unix::fs::symlink("README.md", root.join("alias")).unwrap();
        assert!(run(&dir, "read_file", json!({"path":"outside/passwd"})).is_err());
        assert!(run(&dir, "read_file", json!({"path":"alias"})).is_err());
        let entries = run(&dir, "list_directory", json!({})).unwrap();
        assert!(
            !entries["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["name"] == "outside" || e["name"] == "alias")
        );
        drop(dir);
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// Full, lossless editor read. Never assemble editable content from clipped tool pages.
pub(crate) fn editor_text(root:&Dir,raw:&str)->Result<Value,String>{
    let mut scope=Scope{dir:root.try_clone().map_err(|_|"无法打开项目")?,cancel:Arc::new(AtomicBool::new(false)),deadline:Instant::now()+Duration::from_secs(8),visited:0,bytes:0};
    let path=scope.path(raw)?;let content=scope.text(&path)?;
    let editable=content.len()<=131072;
    Ok(json!({"content":content,"sha256":crate::agent_actions::hash(content.as_bytes()),"editable":editable}))
}
