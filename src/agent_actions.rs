//! Reviewed file edits and owned local subprocesses. Preparation never mutates a project.
use cap_std::fs::Dir;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(crate) fn definitions() -> Vec<Value> {
    [
 ("write_file","创建或覆盖项目内 UTF-8 文件。由系统安全策略决定自动批准或等待用户确认；父目录必须存在。新建文件使用 createOnly=true，同名文件存在时拒绝覆盖；不能与 expectedSha256 同时使用。修改已有文件时应将 read_file 返回的 sha256 传入 expectedSha256；不匹配则拒绝生成修改预览。",json!({"path":{"type":"string"},"content":{"type":"string"},"createOnly":{"type":"boolean"},"expectedSha256":{"type":"string","minLength":64,"maxLength":64}}),json!(["path","content"])),
 ("apply_patch","精确替换同一文件：使用 oldText/newText 单组参数，或 edits 数组（1–20组），不能混用。各组按顺序基于上一步结果唯一匹配，oldText 不能为空；任何一组失败均不写入，全部成功后只生成一份审批差异并写入一次。由系统安全策略决定自动批准或等待用户确认；应传入 read_file 的 sha256 作为 expectedSha256，拒绝基于旧版本修改。",json!({"path":{"type":"string"},"oldText":{"type":"string"},"newText":{"type":"string"},"edits":{"type":"array","minItems":1,"maxItems":20,"items":{"type":"object","properties":{"oldText":{"type":"string","minLength":1},"newText":{"type":"string"}},"required":["oldText","newText"],"additionalProperties":false}},"expectedSha256":{"type":"string","minLength":64,"maxLength":64}}),json!(["path"])),
 ("run_command","提出本机 shell 命令，默认在当前项目根目录运行。系统按安全模式决定自动批准或等待用户确认完整命令和执行目录。以 Keeper 用户权限执行，不是文件沙箱。timeoutSeconds 默认 60，最多 120。",json!({"command":{"type":"string"},"cwd":{"type":"string"},"timeoutSeconds":{"type":"integer","minimum":1,"maximum":120}}),json!(["command"])),
].into_iter().map(|(name,description,properties,required)|json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})).collect()
}
pub(crate) fn is_action(name: &str) -> bool {
    matches!(name, "write_file" | "apply_patch" | "run_command")
}
pub(crate) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn check(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("操作已停止，未开始新的写入或命令".into())
    } else {
        Ok(())
    }
}
fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .ok_or_else(|| format!("{key} 必须是字符串"))
}
fn path(value: &str, allow_root: bool) -> Result<PathBuf, String> {
    if value.len() > 2048
        || value.chars().any(char::is_control)
        || value.contains('\\')
        || value.contains(':')
    {
        return Err("无效的项目相对路径".into());
    }
    let mut result = PathBuf::new();
    for part in Path::new(value).components() {
        match part {
            Component::Normal(name) => {
                if crate::agent_files::excluded(&name.to_string_lossy().to_ascii_lowercase()) {
                    return Err("未开放修改此私密文件或构建缓存路径".into());
                }
                result.push(name);
            }
            Component::CurDir => {}
            _ => return Err("仅允许项目相对路径，不能使用 .. 或绝对路径".into()),
        }
    }
    if result.as_os_str().is_empty() {
        if allow_root {
            result.push(".")
        } else {
            return Err("文件路径不能为空".into());
        }
    }
    Ok(result)
}
fn no_links(root: &Dir, path: &Path, missing_last: bool) -> Result<(), String> {
    let mut current = PathBuf::new();
    let parts = path.components().collect::<Vec<_>>();
    for (i, part) in parts.iter().enumerate() {
        current.push(part);
        match root.symlink_metadata(&current) {
            Ok(m) => {
                if m.file_type().is_symlink() {
                    return Err("不通过符号链接执行操作".into());
                }
            }
            Err(e)
                if missing_last
                    && i + 1 == parts.len()
                    && e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("项目路径不存在或不可访问".into()),
        }
    }
    Ok(())
}
fn snapshot(
    parent: &Dir,
    name: &Path,
) -> Result<Option<(String, cap_std::fs::Permissions)>, String> {
    match parent.symlink_metadata(name) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("无法检查文件".into()),
        Ok(m) => {
            if !m.is_file() || m.file_type().is_symlink() {
                return Err("仅支持普通 UTF-8 文件，不操作链接或特殊文件".into());
            }
            if m.len() > 131072 {
                return Err("编辑文件最多 128 KiB".into());
            }
            let mut opts = cap_std::fs::OpenOptions::new();
            opts.read(true);
            #[cfg(unix)]
            {
                use cap_std::fs::OpenOptionsExt;
                opts.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
            }
            let file = parent.open_with(name, &opts).map_err(|_| "无法打开文件")?;
            if !file.metadata().map_err(|_| "无法检查文件类型")?.is_file() {
                return Err("仅支持普通文本文件".into());
            }
            let mut bytes = Vec::new();
            file.take(131073)
                .read_to_end(&mut bytes)
                .map_err(|_| "无法读取文件")?;
            if bytes.len() > 131072 || bytes.contains(&0) {
                return Err("文件过大或为二进制文件".into());
            }
            let content = String::from_utf8(bytes).map_err(|_| "仅支持 UTF-8 文本文件")?;
            Ok(Some((content, m.permissions())))
        }
    }
}
fn same_directory(a: &Dir, b: &Dir) -> Result<bool, String> {
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        let a = a.dir_metadata().map_err(|_| "无法检查目录")?;
        let b = b.dir_metadata().map_err(|_| "无法检查目录")?;
        Ok(a.dev() == b.dev() && a.ino() == b.ino())
    }
    #[cfg(windows)]
    {
        use std::{mem::zeroed, os::windows::io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut x: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
        let mut y: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
        if unsafe { GetFileInformationByHandle(a.as_raw_handle() as _, &mut x) } == 0
            || unsafe { GetFileInformationByHandle(b.as_raw_handle() as _, &mut y) } == 0
        {
            return Err("无法检查目录标识".into());
        }
        Ok((x.dwVolumeSerialNumber, x.nFileIndexHigh, x.nFileIndexLow)
            == (y.dwVolumeSerialNumber, y.nFileIndexHigh, y.nFileIndexLow))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (a, b);
        Err("此平台尚不支持写操作".into())
    }
}
pub(crate) struct Prepared {
    pub review: Value,
    action: Action,
}
enum Action {
    File {
        root: Arc<Dir>,
        parent: Dir,
        parent_path: PathBuf,
        name: PathBuf,
        before: Option<String>,
        after: String,
        permissions: Option<cap_std::fs::Permissions>,
    },
    Command {
        root: Arc<Dir>,
        cwd: Dir,
        path: PathBuf,
        command: String,
        timeout: u64,
    },
}
pub(crate) fn command_shell()->Result<String,String>{
    #[cfg(windows)] {Ok(std::env::var("SystemRoot").map_err(|_|"无法确定 Windows shell")?+"\\System32\\cmd.exe")}
    #[cfg(not(windows))] {Ok("/bin/sh".to_owned())}
}
pub(crate) fn prepare(root:Arc<Dir>,directory:&str,name:&str,args:&Value,cancel:&AtomicBool)->Result<Prepared,String>{
    prepare_with_limit(root,directory,name,args,cancel,32768)
}
pub(crate) fn prepare_editor(root:Arc<Dir>,directory:&str,args:&Value,cancel:&AtomicBool)->Result<Prepared,String>{
    prepare_with_limit(root,directory,"write_file",args,cancel,131072)
}
fn prepare_with_limit(
    root: Arc<Dir>,
    directory: &str,
    name: &str,
    args: &Value,
    cancel: &AtomicBool,
    content_limit:usize,
) -> Result<Prepared, String> {
    check(cancel)?;
    let definition = definitions()
        .into_iter()
        .find(|d| d["function"]["name"] == name)
        .ok_or("未知操作")?;
    let object = args.as_object().ok_or("工具参数必须为对象")?;
    if object.keys().any(|k| {
        definition["function"]["parameters"]["properties"]
            .get(k)
            .is_none()
    }) {
        return Err("工具参数包含未定义字段".into());
    }
    if name == "run_command" {
        let command = text(args, "command")?;
        if command.trim().is_empty() || command.len() > 8192 || command.contains('\0') {
            return Err("命令为空、超过 8192 字节或包含空字符".into());
        }
        let cwd = match args.get("cwd") {
            None => ".",
            Some(v) => v.as_str().ok_or("cwd 必须为字符串")?,
        };
        let cwd = path(cwd, true)?;
        no_links(&root, &cwd, false)?;
        let dir = root.open_dir(&cwd).map_err(|_| "命令执行目录不可访问")?;
        let timeout = match args.get("timeoutSeconds") {
            None => 60,
            Some(v) => v
                .as_u64()
                .filter(|n| (1..=120).contains(n))
                .ok_or("超时须为 1–120 秒")?,
        };
        let shell=command_shell()?;
        let review = json!({"kind":"command","command":command,"cwd":cwd,"directory":Path::new(directory).join(&cwd).components().collect::<PathBuf>().to_string_lossy(),"shell":shell,"timeoutSeconds":timeout,"scope":"Keeper 用户权限；执行目录不是文件访问沙箱"});
        return Ok(Prepared {
            review,
            action: Action::Command {
                root,
                cwd: dir,
                path: cwd,
                command: command.to_owned(),
                timeout,
            },
        });
    }
    let raw = text(args, "path")?;
    let relative = path(raw, false)?;
    no_links(&root, &relative, true)?;
    let parent_path = relative
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_owned();
    let parent = root
        .open_dir(&parent_path)
        .map_err(|_| "父目录必须已存在")?;
    let filename = PathBuf::from(relative.file_name().ok_or("无效文件名")?);
    let create_only = match args.get("createOnly") {
        None => false,
        Some(value) => value.as_bool().ok_or("createOnly 必须为布尔值")?,
    };
    if create_only && args.get("expectedSha256").is_some() {return Err("只创建文件不能同时指定已有文件版本 expectedSha256".into());}
    let old = snapshot(&parent, &filename)?;
    if create_only && old.is_some() {return Err("只创建模式：同名文件已存在，未生成预览或覆盖；请使用其他名称，或读取后明确修改已有文件。".into());}
    let before = old.as_ref().map(|(s, _)| s.as_str()).unwrap_or("");
    if let Some(value)=args.get("expectedSha256") {
        let expected=value.as_str().filter(|s|s.len()==64 && s.bytes().all(|c|c.is_ascii_hexdigit())).ok_or("expectedSha256 须为64位十六进制校验值")?;
        if old.is_none() || !hash(before.as_bytes()).eq_ignore_ascii_case(expected){return Err("文件与读取时的版本不一致，未生成修改预览或写入；请重新读取后调整修改。".into());}
    }

    let after = if name == "write_file" {
        let content = text(args, "content")?;
        if content.len() > content_limit {
            return Err(format!("写入内容最多 {} KiB",content_limit/1024));
        }
        content.to_owned()
    } else {
        if old.is_none() {
            return Err("补丁目标文件不存在".into());
        }
        let pairs:Vec<(&str,&str)>=if let Some(edits)=args.get("edits"){
            if args.get("oldText").is_some() || args.get("newText").is_some(){return Err("edits 不能与 oldText/newText 单组参数混用".into());}
            let edits=edits.as_array().filter(|v|!v.is_empty() && v.len()<=20).ok_or("edits 必须包含1–20组替换")?;
            edits.iter().map(|edit|{let object=edit.as_object().ok_or("每组替换须为对象")?;if object.keys().any(|k|k!="oldText" && k!="newText"){return Err("替换组包含未定义字段".into());}Ok((text(edit,"oldText")?,text(edit,"newText")?))}).collect::<Result<_,String>>()?
        }else{vec![(text(args,"oldText")?,text(args,"newText")?)]};
        let mut content=before.to_owned();
        for (index,(old_text,new_text)) in pairs.into_iter().enumerate(){
            check(cancel)?;
            if old_text.is_empty() || content.find(old_text).is_none() || content.find(old_text)!=content.rfind(old_text){return Err(format!("第 {} 组 oldText 必须在当前内容中唯一匹配；整次补丁未写入，请重新读取并调整。",index+1));}
            if new_text.len()>131072{return Err("替换内容超过128 KiB，整次补丁未写入".into());}
            content=content.replacen(old_text,new_text,1);
            if content.len()>131072{return Err("修改后文件最多128 KiB，整次补丁未写入".into());}
        }
        content
    };
    if after.len() > 131072 {
        return Err("修改后文件最多 128 KiB".into());
    }
    if old.is_some() && before == after {
        return Err("文件内容未变化，无需执行写入".into());
    }
    let diff = similar::TextDiff::configure()
        .timeout(Duration::from_millis(200))
        .diff_lines(before, after.as_str())
        .unified_diff()
        .context_radius(3)
        .header(if old.is_some() { raw } else { "/dev/null" }, raw)
        .to_string();
    check(cancel)?;
    let review = json!({"kind":"file","path":relative,"operation":name,"diff":diff,"beforeHash":old.as_ref().map(|(s,_)|hash(s.as_bytes())),"afterHash":hash(after.as_bytes()),"newFile":old.is_none(),"createOnly":create_only,"bytes":after.len(),"sourceHashVerified":args.get("expectedSha256").is_some()});
    Ok(Prepared {
        review,
        action: Action::File {
            root,
            parent,
            parent_path,
            name: filename,
            before: old.as_ref().map(|(s, _)| hash(s.as_bytes())),
            after,
            permissions: old.map(|(_, p)| p),
        },
    })
}
struct TempFile<'a> {
    dir: &'a Dir,
    path: PathBuf,
}
impl Drop for TempFile<'_> {
    fn drop(&mut self) {
        let _ = self.dir.remove_file(&self.path);
    }
}
impl Prepared {
    pub(crate) fn execute(
        self,
        cancel: Arc<AtomicBool>,
        mut progress: impl FnMut(String),
    ) -> Result<Value, String> {
        register(&cancel)?;
        match self.action {
            Action::File {
                root,
                parent,
                parent_path,
                name,
                before,
                after,
                permissions,
            } => {
                static WRITES: Mutex<()> = Mutex::new(());
                let _guard = loop {
                    check(&cancel)?;
                    match WRITES.try_lock() {
                        Ok(guard) => break guard,
                        Err(std::sync::TryLockError::WouldBlock) => {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Err(_) => return Err("文件写入锁异常".into()),
                    }
                };
                check(&cancel)?;
                no_links(&root, &parent_path, false)?;
                if !same_directory(
                    &parent,
                    &root.open_dir(&parent_path).map_err(|_| "父目录已变化")?,
                )? {
                    return Err("父目录已变化，请重新生成差异后确认".into());
                }
                let current = snapshot(&parent, &name)?;
                if current.as_ref().map(|(s, _)| hash(s.as_bytes())) != before {
                    return Err("文件内容已变化，拒绝覆盖；请重新读取并生成差异".into());
                }
                let temp = TempFile {
                    dir: &parent,
                    path: PathBuf::from(format!(".cangling-agent-{}.tmp", uuid::Uuid::new_v4())),
                };
                let mut opts = cap_std::fs::OpenOptions::new();
                opts.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use cap_std::fs::OpenOptionsExt;
                    opts.mode(0o600);
                }
                let mut file = parent
                    .open_with(&temp.path, &opts)
                    .map_err(|_| "无法创建临时文件")?;
                for bytes in after.as_bytes().chunks(8192) {
                    check(&cancel)?;
                    file.write_all(bytes).map_err(|_| "文件写入失败")?;
                }
                if let Some(p) = permissions {
                    file.set_permissions(p).map_err(|_| "无法保留文件权限")?;
                }
                file.sync_all().map_err(|_| "文件同步失败")?;
                drop(file);
                check(&cancel)?;
                let current = snapshot(&parent, &name)?;
                if current.as_ref().map(|(s, _)| hash(s.as_bytes())) != before {
                    return Err("文件内容已变化，拒绝覆盖".into());
                }
                if before.is_none() {
                    parent
                        .hard_link(&temp.path, &parent, &name)
                        .map_err(|_| "目标已存在或文件系统不支持安全创建，未覆盖")?;
                } else {
                    parent
                        .rename(&temp.path, &parent, &name)
                        .map_err(|_| "无法原子替换目标文件")?;
                }
                Ok(
                    json!({"applied":true,"path":self.review["path"],"bytes":after.len(),"sha256":hash(after.as_bytes())}),
                )
            }
            Action::Command {
                root,
                cwd,
                path,
                command,
                timeout,
            } => {
                no_links(&root, &path, false)?;
                if !same_directory(&cwd, &root.open_dir(&path).map_err(|_| "执行目录已变化")?)?
                {
                    return Err("执行目录已变化，请重新确认".into());
                }
                command_run(cwd, &self.review, &command, timeout, cancel, &mut progress)
            }
        }
    }
}
#[derive(Default)]
struct Registry {
    stopping: bool,
    entries: Vec<Weak<AtomicBool>>,
}
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}
fn register(cancel: &Arc<AtomicBool>) -> Result<(), String> {
    let mut registry = registry().lock().map_err(|_| "无法注册操作")?;
    if registry.stopping {
        return Err("Keeper 正在关闭，未执行操作".into());
    }
    registry.entries.retain(|e| e.strong_count() > 0);
    registry.entries.push(Arc::downgrade(cancel));
    Ok(())
}
pub(crate) fn is_shutting_down() -> bool {
    registry().lock().map(|r| r.stopping).unwrap_or(true)
}
pub(crate) fn shutdown() {
    if let Ok(mut active) = registry().lock() {
        active.stopping = true;
        for flag in active.entries.drain(..).filter_map(|f| f.upgrade()) {
            flag.store(true, Ordering::Relaxed);
        }
    }
}
struct OwnedChild {
    child: Child,
    tree: Option<crate::sync_process::ProcessTree>,
    terminated: bool,
}
impl OwnedChild {
    fn terminate(&mut self) {
        if self.terminated {
            return;
        }
        self.terminated = true;
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        self.tree.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        self.terminate();
    }
}
// Keep both ends of each stream independently; noisy stdout cannot hide stderr.
#[derive(Default)]
struct CommandOutput {
    head: Vec<u8>,
    tail: std::collections::VecDeque<u8>,
    total: usize,
}
impl CommandOutput {
    fn push(&mut self, bytes: &[u8]) {
        self.total = self.total.saturating_add(bytes.len());
        let n = bytes.len().min(4096 - self.head.len());
        self.head.extend_from_slice(&bytes[..n]);
        for &byte in &bytes[n..] {
            if self.tail.len() == 4096 { self.tail.pop_front(); }
            self.tail.push_back(byte);
        }
    }
    fn render(&self) -> (String, usize) {
        let tail: Vec<u8> = self.tail.iter().copied().collect();
        let mut head = self.head.as_slice();
        let mut tail = tail.as_slice();
        // Bound escaped JSON too, so downstream result limits preserve the tail.
        while serde_json::to_string(&String::from_utf8_lossy(head)).unwrap().len() > 8192 {
            head = &head[..head.len().saturating_sub(256)];
        }
        while serde_json::to_string(&String::from_utf8_lossy(tail)).unwrap().len() > 8192 {
            tail = &tail[256.min(tail.len())..];
        }
        if self.total == head.len() + tail.len() {
            return (String::from_utf8_lossy(&[head, tail].concat()).into_owned(), 0);
        }
        // Avoid splitting a valid multi-byte character at the omitted region.
        if let Err(error) = std::str::from_utf8(head) {
            if error.error_len().is_none() { head = &head[..error.valid_up_to()]; }
        }
        while tail.first().is_some_and(|b| b & 0xc0 == 0x80) { tail = &tail[1..]; }
        let omitted = self.total.saturating_sub(head.len() + tail.len());
        let marker = if omitted > 0 { format!("\n[省略 {omitted} 字节输出]\n") } else { String::new() };
        (format!("{}{}{}", String::from_utf8_lossy(head), marker, String::from_utf8_lossy(tail)), omitted)
    }
}

fn command_run(
    cwd: Dir,
    review: &Value,
    command: &str,
    timeout: u64,
    cancel: Arc<AtomicBool>,
    progress: &mut impl FnMut(String),
) -> Result<Value, String> {
    check(&cancel)?;
    let mut cmd = Command::new(review["shell"].as_str().ok_or("缺少 shell")?);
    #[cfg(unix)]
    {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        cmd.args(["-c", command]).process_group(0);
        let fd = cwd.as_raw_fd();
        unsafe {
            cmd.pre_exec(move || {
                if libc::fchdir(fd) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        let _ = &cwd;
        cmd.args(["/D", "/S", "/C", command])
            .current_dir(review["directory"].as_str().ok_or("缺少执行目录")?);
    }
    cmd.env_clear();
    for name in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "SystemRoot",
        "SYSTEMROOT",
        "TEMP",
        "TMP",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "PATHEXT",
        "APPDATA",
    ] {
        if let Some(v) = std::env::var_os(name) {
            cmd.env(name, v);
        }
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (child, tree) =
        crate::sync_process::spawn(&mut cmd).map_err(|e| format!("命令启动失败：{e}"))?;
    let mut owned = OwnedChild {
        child,
        tree: Some(tree),
        terminated: false,
    };
    let stdout = owned.child.stdout.take().ok_or("无法捕获标准输出")?;
    let stderr = owned.child.stderr.take().ok_or("无法捕获标准错误")?;
    let (tx, rx) = std::sync::mpsc::sync_channel(32);
    fn reader(
        mut source: impl Read + Send + 'static,
        tx: std::sync::mpsc::SyncSender<(bool, Vec<u8>)>,
        err: bool,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match source.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send((err, buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                }
            }
        })
    }
    let out_thread = reader(stdout, tx.clone(), false);
    let err_thread = reader(stderr, tx, true);
    let start = Instant::now();
    let mut last = Instant::now();
    let mut last_progress = (0, 0);
    let mut out = CommandOutput::default();
    let mut err = CommandOutput::default();
    let mut stopped = false;
    let mut timed_out = false;
    let mut exit = None;
    loop {
        for _ in 0..32 {
            let Ok((is_err, chunk)) = rx.try_recv() else {
                break;
            };
            let dest = if is_err { &mut err } else { &mut out };
            dest.push(&chunk);
        }
        if last.elapsed() >= Duration::from_millis(100)
            && last_progress != (out.total, err.total)
        {
            last_progress = (out.total, err.total);
            progress(format!(
                "[stdout]\n{}\n[stderr]\n{}",
                out.render().0,
                err.render().0
            ));
            last = Instant::now();
        }
        if cancel.load(Ordering::Relaxed) {
            stopped = true;
            owned.terminate();
            break;
        }
        if start.elapsed() >= Duration::from_secs(timeout) {
            timed_out = true;
            owned.terminate();
            break;
        }
        match owned.child.try_wait() {
            Ok(Some(code)) => {
                exit = code.code();
                owned.terminate();
                break;
            }
            Ok(None) => {}
            Err(_) => {
                owned.terminate();
                return Err("无法获取命令状态".into());
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Killing the owned tree also closes inherited pipes. Drain before joining readers.
    while let Ok((is_err, chunk)) = rx.recv_timeout(Duration::from_millis(100)) {
        let dest = if is_err { &mut err } else { &mut out };
        dest.push(&chunk);
    }
    let _ = out_thread.join();
    let _ = err_thread.join();
    let (stdout, stdout_omitted) = out.render();
    let (stderr, stderr_omitted) = err.render();
    let result = json!({"exitCode":exit,"stdout":stdout,"stderr":stderr,"stopped":stopped,"timedOut":timed_out,"outputTruncated":stdout_omitted + stderr_omitted > 0,"stdoutOmittedBytes":stdout_omitted,"stderrOmittedBytes":stderr_omitted});
    progress(serde_json::to_string_pretty(&result).unwrap_or_default());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, Arc<Dir>) {
        let path = std::env::temp_dir().join(format!("keeper-actions-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(path.join("sub")).unwrap();
        let root = Arc::new(Dir::open_ambient_dir(&path, cap_std::ambient_authority()).unwrap());
        (path, root)
    }
    fn flag() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }
    #[test]
    fn multiple_edits_prepare_once_and_never_partially_write(){
        let (path,root)=fixture();std::fs::write(path.join("multi.txt"),"first\nsecond\n").unwrap();
        let args=json!({"path":"multi.txt","edits":[{"oldText":"first","newText":"ONE"},{"oldText":"second","newText":"TWO"}],"expectedSha256":hash(b"first\nsecond\n")});
        let plan=prepare(root.clone(),path.to_str().unwrap(),"apply_patch",&args,&flag()).unwrap();assert!(plan.review["diff"].as_str().unwrap().contains("+ONE"));assert!(plan.review["diff"].as_str().unwrap().contains("+TWO"));assert_eq!(std::fs::read_to_string(path.join("multi.txt")).unwrap(),"first\nsecond\n");plan.execute(flag(), |_|{}).unwrap();
        assert_eq!(std::fs::read_to_string(path.join("multi.txt")).unwrap(),"ONE\nTWO\n");
        let invalid=json!({"path":"multi.txt","edits":[{"oldText":"ONE","newText":"changed"},{"oldText":"missing","newText":"bad"}]});assert!(prepare(root.clone(),path.to_str().unwrap(),"apply_patch",&invalid,&flag()).is_err());assert_eq!(std::fs::read_to_string(path.join("multi.txt")).unwrap(),"ONE\nTWO\n");
        for args in [json!({"path":"multi.txt","edits":[]}),json!({"path":"multi.txt","edits":[{"oldText":"ONE","newText":"x"}],"oldText":"ONE","newText":"x"})]{assert!(prepare(root.clone(),path.to_str().unwrap(),"apply_patch",&args,&flag()).is_err());}
        drop(root);std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn create_only_never_overwrites_existing_or_newly_appearing_file() {
        let (path,root)=fixture();
        let args=json!({"path":"new.txt","content":"new content","createOnly":true});
        let plan=prepare(root.clone(),path.to_str().unwrap(),"write_file",&args,&flag()).unwrap();
        assert_eq!(plan.review["createOnly"],true);assert_eq!(plan.review["newFile"],true);
        std::fs::write(path.join("new.txt"),"another writer").unwrap();
        assert!(plan.execute(flag(), |_|{}).is_err());
        assert_eq!(std::fs::read_to_string(path.join("new.txt")).unwrap(),"another writer");
        assert!(prepare(root.clone(),path.to_str().unwrap(),"write_file",&args,&flag()).is_err());
        let mut invalid=args.clone();invalid["createOnly"]=json!("true");assert!(prepare(root.clone(),path.to_str().unwrap(),"write_file",&invalid,&flag()).is_err());
        invalid=args.clone();invalid["expectedSha256"]=json!(hash(b"another writer"));assert!(prepare(root.clone(),path.to_str().unwrap(),"write_file",&invalid,&flag()).is_err());
        std::fs::remove_file(path.join("new.txt")).unwrap();
        let plan=prepare(root.clone(),path.to_str().unwrap(),"write_file",&args,&flag()).unwrap();plan.execute(flag(), |_|{}).unwrap();
        assert_eq!(std::fs::read_to_string(path.join("new.txt")).unwrap(),"new content");
        drop(root);std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn read_revision_checked_before_write_or_patch_preview(){
        let (path,root)=fixture();std::fs::write(path.join("version.txt"),"before").unwrap();let digest=hash(b"before");
        for (name,mut args) in [("write_file",json!({"path":"version.txt","content":"after"})),("apply_patch",json!({"path":"version.txt","oldText":"before","newText":"after"}))]{
            args["expectedSha256"]=json!(digest.to_uppercase());let plan=prepare(root.clone(),path.to_str().unwrap(),name,&args,&flag()).unwrap();assert_eq!(plan.review["sourceHashVerified"],true);drop(plan);
            args["expectedSha256"]=json!(hash(b"stale"));assert!(prepare(root.clone(),path.to_str().unwrap(),name,&args,&flag()).is_err());
            assert_eq!(std::fs::read_to_string(path.join("version.txt")).unwrap(),"before");
        }
        assert!(prepare(root.clone(),path.to_str().unwrap(),"write_file",&json!({"path":"absent","content":"x","expectedSha256":hash(b"")}),&flag()).is_err());
        drop(root);std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn preparation_is_read_only_and_commits_exact_reviewed_content() {
        let (path, root) = fixture();
        let prepared = prepare(
            root.clone(),
            path.to_str().unwrap(),
            "write_file",
            &json!({"path":"hello.txt","content":"hello\n"}),
            &flag(),
        )
        .unwrap();
        assert!(!path.join("hello.txt").exists());
        assert!(prepared.review["diff"].as_str().unwrap().contains("+hello"));
        assert_eq!(prepared.execute(flag(), |_| {}).unwrap()["applied"], true);
        assert_eq!(
            std::fs::read_to_string(path.join("hello.txt")).unwrap(),
            "hello\n"
        );
        let patch = prepare(
            root.clone(),
            path.to_str().unwrap(),
            "apply_patch",
            &json!({"path":"hello.txt","oldText":"hello","newText":"updated"}),
            &flag(),
        )
        .unwrap();
        assert!(patch.review["diff"].as_str().unwrap().contains("-hello"));
        patch.execute(flag(), |_| {}).unwrap();
        assert_eq!(
            std::fs::read_to_string(path.join("hello.txt")).unwrap(),
            "updated\n"
        );
        drop(root);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn conflict_cancellation_and_parent_replacement_do_not_overwrite() {
        let (path, root) = fixture();
        std::fs::write(path.join("hello.txt"), "before").unwrap();
        let plan = prepare(
            root.clone(),
            "fixture",
            "write_file",
            &json!({"path":"hello.txt","content":"after"}),
            &flag(),
        )
        .unwrap();
        std::fs::write(path.join("hello.txt"), "user edit").unwrap();
        assert!(plan.execute(flag(), |_| {}).is_err());
        assert_eq!(
            std::fs::read_to_string(path.join("hello.txt")).unwrap(),
            "user edit"
        );
        let plan = prepare(
            root.clone(),
            "fixture",
            "write_file",
            &json!({"path":"new.txt","content":"new"}),
            &flag(),
        )
        .unwrap();
        assert!(
            plan.execute(Arc::new(AtomicBool::new(true)), |_| {})
                .is_err()
        );
        assert!(!path.join("new.txt").exists());
        let plan = prepare(
            root.clone(),
            "fixture",
            "write_file",
            &json!({"path":"sub/new.txt","content":"new"}),
            &flag(),
        )
        .unwrap();
        std::fs::rename(path.join("sub"), path.join("moved")).unwrap();
        std::fs::create_dir(path.join("sub")).unwrap();
        assert!(plan.execute(flag(), |_| {}).is_err());
        assert!(!path.join("moved/new.txt").exists());
        assert!(!path.join("sub/new.txt").exists());
        drop(root);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn invalid_patches_and_outside_paths_never_prepare() {
        let (path, root) = fixture();
        std::fs::write(path.join("repeat.txt"), "same same").unwrap();
        assert!(
            prepare(
                root.clone(),
                "fixture",
                "apply_patch",
                &json!({"path":"repeat.txt","oldText":"same","newText":"other"}),
                &flag()
            )
            .is_err()
        );
        for value in ["../outside", "/tmp/outside", ".env", "sub/../outside"] {
            assert!(
                prepare(
                    root.clone(),
                    "fixture",
                    "write_file",
                    &json!({"path":value,"content":"bad"}),
                    &flag()
                )
                .is_err()
            );
        }
        assert!(!path.join("outside").exists());
        drop(root);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn overlapping_patch_and_concurrent_write_conflicts_are_rejected() {
        let (path, root) = fixture();
        std::fs::write(path.join("overlap.txt"), "aaa").unwrap();
        assert!(
            prepare(
                root.clone(),
                "fixture",
                "apply_patch",
                &json!({"path":"overlap.txt","oldText":"aa","newText":"b"}),
                &flag()
            )
            .is_err()
        );
        std::fs::write(path.join("same.txt"), "base").unwrap();
        let first = prepare(
            root.clone(),
            "fixture",
            "write_file",
            &json!({"path":"same.txt","content":"first"}),
            &flag(),
        )
        .unwrap();
        let second = prepare(
            root.clone(),
            "fixture",
            "write_file",
            &json!({"path":"same.txt","content":"second"}),
            &flag(),
        )
        .unwrap();
        let a = std::thread::spawn(move || first.execute(flag(), |_| {}));
        let b = std::thread::spawn(move || second.execute(flag(), |_| {}));
        assert_eq!(
            [a.join().unwrap(), b.join().unwrap()]
                .iter()
                .filter(|r| r.is_ok())
                .count(),
            1
        );
        let content = std::fs::read_to_string(path.join("same.txt")).unwrap();
        assert!(content == "first" || content == "second");
        assert!(!std::fs::read_dir(&path).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".cangling-agent-")
        }));
        drop(root);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn command_output_keeps_both_ends_and_bounds_escaped_json() {
        let mut output = CommandOutput::default();
        output.push(b"BEGIN"); output.push(&vec![b'x'; 40000]); output.push(b"END");
        let (text, omitted) = output.render();
        assert!(text.starts_with("BEGIN") && text.ends_with("END"));
        assert_eq!(omitted, 40008 - 8192);
        let mut escaped = CommandOutput::default();
        escaped.push(&vec![0; 20000]); escaped.push(b"FINAL ERROR");
        let (text, omitted) = escaped.render();
        assert!(text.ends_with("FINAL ERROR") && omitted > 0);
        let mut value = json!({"stdout":text,"stderr":text,"outputTruncated":true});
        let before = value["stderr"].clone();
        crate::agent_tool_result::annotate("run_command", &mut value, "succeeded");
        assert_eq!(value["stderr"], before);
        assert!(value.to_string().len() < 46000);
        for n in [2000, 10000] {
            let input = "中文".repeat(n);
            let mut output = CommandOutput::default();
            for bytes in input.as_bytes().chunks(17) { output.push(bytes); }
            let (text, omitted) = output.render();
            assert!(!text.contains('�'));
            assert_eq!(omitted, input.len() - 8190);
        }
        let input = "中".repeat(2000);
        let mut output = CommandOutput::default(); output.push(input.as_bytes());
        assert_eq!(output.render(), (input, 0));
    }

    #[cfg(unix)]
    #[test]
    fn command_captures_both_streams_and_times_out_tree() {
        let (path, root) = fixture();
        let plan = prepare(
            root.clone(),
            path.to_str().unwrap(),
            "run_command",
            &json!({"command":"printf out; printf err >&2","timeoutSeconds":5}),
            &flag(),
        )
        .unwrap();
        let result = plan.execute(flag(), |_| {}).unwrap();
        assert_eq!(result["exitCode"], 0);
        assert_eq!(result["stdout"], "out");
        assert_eq!(result["stderr"], "err");
        let plan = prepare(
            root.clone(),
            path.to_str().unwrap(),
            "run_command",
            &json!({"command":"sleep 10; printf bad > late.txt","timeoutSeconds":1}),
            &flag(),
        )
        .unwrap();
        let start = Instant::now();
        let result = plan.execute(flag(), |_| {}).unwrap();
        assert_eq!(result["timedOut"], true);
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(!path.join("late.txt").exists());
        drop(root);
        std::fs::remove_dir_all(path).unwrap();
    }
}

/// Explicit UI creation: a single name under an existing, capability-scoped parent.
pub(crate) fn create_entry(root:&Dir,parent:&str,name:&str,directory:bool)->Result<(),String>{
    if name.is_empty() || name.trim()!=name || name.len()>255 || name.contains('/') || name.contains('\\') || name=="." || name==".." || name.ends_with('.') {return Err("请输入有效名称（不能包含路径分隔符、首尾空格或末尾点）".into());}
    let leaf=path(name,false)?;let parent_path=path(parent,true)?;
    no_links(root,&parent_path,false)?;let parent=root.open_dir(&parent_path).map_err(|_|"父目录不可访问")?;
    let result=if directory {parent.create_dir(&leaf)}else{
        let mut options=cap_std::fs::OpenOptions::new();options.write(true).create_new(true);
        #[cfg(unix)] {use cap_std::fs::OpenOptionsExt;options.mode(0o600);}
        parent.open_with(&leaf,&options).map(|_|())
    };
    result.map_err(|e|if e.kind()==std::io::ErrorKind::AlreadyExists{"同名文件或目录已存在，未覆盖".into()}else{format!("创建失败：{e}")})
}
