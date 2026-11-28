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
    let mut values: Vec<Value> = [
        ("list_directory", "分页列出当前项目目录，返回名称、类型和大小。path 默认为 .；首次读取不要填写 expectedSha256；有 nextList 时只复制本目录返回的完整参数续读。offset 从0开始，expectedSha256 校验目录列表，不得猜测或复用文件哈希；变化时移除 expectedSha256 并将 offset 设为0重新读取首页。", json!({"path":{"type":"string"},"offset":{"type":"integer","minimum":0},"expectedSha256":{"type":"string","minLength":64,"maxLength":64}}), json!([])),
        ("read_file", "读取项目相对路径的UTF-8文本。已知函数/片段时使用 path、anchor（唯一字面片段）、lineCount，一次返回附近上下文；已知行号使用 startLine、lineCount，默认200行。anchor不能与startLine/startColumn混用。startColumn仅复制nextRead续读参数，不用于行号。返回带行号内容；每页最多200行，每行片段最多1000字节。结果有 nextRead 时使用其完整参数续读，避免遗漏长行；startColumn 为首行从1开始的Unicode字符列，expectedSha256 用于拒绝跨文件版本续读。", json!({"path":{"type":"string"},"anchor":{"type":"string","minLength":1,"maxLength":200},"startLine":{"type":"integer","minimum":1},"startColumn":{"type":"integer","minimum":1},"expectedSha256":{"type":"string","minLength":64,"maxLength":64},"lineCount":{"type":"integer","minimum":1,"maximum":200}}), json!(["path"])),
        ("search_files", "字面搜索，推荐 {query:函数名,path:已知文件,mode:content}；pattern是query兼容别名，二者只能选一，不支持正则。path可为单个文件，已知文件不要从项目根目录重复搜索。默认返回2行相邻上下文，后续使用contextReadArgs或read_files读取相关实现和测试。在项目相对文件路径和 UTF-8 内容中搜索字面字符串；mode 为 all（默认）、filename 或 content；extensions 可限制后缀，如 [rs,py]，不带点，不区分大小写。内容结果包含命中行/字符列、附近片段及可直接调用 read_file 的 readArgs。maxResults 可选1–50（默认50）；truncationReasons 说明截断原因，可缩小范围继续搜索。contextLines 可选0–5（默认2），返回前后相邻行；contextReadArgs 用于读取同一版本的完整上下文。递归搜索 path（默认 .），跳过链接、私密文件及构建缓存。", json!({"path":{"type":"string"},"query":{"type":"string","minLength":1,"maxLength":200},"pattern":{"type":"string","minLength":1,"maxLength":200},"caseSensitive":{"type":"boolean"},"contextLines":{"type":"integer","minimum":0,"maximum":5},"maxResults":{"type":"integer","minimum":1,"maximum":50},"mode":{"type":"string","enum":["all","filename","content"]},"extensions":{"type":"array","minItems":1,"maxItems":10,"items":{"type":"string","minLength":1,"maxLength":16}}}), json!([])),
    ].into_iter().map(|(name,description,properties,required)|json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})).chain(crate::agent_earth::definitions()).chain(crate::agent_actions::definitions()).chain(std::iter::once(crate::agent_skills::definition())).chain(std::iter::once(crate::agent_plan::definition())).collect();
    let mut item=values.iter().find(|d|d["function"]["name"]=="read_file").unwrap()["function"]["parameters"].clone();
    values.push(crate::agent_plan_policy::expansion_tool());
    item["properties"]["path"]["maxLength"]=json!(1024);
    values.push(json!({"type":"function","function":{"name":"read_files","description":"批量读取1–8个已知的项目UTF-8文本文件；files每项使用read_file的参数。逐文件返回status及result/error，部分失败不表示全部失败或全部成功。总输出有界；deferred为本次未返回结果的完整参数，可作为下一次files。每个result的nextRead用于继续该文件，sha256保护版本；不是跨文件原子快照。优先用于已知相关文件，勿读取无关技能。","parameters":{"type":"object","properties":{"files":{"type":"array","minItems":1,"maxItems":8,"items":item}},"required":["files"],"additionalProperties":false}}}));
    values
}
pub(crate) fn title(name: &str) -> &str {
    match name {
        "earth_publish_model"=>"生成模型下载链接",
        "earth_search_datasets"=>"查询三维数据集",
        "earth_load_dataset"=>"加载三维数据集",
        "earth_remove_tileset"=>"移除三维图层",
        "earth_add_tileset"=>"加载3D Tiles",
        "earth_add_model"=>"加载三维模型",
        "earth_publish_image"=>"生成图片下载链接",
        "earth_add_image_layer"=>"叠加地理图片",
        "earth_get_view"=>"读取地球视图",
        "earth_fly_to"=>"地球定位",
        "earth_set_layer_visibility"=>"切换地球图层",
        "earth_get_selection"=>"读取地球选择",
        "earth_add_geojson_layer"=>"加载GeoJSON图层",
        "list_directory" => "列出项目目录",
        "read_file" => "读取项目文件",
        "read_files" => "批量读取项目文件",
        "search_files" => "搜索项目文件",
        "load_skill" => "加载技能",
        "read_skill_file" => "读取技能资源",
        "update_plan" => "更新任务计划",
        "expand_task_scope" => "调整任务范围",
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
// Convert a match in lowercased text back to original UTF-8 boundaries.
fn match_range(line: &str, needle: &str, sensitive: bool) -> Option<(usize,usize)> {
    if sensitive {return line.find(needle).map(|start|(start,start+needle.len()));}
    let lower=line.to_lowercase();let start=lower.find(needle)?;let end=start+needle.len();
    let mut lowered=0;let mut original_start=None;
    for (offset,ch) in line.char_indices(){
        let length=ch.to_lowercase().map(char::len_utf8).sum::<usize>();
        if original_start.is_none() && lowered+length>start{original_start=Some(offset);}
        lowered+=length;
        if lowered>=end{return Some((original_start.unwrap_or(offset),offset+ch.len_utf8()));}
    }
    None
}
pub(crate) fn execute(
    root: &Dir,
    name: &str,
    args: &Value,
    cancel: Arc<AtomicBool>,
    mut progress: impl FnMut(Value),
) -> Result<Value, String> {
    execute_until(root,name,args,cancel,&mut progress,Instant::now()+Duration::from_secs(8))
}
fn execute_until(root:&Dir,name:&str,args:&Value,cancel:Arc<AtomicBool>,progress:&mut dyn FnMut(Value),deadline:Instant)->Result<Value,String>{
    if cancel.load(Ordering::Relaxed){return Err("操作已停止".into());}
    if Instant::now()>=deadline{return Err("文件读取时间预算已用尽".into());}
    if name=="read_files" {
        crate::agent_tool_args::validate(name,args)?;
        let files=args["files"].as_array().ok_or("files 必须为数组")?;
        let input_bytes=args.to_string().len();if input_bytes>8192{return Err("批量读取参数超过8KiB".into());}
        let mut results=Vec::new();let mut next=0;let mut bytes=0;let mut failed=0;let mut partial=false;
        for (index,file) in files.iter().enumerate(){
            if cancel.load(Ordering::Relaxed){return Err("操作已停止".into());}
            if Instant::now()>=deadline || bytes>18000 {break;}
            let entry=match execute_until(root,"read_file",file,cancel.clone(),&mut |_|{},deadline){
                Ok(result)=>{partial|=result["truncated"]==true;json!({"index":index,"path":file["path"],"status":"succeeded","result":result})},
                Err(error)=>{if cancel.load(Ordering::Relaxed){return Err("操作已停止".into());}json!({"index":index,"path":file["path"],"status":"failed","error":error})}
            };
            if bytes+entry.to_string().len()+input_bytes>36000 {break;}
            failed+=usize::from(entry["status"]=="failed");bytes+=entry.to_string().len();results.push(entry);next=index+1;
            progress(json!({"completedFiles":next,"totalFiles":files.len(),"failedFiles":failed}));
        }
        if cancel.load(Ordering::Relaxed){return Err("操作已停止".into());}
        return Ok(json!({"results":results,"requestedFiles":files.len(),"completedFiles":next,"failedFiles":failed,"partialFailure":failed>0,"truncated":partial||next<files.len(),"deferred":&files[next..],"deferredReason":if next==files.len(){Value::Null}else if Instant::now()>=deadline{json!("time_limit")}else{json!("output_limit")}}));
    }
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
        deadline,
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
            let entries=scope.entries(&path)?.into_iter().map(|(name,_,directory,size)|json!({"name":name,"type":if directory{"directory"}else{"file"},"size":size})).collect::<Vec<_>>();
            let hash=crate::agent_actions::hash(serde_json::to_string(&entries).map_err(|_|"目录列表编码失败")?.as_bytes());
            if let Some(expected)=args.get("expectedSha256") {if expected.as_str()!=Some(hash.as_str()){return Err("目录列表已变化或 expectedSha256 不匹配，本次未返回列表。从首页重新读取时移除 expectedSha256 并将 offset 设为0；不要猜测哈希或使用文件的 sha256。后续仅复制此目录 nextList 中的完整参数，避免跨页遗漏或重复。".into());}}
            let offset=match args.get("offset"){None=>0,Some(v)=>v.as_u64().ok_or("offset 必须为非负整数")?};
            if offset>entries.len() as u64{return Err("offset 超出目录列表范围".into());}
            let mut page=Vec::new();let mut bytes=0;
            for entry in entries.iter().skip(offset as usize){
                let size=entry.to_string().len();if page.len()>=200 || bytes+size>12000{break;}bytes+=size;page.push(entry.clone());
            }
            let next=offset as usize+page.len();let truncated=next<entries.len();
            Ok(json!({"path":raw,"entries":page,"totalEntries":entries.len(),"offset":offset,"sha256":hash,"truncated":truncated,"nextList":if truncated{json!({"path":raw,"offset":next,"expectedSha256":hash})}else{Value::Null}}))
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
            let mut start = number("startLine", 1)?;
            let count = number("lineCount", 200)?;
            if count > 200 {
                return Err("lineCount 最多 200".into());
            }
            let column=number("startColumn",1)?;
            let text = scope.text(&path)?;
            let hash=crate::agent_actions::hash(text.as_bytes());
            if let Some(expected)=args.get("expectedSha256") {
                let expected=expected.as_str().ok_or("expectedSha256 必须是字符串")?;
                if expected!=hash {return Err("文件已变化，未返回续读内容；请从头重新读取，避免混合不同版本。".into());}
            }
            let all=text.lines().collect::<Vec<_>>();
            if let Some(anchor)=args.get("anchor") {
                if args.get("startLine").is_some()||args.get("startColumn").is_some(){return Err("anchor 与 startLine/startColumn 不能混用".into());}
                let anchor=anchor.as_str().filter(|v|!v.is_empty()&&v.chars().count()<=200).ok_or("anchor须为1–200字符的字面片段")?;
                let locations=text.match_indices(anchor).map(|(i,_)|text[..i].bytes().filter(|b|*b==b'\n').count()+1).take(9).collect::<Vec<_>>();
                if locations.len()!=1{return Err(format!("anchor须唯一匹配，匹配行（最多9个）：{:?}；请使用更具体的片段或startLine读取。",locations));}
                start=locations[0].saturating_sub(10).max(1) as u64;
            }

            let mut lines=Vec::new();let mut size=0usize;let mut next=Value::Null;let mut read_stop="end_of_file";
            for (i,line) in all.iter().enumerate(){
                scope.check()?;
                if (i as u64)+1<start{continue;}
                let current_column=if (i as u64)+1==start{column}else{1};
                if lines.len()>=count as usize || size>=12000 {
                    read_stop=if lines.len()>=count as usize {if args.get("lineCount").is_some(){"requested_range"}else{"line_limit"}}else{"byte_limit"};
                    next=json!({"path":raw,"startLine":i+1,"startColumn":current_column,"lineCount":count,"expectedSha256":hash});break;
                }
                let offset=if current_column==1{0}else{line.char_indices().nth((current_column-1).try_into().map_err(|_|"列号超出范围")?).map(|(index,_)|index).or_else(||if current_column-1==line.chars().count() as u64{Some(line.len())}else{None}).ok_or("startColumn 超出当前行范围")?};
                let rest=&line[offset..];let mut end=rest.floor_char_boundary(1000.min(12000-size));
                while end>0 && json!({"line":i+1,"text":&rest[..end]}).to_string().len()>12000-size {end=rest.floor_char_boundary(end/2);}
                let fragment=&rest[..end];
                if end==0 && !rest.is_empty(){read_stop="byte_limit";next=json!({"path":raw,"startLine":i+1,"startColumn":current_column,"lineCount":count,"expectedSha256":hash});break;}
                let row=json!({"line":i+1,"text":fragment});size+=row.to_string().len();lines.push(row);
                if end<rest.len(){read_stop="line_fragment_limit";next=json!({"path":raw,"startLine":i+1,"startColumn":current_column+fragment.chars().count() as u64,"lineCount":count,"expectedSha256":hash});break;}
            }
            Ok(json!({"path":raw,"startLine":start,"startColumn":column,"lines":lines,"totalLines":all.len(),"sha256":hash,"truncated":!next.is_null() && read_stop!="requested_range","hasMore":!next.is_null(),"readStop":read_stop,"requestedRangeComplete":matches!(read_stop,"requested_range"|"end_of_file"),"nextRead":next}))
        }

        "search_files" => {
            if args.get("query").is_some()&&args.get("pattern").is_some(){return Err("query与pattern只能提供一个".into());}
            let query = args.get("query").or_else(||args.get("pattern")).unwrap_or(&Value::Null)
                .as_str()
                .filter(|q| !q.is_empty() && q.chars().count() <= 200)
                .ok_or("query 必须为 1–200 字符的字符串")?;
            let sensitive = match args.get("caseSensitive") {
                None => false,
                Some(v) => v.as_bool().ok_or("caseSensitive 必须为布尔值")?,
            };
            let mode=match args.get("mode"){None=>"all",Some(v)=>v.as_str().filter(|v|matches!(*v,"all"|"filename"|"content")).ok_or("mode 必须为 all、filename 或 content")?};
            let context_lines = match args.get("contextLines") {
                None => 2,
                Some(value) => value.as_u64().filter(|n| *n <= 5).ok_or("contextLines 须为0–5的整数")? as usize,
            };
            let max_results=match args.get("maxResults") {
                None=>50,
                Some(value)=>value.as_u64().filter(|n|(1..=50).contains(n)).ok_or("maxResults 须为1–50的整数")? as usize,
            };
            let mut extensions=Vec::new();
            if let Some(value)=args.get("extensions"){
                let values=value.as_array().filter(|a|!a.is_empty() && a.len()<=10).ok_or("extensions 须为1–10个后缀")?;
                for value in values {let ext=value.as_str().filter(|s|!s.is_empty() && s.len()<=16 && s.bytes().all(|c|c.is_ascii_alphanumeric() || c==b'_' || c==b'-')).ok_or("后缀须为1–16个英文字母、数字、横线或下划线，不带点")?;extensions.push(ext.to_ascii_lowercase());}
            }
            let mut filtered=0;
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
            let mut output_bytes=0;
            let mut skipped = 0;
            let mut files = 0;
            let mut truncated = false;
            let mut truncation_reasons=std::collections::BTreeSet::new();
            'scan: while let Some((path, depth)) = stack.pop() {
                scope.check()?;
                if depth > 24 {
                    skipped += 1;
                    truncated = true;
                    truncation_reasons.insert("depth_limit");
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
                if !extensions.is_empty() && !path.extension().and_then(|e|e.to_str()).is_some_and(|ext|extensions.iter().any(|e|e==&ext.to_ascii_lowercase())){filtered+=1;continue;}
                files += 1;
                if files % 20 == 0 {
                    progress(json!({"scannedFiles":files,"matches":found.len()}));
                }
                let label = path.to_string_lossy().trim_start_matches("./").to_owned();
                if mode!="content" && matches(&label) {
                    if found.len()>=max_results {truncated=true;truncation_reasons.insert("result_limit");break;}
                    let hit=json!({"path":label,"kind":"filename"});let bytes=hit.to_string().len();if output_bytes+bytes>16000{truncated=true;truncation_reasons.insert("output_limit");break;}output_bytes+=bytes;found.push(hit);
                }
                if mode=="filename" {continue;}
                match scope.text(&path) {
                    Ok(text) => {
                        let hash=crate::agent_actions::hash(text.as_bytes());
                        let lines: Vec<&str> = text.lines().collect();
                        for (i, line) in lines.iter().enumerate() {
                            scope.check()?;
                            if let Some((start,end))=match_range(line,&needle,sensitive) {
                                if found.len()>=max_results {truncated=true;truncation_reasons.insert("result_limit");break 'scan;}
                                let from=line.floor_char_boundary(start.saturating_sub(80));let to=line.floor_char_boundary((end+80).min(line.len()));
                                let column=line[..start].chars().count()+1;let snippet_column=line[..from].chars().count()+1;
                                let mut hit=json!({"path":label,"line":i+1,"column":column,"snippetStartColumn":snippet_column,"lineTruncated":from>0 || to<line.len(),"text":&line[from..to],"readArgs":{"path":label,"startLine":i+1,"startColumn":snippet_column,"lineCount":1,"expectedSha256":hash}});
                                if context_lines > 0 {
                                    let first = i.saturating_sub(context_lines);
                                    let last = (i + context_lines + 1).min(lines.len());
                                    let context: Vec<Value> = (first..last).map(|index| {
                                        let source = lines[index];
                                        let (begin, end) = if index == i { (from, to) }
                                            else { (0, source.floor_char_boundary(source.len().min(160))) };
                                        json!({"line":index+1,"text":&source[begin..end],"startColumn":source[..begin].chars().count()+1,"isMatch":index==i,"truncated":begin>0 || end<source.len()})
                                    }).collect();
                                    hit["contextTruncated"] = json!(context.iter().any(|row| row["truncated"] == true));
                                    hit["context"] = json!(context);
                                    hit["contextReadArgs"] = json!({"path":label,"startLine":first+1,"lineCount":last-first,"expectedSha256":hash});
                                }
                                let bytes=hit.to_string().len();if output_bytes+bytes>16000{truncated=true;truncation_reasons.insert("output_limit");break 'scan;}output_bytes+=bytes;found.push(hit);
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
            }
            Ok(
                json!({"query":query,"mode":mode,"extensions":extensions,"contextLines":context_lines,"maxResults":max_results,"truncationReasons":truncation_reasons,"matches":found,"scannedFiles":files,"filteredFiles":filtered,"skippedFiles":skipped,"truncated":truncated}),
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
    fn stale_directory_hash_explains_how_to_reset_without_weakening_the_check() {
        let (root,dir)=fixture();
        let error=run(&dir,"list_directory",json!({"expectedSha256":"0".repeat(64)})).unwrap_err();
        assert!(error.contains("移除 expectedSha256"));assert!(error.contains("offset"));
        let fresh=run(&dir,"list_directory",json!({"offset":0})).unwrap();
        assert!(fresh["sha256"].as_str().is_some_and(|s|s.len()==64));
        assert!(run(&dir,"list_directory",json!({"expectedSha256":"0".repeat(64)})).is_err());
        drop(dir);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn anchor_reads_and_search_alias_keep_scope_and_version_checks(){
        let (root,dir)=fixture();let v=run(&dir,"read_file",json!({"path":"README.md","anchor":"project needle"})).unwrap();assert_eq!(v["startLine"],1);assert_eq!(v["lines"].as_array().unwrap().len(),3);
        for args in [json!({"path":"README.md","anchor":"missing"}),json!({"path":"README.md","anchor":"e"}),json!({"path":"README.md","anchor":"needle","startLine":1}),json!({"path":"../secret","anchor":"needle"}),json!({"path":"README.md","anchor":"needle","expectedSha256":"0".repeat(64)})]{assert!(run(&dir,"read_file",args).is_err());}
        let a=run(&dir,"search_files",json!({"query":"needle","path":"README.md"})).unwrap();let b=run(&dir,"search_files",json!({"pattern":"needle","path":"README.md"})).unwrap();assert_eq!(a,b);assert_eq!(b["matches"][0]["context"].as_array().unwrap().len(),3);
        assert!(run(&dir,"search_files",json!({"pattern":"needle","query":"other"})).is_err());drop(dir);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn batch_reads_preserve_per_file_errors_versions_and_boundaries(){
        let (root,dir)=fixture();
        let args=json!({"files":[{"path":"README.md","lineCount":1},{"path":"missing.txt"},{"path":"../secret.txt"},{"path":".env"},{"path":"src/main.rs"}]});
        let result=run(&dir,"read_files",args).unwrap();
        assert_eq!(result["completedFiles"],5);assert_eq!(result["failedFiles"],3);assert_eq!(result["partialFailure"],true);
        assert_eq!(result["results"][0]["result"]["lines"][0]["text"],"# Demo");
        let next=result["results"][0]["result"]["nextRead"].clone();
        assert_eq!(run(&dir,"read_files",json!({"files":[next.clone()]})).unwrap()["results"][0]["status"],"succeeded");
        std::fs::write(root.join("README.md"),"changed").unwrap();
        assert_eq!(run(&dir,"read_files",json!({"files":[next]})).unwrap()["results"][0]["status"],"failed");
        assert!(run(&dir,"read_files",json!({"files":[]})).is_err());
        assert!(run(&dir,"read_files",json!({"files":vec![json!({"path":"README.md"});9]})).is_err());
        assert!(run(&dir,"read_files",json!({"files":[{"path":"README.md","lineCount":0}]})).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn batch_reads_defer_without_losing_items_and_stop_between_files(){
        let (root,dir)=fixture();
        for i in 0..8 {std::fs::write(root.join(format!("{i}.txt")),"中文\n".repeat(200)).unwrap();}
        let mut files:Vec<Value>=(0..8).map(|i|json!({"path":format!("{i}.txt")})).collect();let mut total=0;
        while !files.is_empty(){let value=run(&dir,"read_files",json!({"files":files})).unwrap();assert!(value.to_string().len()<40000);let n=value["completedFiles"].as_u64().unwrap();assert!(n>0);total+=n;files=value["deferred"].as_array().unwrap().clone();}
        assert_eq!(total,8);
        let flag=Arc::new(AtomicBool::new(false));let copy=flag.clone();let mut events=0;
        assert!(execute(&dir,"read_files",&json!({"files":[{"path":"README.md"},{"path":"src/main.rs"}]}),flag,|_|{events+=1;copy.store(true,Ordering::Relaxed);}).is_err());assert_eq!(events,1);
        assert!(execute_until(&dir,"read_files",&json!({"files":[{"path":"README.md"}]}),Arc::new(AtomicBool::new(false)),&mut |_|{},Instant::now()).is_err());
        std::fs::remove_dir_all(root).unwrap();
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
        assert_eq!(file["truncated"], false);assert_eq!(file["hasMore"],true);assert_eq!(file["readStop"],"requested_range");assert_eq!(file["nextRead"]["startLine"],3);
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
        let limited=run(&dir,"read_file",json!({"path":"long","lineCount":1})).unwrap();
        assert_eq!(limited["truncated"],true);assert_eq!(limited["requestedRangeComplete"],false);assert_eq!(limited["readStop"],"line_fragment_limit");
        drop(dir);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn long_unicode_lines_resume_without_loss_and_reject_changes(){
        let (root,dir)=fixture();let content=format!("{}\nsecond line\n", "中文🙂".repeat(500));std::fs::write(root.join("unicode"),&content).unwrap();
        let mut args=json!({"path":"unicode","lineCount":1});let mut restored=String::new();let mut pages=0;
        loop{let result=run(&dir,"read_file",args).unwrap();pages+=1;
            for line in result["lines"].as_array().unwrap(){if line["line"]==1{restored.push_str(line["text"].as_str().unwrap());}}
            if result["nextRead"].is_null(){break;}args=result["nextRead"].clone();assert!(pages<20);
        }
        assert_eq!(restored,"中文🙂".repeat(500));assert!(pages>1);
        let first=run(&dir,"read_file",json!({"path":"unicode"})).unwrap();std::fs::write(root.join("unicode"),"changed").unwrap();
        assert!(run(&dir,"read_file",first["nextRead"].clone()).unwrap_err().contains("文件已变化"));
        assert!(run(&dir,"read_file",json!({"path":"unicode","startColumn":100})).is_err());
        drop(dir);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn escaped_output_stays_bounded_without_skipping_text(){
        let (root,dir)=fixture();let content=(0..100).map(|_|format!("{}\n","\u{1}".repeat(100))).collect::<String>();std::fs::write(root.join("escaped"),content).unwrap();
        let mut args=json!({"path":"escaped"});let mut seen=0;let mut pages=0;
        loop{let result=run(&dir,"read_file",args).unwrap();assert!(result.to_string().len()<16000);pages+=1;
            seen+=result["lines"].as_array().unwrap().iter().map(|v|v["text"].as_str().unwrap().len()).sum::<usize>();
            if result["nextRead"].is_null(){break;}args=result["nextRead"].clone();assert!(pages<20);
        }
        assert_eq!(seen,10000);drop(dir);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn directory_pages_are_complete_and_revision_checked(){
        let (root,dir)=fixture();std::fs::create_dir(root.join("many")).unwrap();
        for i in 0..240{std::fs::write(root.join("many").join(format!("{i:03}-{}", "x".repeat(80))),"").unwrap();}
        std::fs::write(root.join("many/.env"),"hidden").unwrap();
        let first=run(&dir,"list_directory",json!({"path":"many"})).unwrap();assert_eq!(first["totalEntries"],240);assert_eq!(first["truncated"],true);
        let mut args=json!({"path":"many"});let mut names=std::collections::HashSet::new();let mut pages=0;
        loop{let result=run(&dir,"list_directory",args).unwrap();pages+=1;assert!(result.to_string().len()<16000);
            for item in result["entries"].as_array().unwrap(){assert!(names.insert(item["name"].as_str().unwrap().to_owned()));}
            if result["nextList"].is_null(){break;}args=result["nextList"].clone();assert!(pages<10);
        }
        assert_eq!(names.len(),240);assert!(pages>1);
        std::fs::write(root.join("many/new"),"").unwrap();assert!(run(&dir,"list_directory",first["nextList"].clone()).unwrap_err().contains("已变化"));
        assert!(run(&dir,"list_directory",json!({"offset":99999})).is_err());
        drop(dir);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn search_modes_and_extensions_preserve_scope(){
        let (root,dir)=fixture();std::fs::write(root.join("needle.PY"),"unrelated").unwrap();std::fs::write(root.join("needle.bin"),[0,1,2]).unwrap();
        let names=run(&dir,"search_files",json!({"query":"needle","mode":"filename"})).unwrap();assert_eq!(names["matches"].as_array().unwrap().len(),2);assert_eq!(names["skippedFiles"],0);
        let content=run(&dir,"search_files",json!({"query":"needle","mode":"content","extensions":["RS"]})).unwrap();assert_eq!(content["matches"].as_array().unwrap().len(),1);assert!(content["matches"][0]["path"].as_str().unwrap().ends_with("main.rs"));assert_eq!(content["scannedFiles"],1);
        let python=run(&dir,"search_files",json!({"query":"needle","mode":"filename","extensions":["py"]})).unwrap();assert_eq!(python["matches"][0]["path"],"needle.PY");
        for args in [json!({"query":"x","mode":"bad"}),json!({"query":"x","extensions":["../rs"]}),json!({"query":"x","extensions":[]})]{assert!(run(&dir,"search_files",args).is_err());}
        drop(dir);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn search_locates_late_unicode_match_and_reads_same_revision(){
        let (root,dir)=fixture();let prefix="中文İ".repeat(300);let content=format!("{prefix}NEEDLE{}", "后".repeat(100));std::fs::write(root.join("late.txt"),&content).unwrap();
        let result=run(&dir,"search_files",json!({"path":"late.txt","query":"needle","mode":"content"})).unwrap();let hit=&result["matches"][0];
        assert_eq!(hit["column"],901);assert!(hit["text"].as_str().unwrap().contains("NEEDLE"));assert_eq!(hit["lineTruncated"],true);
        let read=run(&dir,"read_file",hit["readArgs"].clone()).unwrap();assert!(read["lines"][0]["text"].as_str().unwrap().contains("NEEDLE"));
        std::fs::write(root.join("bounded.txt"),(0..100).map(|_|format!("{}needle{}\n","前".repeat(200),"后".repeat(200))).collect::<String>()).unwrap();
        let bounded=run(&dir,"search_files",json!({"path":"bounded.txt","query":"needle","mode":"content"})).unwrap();assert_eq!(bounded["truncated"],true);assert!(bounded.to_string().len()<20000);assert!(bounded["matches"].as_array().unwrap().iter().all(|v|v["text"].as_str().unwrap().contains("needle")));
        assert_eq!(match_range("İx","x",false),Some((2,3)));assert_eq!(match_range("İx","i",false),Some((0,2)));assert_eq!(match_range("ABC","a",true),None);
        std::fs::write(root.join("late.txt"),"changed").unwrap();assert!(run(&dir,"read_file",hit["readArgs"].clone()).is_err());
        drop(dir);std::fs::remove_dir_all(root).unwrap();
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

    #[test]
    fn search_result_limit_distinguishes_exact_and_extra_matches() {
        let (root,dir)=fixture();
        std::fs::write(root.join("limit.txt"),"needle\nneedle\n").unwrap();
        let exact=run(&dir,"search_files",json!({"path":"limit.txt","query":"needle","maxResults":2})).unwrap();
        assert_eq!(exact["matches"].as_array().unwrap().len(),2);assert_eq!(exact["truncated"],false);assert_eq!(exact["truncationReasons"],json!([]));
        let limited=run(&dir,"search_files",json!({"path":"limit.txt","query":"needle","maxResults":1})).unwrap();
        assert_eq!(limited["matches"].as_array().unwrap().len(),1);assert_eq!(limited["truncated"],true);assert_eq!(limited["truncationReasons"],json!(["result_limit"]));
        std::fs::write(root.join("needle.txt"),"needle").unwrap();
        for (mode,count,truncated) in [("filename",1,false),("content",1,false),("all",1,true)] {
            let result=run(&dir,"search_files",json!({"path":"needle.txt","query":"needle","mode":mode,"maxResults":1})).unwrap();
            assert_eq!(result["matches"].as_array().unwrap().len(),count);assert_eq!(result["truncated"],truncated);
        }
        for value in [json!(0),json!(51),json!(-1),json!(1.5),json!("1")] {assert!(run(&dir,"search_files",json!({"query":"needle","maxResults":value})).is_err());}
        drop(dir);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn search_context_has_numbered_neighbors_and_versioned_reads() {
        let (root, dir) = fixture();
        std::fs::write(root.join("context.txt"), "needle first\n中文前文\nneedle middle\n\nneedle last").unwrap();
        let args=json!({"path":"context.txt","query":"needle","mode":"content","contextLines":1});
        let result=run(&dir,"search_files",args).unwrap();
        let hits=result["matches"].as_array().unwrap();
        assert_eq!(hits.len(),3);
        assert_eq!(hits[0]["context"].as_array().unwrap().len(),2);
        assert_eq!(hits[1]["context"][0]["line"],2);
        assert_eq!(hits[1]["context"][0]["text"],"中文前文");
        assert_eq!(hits[1]["context"][1]["isMatch"],true);
        assert_eq!(hits[1]["context"][2]["text"],"");
        assert_eq!(hits[2]["context"].as_array().unwrap().len(),2);
        let read=run(&dir,"read_file",hits[1]["contextReadArgs"].clone()).unwrap();
        assert_eq!(read["lines"][0]["text"],"中文前文");
        let plain=run(&dir,"search_files",json!({"path":"context.txt","query":"needle","contextLines":0})).unwrap();
        assert!(plain["matches"][0].get("context").is_none());
        let names=run(&dir,"search_files",json!({"query":"context","mode":"filename","contextLines":5})).unwrap();
        assert!(names["matches"][0].get("context").is_none());
        for value in [json!(-1),json!(6),json!(1.5),json!("2")] {
            assert!(run(&dir,"search_files",json!({"query":"needle","contextLines":value})).is_err());
        }
        std::fs::write(root.join("context.txt"),"changed").unwrap();
        assert!(run(&dir,"read_file",hits[1]["contextReadArgs"].clone()).is_err());
        drop(dir);std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn search_context_is_bounded_and_long_match_remains_visible() {
        let (root, dir) = fixture();
        let neighbor="中文\u{0001}".repeat(400);
        let body=format!("{}\n{}needle\n{}",neighbor,"中".repeat(1000),neighbor);
        std::fs::write(root.join("context.txt"),&body).unwrap();
        let args=json!({"path":"context.txt","query":"needle","contextLines":5});
        let result=run(&dir,"search_files",args.clone()).unwrap();
        let hit=&result["matches"][0];
        assert_eq!(hit["contextTruncated"],true);
        assert!(hit["context"][1]["text"].as_str().unwrap().contains("needle"));
        assert!(hit["context"][1]["startColumn"].as_u64().unwrap()>900);
        assert!(result.to_string().len()<20000);
        std::fs::write(root.join("context.txt"),body.repeat(100)).unwrap();
        let result=run(&dir,"search_files",args).unwrap();
        assert_eq!(result["truncated"],true);
        assert!(!result["matches"].as_array().unwrap().is_empty());
        assert_eq!(result["truncationReasons"],json!(["output_limit"]));
        assert!(result.to_string().len()<20000);
        drop(dir);std::fs::remove_dir_all(root).unwrap();
    }

}

/// Full, lossless editor read. Never assemble editable content from clipped tool pages.
pub(crate) fn editor_text(root:&Dir,raw:&str)->Result<Value,String>{
    let mut scope=Scope{dir:root.try_clone().map_err(|_|"无法打开项目")?,cancel:Arc::new(AtomicBool::new(false)),deadline:Instant::now()+Duration::from_secs(8),visited:0,bytes:0};
    let path=scope.path(raw)?;let content=scope.text(&path)?;
    let editable=content.len()<=131072;
    Ok(json!({"content":content,"sha256":crate::agent_actions::hash(content.as_bytes()),"editable":editable}))

}

pub(crate) fn image_bytes(root:&Dir,raw:&str)->Result<Vec<u8>,String>{
    let scope=Scope{dir:root.try_clone().map_err(|_|"无法打开项目")?,cancel:Arc::new(AtomicBool::new(false)),deadline:Instant::now()+Duration::from_secs(8),visited:0,bytes:0};
    let path=scope.path(raw)?;
    let file=scope.dir.open(&path).map_err(|_|"无法读取图片")?;
    let meta=file.metadata().map_err(|_|"无法读取图片信息")?;
    if !meta.is_file() || meta.len()>4*1024*1024{return Err("图片须为不超过4 MiB的普通文件".into());}
    let mut bytes=Vec::new();file.take(4*1024*1024+1).read_to_end(&mut bytes).map_err(|_|"读取图片失败")?;
    if bytes.len()>4*1024*1024{return Err("图片超过4 MiB".into());}Ok(bytes)
}

pub(crate) fn model_bytes(root:&Dir,raw:&str)->Result<Vec<u8>,String>{
    let scope=Scope{dir:root.try_clone().map_err(|_|"无法打开项目")?,cancel:Arc::new(AtomicBool::new(false)),deadline:Instant::now()+Duration::from_secs(8),visited:0,bytes:0};
    let path=scope.path(raw)?;
    let file=scope.dir.open(&path).map_err(|_|"无法读取模型")?;
    let meta=file.metadata().map_err(|_|"无法读取模型信息")?;
    if !meta.is_file() || meta.len()>16*1024*1024{return Err("模型须为不超过16 MiB的普通文件".into());}
    let mut bytes=Vec::new();file.take(16*1024*1024+1).read_to_end(&mut bytes).map_err(|_|"读取模型失败")?;
    if bytes.len()>16*1024*1024{return Err("模型超过16 MiB".into());}Ok(bytes)
}
