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
 ("apply_patch","同源错误批量替换：先检查所有匹配位置，使用oldText/newText/expectedOccurrences以及expectedSha256，一次替换全部精确匹配；匹配次数必须与expectedOccurrences一致。不确定各处语义一致时使用edits逐组替换。支持行补丁：startLine/endLine（含首尾、最多200行）、newLines（新行数组，空数组删除），必须提供expectedSha256，不可与文本替换混用。多行文本转义困难时优先使用行补丁。精确替换文件，优先使用简单参数：path、oldText、newText、expectedSha256。newText 为空字符串表示删除匹配文本。仅多处修改时使用 edits 数组（1–20组），不能与单组参数混用。各组均匹配同一原始文件，禁止重叠、嵌套或依赖前一组的新文本；邻近修改合并成一组。oldText使用最小唯一片段，不要包含大片未改内容；任何一组失败均不写入，全部成功后只生成一份审批差异并写入一次。由系统安全策略决定自动批准或等待用户确认；应传入 read_file 的 sha256 作为 expectedSha256，拒绝基于旧版本修改。",json!({"path":{"type":"string"},"startLine":{"type":"integer","minimum":1},"endLine":{"type":"integer","minimum":1},"newLines":{"type":"array","maxItems":200,"items":{"type":"string"}},"oldText":{"type":"string"},"newText":{"type":"string"},"expectedOccurrences":{"type":"integer","minimum":1,"maximum":1000},"edits":{"type":"array","minItems":1,"maxItems":20,"items":{"type":"object","properties":{"oldText":{"type":"string","minLength":1},"newText":{"type":"string"}},"required":["oldText","newText"],"additionalProperties":false}},"expectedSha256":{"type":"string","minLength":64,"maxLength":64}}),json!(["path"])),
 ("run_tests","运行 Python unittest 测试，优先于手写测试 Shell。directory 默认 tests，pattern 默认 test*.py。生成完整命令供原安全策略审批；测试代码具有当前用户权限，并非沙箱。日志保存为项目根目录的 keeper-test-*.log（每流最多2 MiB），返回结构化摘要。测试结束清理所属进程组；测试通过不等于全部需求已验收。",json!({"directory":{"type":"string","maxLength":200},"pattern":{"type":"string","maxLength":100},"timeoutSeconds":{"type":"integer","minimum":1,"maximum":120}}),json!([])),
 ("run_command","提出本机 shell 命令，默认在当前项目根目录运行。Python unittest 使用 run_tests；不得以 head/tail 截断测试进程输出，查看已保存的日志不受影响。系统按安全模式决定自动批准或等待用户确认完整命令和执行目录。以 Keeper 用户权限执行，不是文件沙箱。timeoutSeconds 默认 60，最多 120。命令结束会清理所属进程组，后台服务不能跨调用保活；测试服务器与请求应在同一命令中完成，并在 finally 中关闭。不猜端口、不使用宽泛 pkill。Bash 默认 pipefail；exitCode=0 仅代表命令成功，不代表测试全部通过，须核查测试数量、失败、跳过和截断情况。",json!({"command":{"type":"string"},"cwd":{"type":"string"},"timeoutSeconds":{"type":"integer","minimum":1,"maximum":120}}),json!(["command"])),
].into_iter().map(|(name,description,properties,required)|json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})).collect()
}
pub(crate) fn is_action(name: &str) -> bool {
    matches!(name, "write_file" | "apply_patch" | "run_command" | "run_tests")
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
static WRITES: Mutex<()> = Mutex::new(());
pub(crate) struct Prepared {
    pub review: Value,
    action: Action,
}
enum Action {
    Unchanged(Value),
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
    #[cfg(not(windows))] {Ok(if Path::new("/bin/bash").is_file(){"/bin/bash"}else{"/bin/sh"}.to_owned())}
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
    if name == "run_tests" {
        let directory_arg=args.get("directory").and_then(Value::as_str).unwrap_or("tests");
        let pattern=args.get("pattern").and_then(Value::as_str).unwrap_or("test*.py");
        // Restrict syntax rather than interpolate arbitrary shell fragments.
        if directory_arg.is_empty() || directory_arg.len()>200 || directory_arg.starts_with('-') || !directory_arg.bytes().all(|b|b.is_ascii_alphanumeric()||b"_./-".contains(&b)) {return Err("测试目录须为项目相对路径，仅使用字母数字和 _./-".into());}
        let directory_path=path(directory_arg,true)?;no_links(&root,&directory_path,false)?;
        root.open_dir(&directory_path).map_err(|_|"测试目录不存在，请先创建目录及测试文件")?;
        if pattern.is_empty() || pattern.len()>100 || !pattern.bytes().all(|b|b.is_ascii_alphanumeric()||b"_.*?-".contains(&b)){return Err("测试文件模式仅允许字母数字和 _.*?-".into());}
        let python=if cfg!(windows){"python"}else{"python3"};
        let command=format!(r#"{python} -m unittest discover -s "{directory_arg}" -p "{pattern}" -v"#);
        let mut request=json!({"command":command,"timeoutSeconds":args.get("timeoutSeconds").cloned().unwrap_or(json!(120))});
        request["cwd"]=json!(".");
        let mut prepared=prepare_with_limit(root,directory,"run_command",&request,cancel,content_limit)?;
        prepared.review["testRunner"]=json!("unittest");
        prepared.review["testId"]=json!(format!("unittest:{directory_arg}:{pattern}"));
        prepared.review["testLogPrefix"]=json!(format!("keeper-test-{}",uuid::Uuid::new_v4()));
        prepared.review["testLogLimitBytes"]=json!(2097152);
        return Ok(prepared);
    }
    if name == "run_command" {
        let command = text(args, "command")?;
        if crate::agent_test_summary::truncates_test_output(command){return Err("未执行：head/tail 会截断测试汇总或提前关闭输出管道。请使用 run_tests 保存完整日志，或提交不截断的测试命令重新审批；不要原样重试。".into());}
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
        let mut review = json!({"kind":"command","command":command,"cwd":cwd,"directory":Path::new(directory).join(&cwd).components().collect::<PathBuf>().to_string_lossy(),"shell":shell,"timeoutSeconds":timeout,"pipelineFailureMode":if shell=="/bin/bash"{"pipefail"}else{"shell-default"},"processLifetime":"命令结束、超时或停止时清理所属进程组；后台服务不能跨工具调用存活","scope":"Keeper 用户权限；执行目录不是文件访问沙箱"});
        // Preserve the exact reviewed command; attach log capture to canonical tests.
        if cwd==Path::new(".") {if let Some(id)=crate::agent_test_summary::identity(command){
            review["testRunner"]=json!("unittest");review["testId"]=json!(id);
            review["testLogPrefix"]=json!(format!("keeper-test-{}",uuid::Uuid::new_v4()));
            review["testLogLimitBytes"]=json!(2097152);
        }}
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
    if create_only && old.is_some() {return Err("只创建模式：同名文件已存在，未生成预览或覆盖；如需修改此文件，请先read_file读取path和sha256，再用write_file(path,content,createOnly=false,expectedSha256=读取的sha256)或apply_patch；不要继续createOnly=true重试。".into());}
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
    } else if ["startLine","endLine","newLines"].iter().any(|k|args.get(k).is_some()) {
        if old.is_none() {return Err("补丁目标文件不存在".into());}
        patch_lines(before,args)?
    } else {
        if old.is_none() {
            return Err("补丁目标文件不存在".into());
        }
        let expected_occurrences=args.get("expectedOccurrences").map(|v|v.as_u64().filter(|n|*n>=1&&*n<=1000).ok_or("expectedOccurrences须为1–1000的整数")).transpose()?;
        if expected_occurrences.is_some() && (args.get("edits").is_some() || args.get("expectedSha256").is_none()){return Err("计数替换必须提供expectedSha256，不能与edits混用".into());}
        let pairs:Vec<(&str,&str)>=if let Some(edits)=args.get("edits"){
            if args.get("oldText").is_some() || args.get("newText").is_some(){return Err("edits 不能与 oldText/newText 单组参数混用".into());}
            let edits=edits.as_array().filter(|v|!v.is_empty() && v.len()<=20).ok_or("edits 必须包含1–20组替换")?;
            edits.iter().map(|edit|{let object=edit.as_object().ok_or("每组替换须为对象")?;if object.keys().any(|k|k!="oldText" && k!="newText"){return Err("替换组包含未定义字段".into());}Ok((text(edit,"oldText")?,text(edit,"newText")?))}).collect::<Result<_,String>>()?
        }else{vec![(text(args,"oldText")?,text(args,"newText")?)]};
        if args.get("edits").is_some(){patch_original(before,&pairs,cancel)?}else{
        let mut content=before.to_owned();
        for (index,(old_text,new_text)) in pairs.into_iter().enumerate(){
            check(cancel)?;
            let occurrences=if old_text.is_empty(){0}else{content.match_indices(old_text).count()};
            let overlapping=if old_text.is_empty(){0}else{content.char_indices().filter(|(offset,_)|content[*offset..].starts_with(old_text)).take(1001).count()};
            if old_text.is_empty() || overlapping!=occurrences || occurrences != expected_occurrences.unwrap_or(1) as usize {
                let anchor=old_text.lines().find(|s|!s.trim().is_empty()).unwrap_or("").chars().take(160).collect::<String>();
                return Err(format!("第 {} 组 oldText 匹配次数不符（实际{}，要求{}）；整次补丁未写入。不要猜测或模糊替换，不要重复全文件定位。可读取 {}，复制实际文本作为oldText（不带行号，JSON中只转义一次）；保留expectedSha256。",index+1,occurrences,expected_occurrences.unwrap_or(1),json!({"path":raw,"anchor":anchor,"lineCount":80,"expectedSha256":hash(before.as_bytes())})));
            }
            if new_text.len()>131072{return Err("替换内容超过128 KiB，整次补丁未写入".into());}
            let count=expected_occurrences.unwrap_or(1) as usize;
            let projected=content.len().saturating_sub(old_text.len().saturating_mul(count)).saturating_add(new_text.len().saturating_mul(count));
            if projected>131072{return Err("修改后文件最多128 KiB，整次补丁未写入".into());}
            content=content.replacen(old_text,new_text,count);
            if content.len()>131072{return Err("修改后文件最多128 KiB，整次补丁未写入".into());}
        }
        content
        }
    };
    if after.len() > 131072 {
        return Err("修改后文件最多 128 KiB".into());
    }
    if old.is_some() && before == after {
        return Ok(Prepared{review:json!({"kind":"unchanged","path":raw}),action:Action::Unchanged(json!({"path":raw,"changed":false,"applied":false,"notExecuted":true,"unchanged":true,"sha256":hash(before.as_bytes()),"message":"内容已与目标一致，无需写入或审批；继续验证下一项，不要重复提交相同修改。"}))});
    }
    let text_diff = similar::TextDiff::configure()
        .timeout(Duration::from_millis(200))
        .diff_lines(before, after.as_str());
    let mut added_lines=0usize;
    let mut removed_lines=0usize;
    for change in text_diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => added_lines+=1,
            similar::ChangeTag::Delete => removed_lines+=1,
            similar::ChangeTag::Equal => {},
        }
    }
    let change_summary=json!({"addedLines":added_lines,"removedLines":removed_lines});
    let diff = text_diff.unified_diff()
        .context_radius(3)
        .header(if old.is_some() { raw } else { "/dev/null" }, raw)
        .to_string();
    check(cancel)?;
    let review = json!({"kind":"file","path":relative,"operation":name,"diff":diff,"beforeHash":old.as_ref().map(|(s,_)|hash(s.as_bytes())),"afterHash":hash(after.as_bytes()),"newFile":old.is_none(),"createOnly":create_only,"changeSummary":change_summary,"bytes":after.len(),"sourceHashVerified":args.get("expectedSha256").is_some(),"rollbackVersion":1,"beforeContent":old.as_ref().map(|(text,_)|text)});
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
/// Resolve every edit against the same snapshot before producing any output.
fn patch_original(before:&str,pairs:&[(&str,&str)],cancel:&AtomicBool)->Result<String,String>{
    let mut spans=Vec::new();let mut bytes=before.len();
    for (index,(old,new)) in pairs.iter().enumerate(){
        check(cancel)?;
        let start=before.find(old).filter(|_|!old.is_empty()).ok_or_else(||format!("第{}组在原始文件中未匹配；整次补丁未写入。每组oldText必须来自同一原始文件，不能引用前一组产生的新文本。",index+1))?;
        if before.rfind(old)!=Some(start){return Err(format!("第{}组在原始文件中不是唯一匹配，整次补丁未写入。",index+1));}
        spans.push((start,start+old.len(),*new));
    }
    spans.sort_by_key(|v|v.0);
    if spans.windows(2).any(|w|w[0].1>w[1].0){return Err("补丁范围重叠或嵌套，请合并为一组；整次补丁未写入".into());}
    for (start,end,_) in &spans {bytes-=end-start;}
    for (_,_,new) in &spans {bytes=bytes.saturating_add(new.len());}
    if bytes>131072{return Err("修改后文件最多128 KiB，整次补丁未写入".into());}
    let mut result=String::with_capacity(bytes);let mut cursor=0;
    for (start,end,new) in spans {result.push_str(&before[cursor..start]);result.push_str(new);cursor=end;}
    result.push_str(&before[cursor..]);Ok(result)
}
#[cfg(test)]mod original_patch_tests{
 use super::*;
 #[test]fn all_edits_use_original_snapshot_and_reject_overlap(){
  let flag=AtomicBool::new(false);
  assert_eq!(patch_original("first second",&[("first","second"),("second","third")],&flag).unwrap(),"second third");
  assert!(patch_original("first",&[("first","second"),("second","third")],&flag).is_err());
  assert!(patch_original("abc",&[("ab","x"),("bc","y")],&flag).is_err());
  assert!(patch_original("aaa",&[("aa","x")],&flag).is_err());
  assert_eq!(patch_original("甲乙",&[("乙","B"),("甲","A")],&flag).unwrap(),"AB");
  assert!(patch_original("a",&[("a","b")],&AtomicBool::new(true)).is_err());
 }
}
fn patch_lines(before: &str, args: &Value) -> Result<String,String> {
    if ["oldText","newText","edits","expectedOccurrences"].iter().any(|k|args.get(k).is_some()) {return Err("行补丁不能与文本补丁混用".into());}
    let expected=text(args,"expectedSha256")?;
    if !hash(before.as_bytes()).eq_ignore_ascii_case(expected) {return Err("文件版本已变化，请重新读取".into());}
    let start=args["startLine"].as_u64().ok_or("startLine 必须为正整数")?;
    let end=args["endLine"].as_u64().ok_or("endLine 必须为正整数")?;
    let lines:Vec<&str>=before.split_inclusive('\n').collect();
    if start==0 || end<start || end>lines.len() as u64 || end-start>=200 {return Err("行范围无效，最多替换200行".into());}
    let new=args["newLines"].as_array().filter(|a|a.len()<=200).ok_or("newLines 须为最多200行的数组")?;
    let values=new.iter().map(|v|v.as_str().filter(|s|!s.contains(['\n','\r'])).ok_or("每个newLines元素只能包含一行".to_string())).collect::<Result<Vec<_>,_>>()?;
    let sep=if lines.iter().filter(|s|s.ends_with('\n')).all(|s|s.ends_with("\r\n")) && before.contains("\r\n") {"\r\n"} else {"\n"};
    let mut replacement=values.join(sep);
    if !values.is_empty() && lines[end as usize-1].ends_with('\n') {replacement.push_str(sep);}
    Ok(format!("{}{}{}",lines[..start as usize-1].concat(),replacement,lines[end as usize..].concat()))
}
#[cfg(test)]
mod line_patch_tests {
 use super::*;
 #[test] fn preserves_endings_and_checks_version_and_range() {
  for before in ["a\nb\nc\n","a\r\nb\r\nc\r\n","a\nb\nc"] {
   let mut a=json!({"startLine":2,"endLine":2,"newLines":["B"],"expectedSha256":hash(before.as_bytes())});
   assert_eq!(patch_lines(before,&a).unwrap(),before.replace('b',"B"));
   a["newLines"]=json!([]);assert_eq!(patch_lines(before,&a).unwrap(),before.replace(if before.contains("\r\n"){"b\r\n"}else{"b\n"},""));
   a["endLine"]=json!(100);assert!(patch_lines(before,&a).is_err());
  }
  let a=json!({"startLine":1,"endLine":1,"newLines":["x"],"expectedSha256":hash(b"a")});
  assert_eq!(patch_lines("a",&a).unwrap(),"x");assert!(patch_lines("changed",&a).is_err());
  for extra in [json!({"newLines":["x\ny"]}),json!({"oldText":"a"}),json!({"expectedSha256":null})] {let mut b=a.clone();b.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());assert!(patch_lines("a",&b).is_err());}
 }
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
    pub(crate) fn unchanged(&self)->Option<Value>{if let Action::Unchanged(value)=&self.action{Some(value.clone())}else{None}}
    pub(crate) fn execute(
        self,
        cancel: Arc<AtomicBool>,
        mut progress: impl FnMut(String),
    ) -> Result<Value, String> {
        register(&cancel)?;
        match self.action {
            Action::Unchanged(value)=>Ok(value),
            Action::File {
                root,
                parent,
                parent_path,
                name,
                before,
                after,
                permissions,
            } => {

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
                    json!({"applied":true,"path":self.review["path"],"bytes":after.len(),"sha256":hash(after.as_bytes()),"previousSha256":self.review["beforeHash"],"created":self.review["newFile"],"changeSummary":self.review["changeSummary"]}),
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
    full: Option<Vec<u8>>,
}
impl CommandOutput {
    fn push(&mut self, bytes: &[u8]) {
        if let Some(full)=self.full.as_mut(){let n=bytes.len().min(2097152usize.saturating_sub(full.len()));full.extend_from_slice(&bytes[..n]);}
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
        if review["shell"].as_str()==Some("/bin/bash"){cmd.args(["-o","pipefail"]);}
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
    if review["testRunner"]=="unittest"{out.full=Some(Vec::new());err.full=Some(Vec::new());}
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
    let mut result = json!({"pipelineFailureMode":review["pipelineFailureMode"],"testVerdict":"not_assessed","testSummary":crate::agent_test_summary::summarize(&stdout,&stderr,exit,stopped||timed_out||stdout_omitted+stderr_omitted>0),"processCleanup":{"scope":"owned_process_group_or_job","attempted":owned.terminated,"guidance":"本次命令结束时已尝试清理所属进程组/Job；不支持跨调用保活服务。测试服务与请求应在同一命令中启动、使用并关闭；不要猜端口或使用宽泛 pkill。脱离所属进程组的进程不在此保证内。"},"exitCode":exit,"stdout":stdout,"stderr":stderr,"stopped":stopped,"timedOut":timed_out,"outputTruncated":stdout_omitted + stderr_omitted > 0,"stdoutOmittedBytes":stdout_omitted,"stderrOmittedBytes":stderr_omitted});
    if result["testSummary"]["format"]=="unittest" {if let Some(id)=crate::agent_test_summary::identity(command){result["testId"]=json!(id);}}
    if review["testRunner"]=="unittest" {
        let full_out=out.full.as_deref().unwrap_or_default();let full_err=err.full.as_deref().unwrap_or_default();
        let incomplete=out.total>full_out.len()||err.total>full_err.len();
        result["testSummary"]=crate::agent_test_summary::summarize(&String::from_utf8_lossy(full_out),&String::from_utf8_lossy(full_err),exit,stopped||timed_out||incomplete);
        result["testId"]=review["testId"].clone();
        let prefix=review["testLogPrefix"].as_str().ok_or("缺少测试日志标识")?;
        let mut logs=Vec::new();
        for (suffix,bytes) in [("stdout",full_out),("stderr",full_err)] {
            let name=format!("{prefix}-{suffix}.log");
            let mut options=cap_std::fs::OpenOptions::new();options.write(true).create_new(true);
            match cwd.open_with(&name,&options).and_then(|mut f|f.write_all(bytes)) {
                Ok(())=>logs.push(json!({"path":name,"bytes":bytes.len(),"truncated":if suffix=="stdout"{out.total>bytes.len()}else{err.total>bytes.len()}})),
                Err(_)=>{result["testLogError"]=json!("日志保存失败；未覆盖已有文件，请查看执行输出");}
            }
        }
        result["testLogs"]=json!(logs);
    }
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
    fn counted_patch_is_atomic_reviewed_and_version_guarded(){
        let (dir,root)=fixture();let file=dir.join("a.py");let before="a = self.db.get()\nb = self.db.put()\n";std::fs::write(&file,before).unwrap();
        let args=json!({"path":"a.py","oldText":"self.db","newText":"self.server.db","expectedOccurrences":2,"expectedSha256":hash(before.as_bytes())});
        crate::agent_tool_args::validate("apply_patch",&args).unwrap();
        for bad in [json!({"expectedOccurrences":3}),json!({"expectedSha256":"0".repeat(64)}),json!({"edits":[{"oldText":"a","newText":"b"}]})] {let mut a=args.clone();a.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());assert!(prepare(root.clone(),dir.to_str().unwrap(),"apply_patch",&a,&flag()).is_err());assert_eq!(std::fs::read_to_string(&file).unwrap(),before);}
        let mut missing=args.clone();missing.as_object_mut().unwrap().remove("expectedSha256");assert!(prepare(root.clone(),dir.to_str().unwrap(),"apply_patch",&missing,&flag()).is_err());
        let p=prepare(root.clone(),dir.to_str().unwrap(),"apply_patch",&args,&flag()).unwrap();assert_eq!(p.review["changeSummary"]["addedLines"],2);assert_eq!(std::fs::read_to_string(&file).unwrap(),before);
        p.execute(flag(),|_|{}).unwrap();assert_eq!(std::fs::read_to_string(&file).unwrap(),before.replace("self.db","self.server.db"));
        std::fs::write(&file,before).unwrap();let p=prepare(root.clone(),dir.to_str().unwrap(),"apply_patch",&args,&flag()).unwrap();std::fs::write(&file,"user edit").unwrap();assert!(p.execute(flag(),|_|{}).is_err());assert_eq!(std::fs::read_to_string(&file).unwrap(),"user edit");
        drop(root);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn line_patch_review_never_writes_and_rechecks_concurrent_changes(){
        let (dir,root)=fixture();let file=dir.join("a.txt");std::fs::write(&file,"a\nb\n").unwrap();
        let args=json!({"path":"a.txt","startLine":2,"endLine":2,"newLines":["B"],"expectedSha256":hash(b"a\nb\n")});
        crate::agent_tool_args::validate("apply_patch",&args).unwrap();
        let p=prepare(root.clone(),dir.to_str().unwrap(),"apply_patch",&args,&flag()).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(),"a\nb\n");assert!(p.review["diff"].as_str().unwrap().contains("+B"));
        std::fs::write(&file,"user edit").unwrap();assert!(p.execute(flag(),|_|{}).is_err());assert_eq!(std::fs::read_to_string(&file).unwrap(),"user edit");
        drop(root);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn canonical_command_keeps_review_and_logs_while_truncation_does_not_execute(){
        let (dir,root)=fixture();let command="python3 -m unittest discover -s sub -v";
        let prepared=prepare(root.clone(),dir.to_str().unwrap(),"run_command",&json!({"command":command}),&flag()).unwrap();
        assert_eq!(prepared.review["command"],command);assert_eq!(prepared.review["testRunner"],"unittest");assert!(prepared.review["testLogPrefix"].as_str().unwrap().starts_with("keeper-test-"));
        assert!(prepare(root.clone(),dir.to_str().unwrap(),"run_command",&json!({"command":format!("{command} | head -100")}),&flag()).err().unwrap().contains("未执行"));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(),1);drop(prepared);drop(root);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn unified_test_runner_is_reviewed_and_rejects_injection(){
        let (dir,root)=fixture();std::fs::create_dir(dir.join("tests")).unwrap();
        let plan=prepare(root.clone(),dir.to_str().unwrap(),"run_tests",&json!({}),&flag()).unwrap();
        assert_eq!(plan.review["kind"],"command");assert_eq!(plan.review["testRunner"],"unittest");assert_eq!(std::fs::read_dir(&dir).unwrap().count(),2);
        assert!(!crate::agent_security::auto_approve("auto","run_tests"));assert!(!crate::agent_security::auto_approve("ask","run_tests"));
        for args in [json!({"directory":"../other"}),json!({"pattern":"*.py;touch x"}),json!({"directory":"$(id)"}),json!({"timeoutSeconds":121})]{assert!(prepare(root.clone(),dir.to_str().unwrap(),"run_tests",&args,&flag()).is_err());}
        drop(root);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn undo_restores_preexisting_content_and_guards_new_file(){
        let (path,root)=fixture();
        std::fs::write(path.join("original.txt"),"user uncommitted content").unwrap();
        for (file,old) in [("original.txt",Some("user uncommitted content")),("new.txt",None)] {
            let p=prepare(root.clone(),path.to_str().unwrap(),"write_file",&json!({"path":file,"content":"agent result"}),&flag()).unwrap();let review=p.review.clone();p.execute(flag(),|_|{}).unwrap();
            std::fs::write(path.join(file),"later change").unwrap();assert!(undo_file(root.clone(),path.to_str().unwrap(),&review).is_err());
            assert_eq!(std::fs::read_to_string(path.join(file)).unwrap(),"later change");
            std::fs::write(path.join(file),"agent result").unwrap();undo_file(root.clone(),path.to_str().unwrap(),&review).unwrap();
            if let Some(old)=old{assert_eq!(std::fs::read_to_string(path.join(file)).unwrap(),old);}else{assert!(!path.join(file).exists());}
        }
        drop(root);std::fs::remove_dir_all(path).unwrap();
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
    fn reviewed_line_counts_match_committed_result() {
        let (path,root)=fixture();
        for (index,(before,after,added,removed)) in [
            (None,"",0,0),(None,"一\n二",2,0),
            (Some("a\nb\n"),"a\nc\n",1,1),
            (Some("a\nb\n"),"",0,2),
            (Some("a\r\nb\r\n"),"a\r\nc\r\n",1,1),
            (Some("a"),"a\n",1,1),
        ].into_iter().enumerate() {
            let name=format!("stats-{index}.txt");if let Some(content)=before {std::fs::write(path.join(&name),content).unwrap();}
            let plan=prepare(root.clone(),path.to_str().unwrap(),"write_file",&json!({"path":name,"content":after}),&flag()).unwrap();
            let summary=json!({"addedLines":added,"removedLines":removed});assert_eq!(plan.review["changeSummary"],summary);
            let result=plan.execute(flag(), |_|{}).unwrap();assert_eq!(result["changeSummary"],summary);assert_eq!(result["created"],before.is_none());
            assert_eq!(result["previousSha256"],json!(before.map(|s|hash(s.as_bytes()))));assert_eq!(result["sha256"],hash(after.as_bytes()));
        }
        std::fs::write(path.join("patch.txt"),"a\nb\nc\n").unwrap();
        let plan=prepare(root.clone(),path.to_str().unwrap(),"apply_patch",&json!({"path":"patch.txt","edits":[{"oldText":"a","newText":"A"},{"oldText":"c","newText":"C"}]}),&flag()).unwrap();
        assert_eq!(plan.execute(flag(), |_|{}).unwrap()["changeSummary"],json!({"addedLines":2,"removedLines":2}));
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
    fn unchanged_is_success_but_stale_hash_still_rejected(){
        let (path,root)=fixture();std::fs::write(path.join("same.txt"),"same").unwrap();
        let args=json!({"path":"same.txt","content":"same","expectedSha256":hash(b"same")});
        let plan=prepare(root.clone(),path.to_str().unwrap(),"write_file",&args,&flag()).unwrap();
        assert_eq!(plan.unchanged().unwrap()["changed"],false);
        assert_eq!(plan.execute(flag(), |_|{}).unwrap()["unchanged"],true);
        let mut stale=args;stale["expectedSha256"]=json!(hash(b"old"));
        assert!(prepare(root.clone(),path.to_str().unwrap(),"write_file",&stale,&flag()).is_err());
        drop(root);std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn pipelines_preserve_failure_and_background_children_are_owned(){
        let (path,root)=fixture();
        let plan=prepare(root.clone(),path.to_str().unwrap(),"run_command",&json!({"command":"false | cat","timeoutSeconds":5}),&flag()).unwrap();
        let bash=plan.review["shell"]=="/bin/bash";
        let result=plan.execute(flag(), |_|{}).unwrap();
        if bash{assert_ne!(result["exitCode"],0);}
        assert_eq!(result["testVerdict"],"not_assessed");
        let plan=prepare(root.clone(),path.to_str().unwrap(),"run_command",&json!({"command":"(sleep 1; printf bad > orphan.txt) &","timeoutSeconds":5}),&flag()).unwrap();
        let result=plan.execute(flag(), |_|{}).unwrap();
        assert_eq!(result["processCleanup"]["attempted"],true);
        std::thread::sleep(Duration::from_millis(1200));assert!(!path.join("orphan.txt").exists());
        drop(root);std::fs::remove_dir_all(path).unwrap();
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
        parent.open_with(&leaf,&options).and_then(|mut file| {
            // Only a newly created root guide receives the template; create_new refuses existing files.
            if parent_path == Path::new(".") && name == "AGENTS.md" {
                file.write_all(include_bytes!("../resources/AGENTS.template.md"))?;
                file.sync_all()?;
            }
            Ok(())
        })
    };
    result.map_err(|e|if e.kind()==std::io::ErrorKind::AlreadyExists{"同名文件或目录已存在，未覆盖".into()}else{format!("创建失败：{e}")})
}

/// Undo only the exact recorded post-image. Never use Git reset or a repository baseline.
pub(crate) fn undo_file(root:Arc<Dir>, directory:&str, review:&Value)->Result<Value,String>{
    if review["rollbackVersion"]!=1 || review.get("undoneAt").is_some(){return Err("此记录无撤销快照或已撤销".into());}
    let relative=review["path"].as_str().ok_or("缺少文件路径")?;
    let expected=review["afterHash"].as_str().ok_or("缺少修改后哈希")?;
    let flag=Arc::new(AtomicBool::new(false));
    if let Some(content)=review["beforeContent"].as_str(){
        if Some(hash(content.as_bytes()).as_str())!=review["beforeHash"].as_str(){return Err("撤销快照校验失败".into());}
        let plan=prepare_editor(root,directory,&json!({"path":relative,"content":content,"expectedSha256":expected}),&flag)?;
        return plan.execute(flag,|_|{});
    }
    if review["newFile"]!=true{return Err("缺少原始文件内容".into());}
    let _guard=WRITES.lock().map_err(|_|"文件写入锁异常")?;
    let target=path(relative,false)?;
    no_links(&root,&target,false)?;
    let parent=root.open_dir(target.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(Path::new("."))).map_err(|_|"父目录不可访问")?;
    let name=PathBuf::from(target.file_name().ok_or("文件名无效")?);
    let current=snapshot(&parent,&name)?.ok_or("文件已不存在")?;
    if hash(current.0.as_bytes())!=expected{return Err("文件已有后续修改，拒绝撤销".into());}
    parent.remove_file(name).map_err(|_|"无法撤销新建文件")?;
    Ok(json!({"removed":true}))
}
