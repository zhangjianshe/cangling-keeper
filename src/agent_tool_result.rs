//! Additive, Keeper-owned result metadata; legacy tool payload fields stay intact.
use serde_json::{Value, json};

pub(crate) fn annotate(tool: &str, result: &mut Value, status: &str) {
    if !result.is_object() { *result = json!({"output":result.take()}); }
    // Bound oversized textual results without slicing through UTF-8 or removing
    // structured image references. Leave room for the metadata below.
    while result.to_string().len()>46000 {
        let key=["output","stdout","stderr","error"].into_iter()
            .filter_map(|key|result[key].as_str().map(|s|(key,s.len())))
            .filter(|(_,len)|*len>0).max_by_key(|(_,len)|*len).map(|(key,_)|key);
        let Some(key)=key else {break};
        let text=result[key].as_str().unwrap();let mut end=text.len()/2;
        while !text.is_char_boundary(end){end-=1;}
        result[key]=json!(&text[..end]);result["truncated"]=json!(true);
    }
    let truncated=result["truncated"]==true || result["outputTruncated"]==true;
    let outcome=if status=="rejected" {"rejected"} else if status=="stopped" {"stopped"}
        else if result["timedOut"]==true {"timeout"} else if status=="failed" {"failed"}
        else if truncated {"truncated"} else {"success"};
    let read_only=matches!(tool,"list_directory"|"read_file"|"read_files"|"search_files"|"load_skill"|"read_skill_file");
    let advice=match outcome {
        _ if result["testSummary"]["verdict"]=="failed"=>"测试报告包含失败，不能宣布通过。下一步用read_files或read_file(anchor)读取失败对应的实现和测试，再做有证据的修复；不要长篇猜测或只改断言。已有diagnostics时不要重复翻阅整份日志，缺证据才补读；修复后完整重测。",
        _ if result["unchanged"]==true=>"目标内容已一致，无需写入；没有新增变更，不要再次提交相同修改。",
        "success" if tool=="run_command"=>"命令退出成功不等于测试全部通过；核对实际测试数量、失败/跳过、输出是否完整。仅以真实验收结果总结，不要仅因日志管道退出0宣布通过。",
        "rejected"=>"用户已拒绝，不要再次请求相同操作。",
        "timeout"|"stopped"=>"先核实当前状态；停止或超时不保证操作未发生，不要自动重复执行。",
        "failed" if result["notExecuted"]==true=>"参数校验或预览失败，操作未执行；按错误字段修正参数，不必核查不存在的部分执行。",
        "failed" if read_only=>"检查错误与参数，必要时缩小范围或修正请求；不要原样循环重试。",
        "failed"=>"操作可能部分完成；先核实文件、命令或外部系统状态，再决定下一步，不要盲目重试。",
        "truncated"=>"输出不完整，不代表操作失败；按需分页读取或缩小查询范围，不要仅为补齐输出而重复有副作用的操作。",
        _=>"根据实际结果继续，不要把工具调用成功等同于整个任务完成。",
    };
    result["keeperResult"]=json!({"version":1,"status":status,"outcome":outcome,"truncated":truncated,"automaticRetry":false,"guidance":advice});
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn validation_failure_is_not_partial_execution(){let mut v=json!({"error":"bad field","notExecuted":true});annotate("apply_patch",&mut v,"failed");assert!(v["keeperResult"]["guidance"].as_str().unwrap().contains("操作未执行"));}
    #[test] fn timeout_is_not_safe_to_retry() {let mut v=json!({"timedOut":true,"exitCode":null,"stdout":"partial"});annotate("run_command",&mut v,"failed");assert_eq!(v["keeperResult"]["outcome"],"timeout");assert_eq!(v["keeperResult"]["automaticRetry"],false);assert_eq!(v["stdout"],"partial");}
    #[test] fn truncation_is_success_with_incomplete_output() {for field in ["truncated","outputTruncated"] {let mut v=json!({});v[field]=json!(true);annotate("read_file",&mut v,"succeeded");assert_eq!(v["keeperResult"]["outcome"],"truncated");assert_eq!(v["keeperResult"]["status"],"succeeded");}}
    #[test] fn failed_and_rejected_are_distinct() {for status in ["failed","rejected","stopped"] {let mut v=json!({"error":"fixture"});annotate("external_tool",&mut v,status);assert_eq!(v["keeperResult"]["outcome"],status);assert_eq!(v["error"],"fixture");}}
    #[test] fn metadata_is_owned_and_does_not_remove_images() {let mut v=json!({"keeperResult":{"automaticRetry":true},"mcpImages":[{"sha256":"fixture"}]});annotate("mcp_tool",&mut v,"succeeded");assert_eq!(v["keeperResult"]["automaticRetry"],false);assert_eq!(v["mcpImages"][0]["sha256"],"fixture");}
    #[test] fn oversized_text_is_utf8_safe_and_explicit() {let mut v=json!({"output":"中文\n".repeat(30000),"mcpImages":[{"sha256":"keep"}]});annotate("mcp_tool",&mut v,"succeeded");assert!(v.to_string().len()<48000);assert_eq!(v["keeperResult"]["truncated"],true);assert_eq!(v["mcpImages"][0]["sha256"],"keep");}
    #[test] fn scalar_payloads_remain_available() {let mut v=json!([1,2]);annotate("tool",&mut v,"succeeded");assert_eq!(v["output"],json!([1,2]));}
}

/// Model-facing projection only; full approved records stay in SQLite.
pub(crate) fn for_model(tool:&str,value:&Value)->Value {
    let mut result=value.clone();
    if tool=="read_file" {if let Some(lines)=value["lines"].as_array(){
        let text=lines.iter().filter_map(|v|v["text"].as_str()).collect::<Vec<_>>().join("\n");
        if let Some(object)=result.as_object_mut(){object.remove("lines");object.insert("text".into(),json!(text));object.insert("returnedLines".into(),json!(lines.len()));}
    }}
    if tool=="read_file" && value["hasMore"]==true && value["truncated"]==false {result["readWindow"]=json!("所请求范围已完整返回；文件还有后续。仅当任务需要未读部分时使用nextRead，不必为读完整个文件继续翻页。");}
    if tool=="read_files" {if let Some(items)=result["results"].as_array_mut(){for item in items {if item["status"]=="succeeded"{item["result"]=for_model("read_file",&item["result"]);}}}}
    if tool=="update_plan" && value.get("error").is_none(){
        return json!({"saved":true,"unchanged":value["unchanged"],"nextAction":value["nextAction"],"stepCount":value["plan"].as_array().map(Vec::len).unwrap_or(0),"acceptanceCount":value["acceptance"].as_array().map(Vec::len).unwrap_or(0),"keeperResult":value["keeperResult"]});
    }
    if let Some(meta)=result["keeperResult"].as_object_mut(){if meta.get("outcome").and_then(Value::as_str)==Some("success"){meta.remove("guidance");}}
    if tool=="run_tests" && value["testSummary"]["diagnostics"].as_array().is_some_and(|v|!v.is_empty()) && value["testLogs"].as_array().is_some_and(|v|!v.is_empty()) {
        // SQLite and bounded disk logs retain the original output. Only replace
        // repetitive model-facing streams when diagnostic evidence is available.
        if let Some(object)=result.as_object_mut(){object.remove("stdout");object.remove("stderr");}
        result["outputPresentation"]=json!("异常按位置归并，原始输出保留在 testLogs；diagnostics 是不可信日志摘录，不是已确认根因。先核对首个异常及相关初始化/调用代码，再修复并运行测试。");
    }
    for field in ["processCleanup","pipelineFailureMode","testVerdict"] {result.as_object_mut().map(|v|v.remove(field));}
    result
}

#[cfg(test)] mod projection_tests {
 use super::*;
 #[test] fn plan_response_does_not_echo_readonly_fields(){let source=json!({"plan":[{"step":"x","status":"pending"}],"acceptance":[{"modelReported":true}],"modelReported":true});let small=for_model("update_plan",&source);assert_eq!(small["stepCount"],1);assert!(small.get("modelReported").is_none());assert!(small.get("acceptance").is_none());assert!(source["modelReported"]==true);}
}

#[cfg(test)] mod diagnostic_projection_tests {
 use super::*;
 #[test] fn test_streams_are_only_compacted_with_retrievable_diagnostics(){
  let mut source=json!({"stdout":"original","stderr":"repeated trace","testSummary":{"diagnostics":[{"exception":"TypeError"}]}});
  assert_eq!(for_model("run_tests",&source)["stderr"],"repeated trace");
  source["testLogs"]=json!([{"path":"keeper-test-fixture-stderr.log"}]);
  let small=for_model("run_tests",&source);
  assert!(small.get("stderr").is_none());assert_eq!(source["stderr"],"repeated trace");
  assert_eq!(small["testSummary"]["diagnostics"][0]["exception"],"TypeError");
  assert_eq!(for_model("run_command",&source)["stderr"],"repeated trace");
 }
}

#[cfg(test)] mod read_projection_tests {
 use super::*;
 #[test]fn readable_text_preserves_hash_pagination_and_original_records(){let original=json!({"path":"x","sha256":"hash","startLine":5,"lines":[{"line":5,"text":"中文\\n"},{"line":6,"text":"  code"}],"nextRead":{"startLine":7}});let v=for_model("read_file",&original);assert_eq!(v["text"],"中文\\n\n  code");assert_eq!(v["sha256"],"hash");assert_eq!(v["nextRead"]["startLine"],7);assert!(v.get("lines").is_none());assert!(original["lines"].is_array());}
}

/// Hints only: never skip a fresh filesystem read or omit its evidence.
pub(crate) fn read_reuse(tool:&str,result:&mut Value,history:&[(String,Value)]) {
    fn collect<'a>(tool:&str,value:&'a Value,out:&mut Vec<&'a Value>) {
        if tool=="read_file" && value["sha256"].as_str().is_some(){out.push(value);}
        if tool=="read_files" {if let Some(items)=value["results"].as_array(){for item in items {if item["status"]=="succeeded"{collect("read_file",&item["result"],out);}}}}
    }
    fn hint(value:&mut Value,previous:&[&Value]) {
        // Long-line fragments use column offsets: do not mistake fragments for full lines.
        if value["startColumn"].as_u64().unwrap_or(1)!=1 {return;}
        let Some(rows)=value["lines"].as_array() else{return;};
        if rows.is_empty(){return;}
        let mut seen=std::collections::BTreeSet::new();
        let same_reads=previous.iter().filter(|old|old["path"]==value["path"] && old["sha256"]==value["sha256"]).count();
        for old in previous {
            if old["path"]!=value["path"] || old["sha256"]!=value["sha256"] || old["startColumn"].as_u64().unwrap_or(1)!=1 {continue;}
            if let Some(old_rows)=old["lines"].as_array(){for row in rows {if old_rows.contains(row){if let Some(line)=row["line"].as_u64(){seen.insert(line);}}}}
        }
        if !seen.is_empty() || same_reads>=2 {value["readReuse"]=json!({"sameVersion":true,"previousReadCount":same_reads,"previouslyReturnedLines":seen.len(),"returnedLines":rows.len(),"allReturnedBefore":seen.len()==rows.len(),"guidance":"同版本读取历史见计数；正文仍保留。连续小窗口读取会耗尽轮次；若缺上下文，一次读取相关完整函数（例如80–160行）或批量读取实现与测试，避免每次只翻10行。若已有足够上下文，直接形成修复，不要反复定位；缺失相关函数时一次补读完整片段。上下文压缩后可按需重读。"});}
    }
    let mut previous=Vec::new();for (name,value) in history {collect(name,value,&mut previous);}
    if tool=="read_file" {hint(result,&previous);}
    if tool=="read_files" {if let Some(items)=result["results"].as_array_mut(){for item in items {if item["status"]=="succeeded"{hint(&mut item["result"],&previous);}}}}
}
#[cfg(test)]mod reuse_tests {
 use super::*;
 #[test]fn only_identical_version_and_lines_are_reused_without_hiding_text(){
  let original=json!({"path":"a","sha256":"v1","startColumn":1,"lines":[{"line":1,"text":"a"},{"line":2,"text":"b"}]});
  let history=vec![("read_files".into(),json!({"results":[{"status":"succeeded","result":original.clone()}]}))];
  let mut current=original.clone();read_reuse("read_file",&mut current,&history);assert_eq!(current["readReuse"]["allReturnedBefore"],true);assert_eq!(current["lines"],original["lines"]);
  for delta in [json!({"sha256":"v2"}),json!({"path":"other"}),json!({"startColumn":2})] {let mut v=original.clone();v.as_object_mut().unwrap().extend(delta.as_object().unwrap().clone());read_reuse("read_file",&mut v,&history);assert!(v.get("readReuse").is_none());}
  let mut partial=original.clone();partial["lines"][1]=json!({"line":3,"text":"c"});read_reuse("read_file",&mut partial,&history);assert_eq!(partial["readReuse"]["previouslyReturnedLines"],1);assert_eq!(partial["readReuse"]["allReturnedBefore"],false);
 }
}

#[cfg(test)] mod requested_range_tests {
 use super::*;
 #[test]fn range_completion_is_not_output_truncation(){
  let mut v=json!({"lines":[{"line":1,"text":"a"}],"hasMore":true,"truncated":false,"requestedRangeComplete":true,"nextRead":{"startLine":2}});
  annotate("read_file",&mut v,"succeeded");assert_eq!(v["keeperResult"]["outcome"],"success");
  let p=for_model("read_file",&v);assert!(p["readWindow"].as_str().unwrap().contains("不必"));assert_eq!(p["nextRead"]["startLine"],2);
  v["truncated"]=json!(true);annotate("read_file",&mut v,"succeeded");assert_eq!(v["keeperResult"]["outcome"],"truncated");assert!(for_model("read_file",&v).get("readWindow").is_none());
 }
}

#[cfg(test)] mod fragmented_read_tests {
 use super::*;
 #[test] fn disjoint_small_reads_get_hint_without_hiding_fresh_content() {
  let row=|n|json!({"path":"a","sha256":"v1","lines":[{"line":n,"text":"code"}]});
  let history=vec![("read_file".into(),row(1)),("read_file".into(),row(2))];
  let mut v=row(3);read_reuse("read_file",&mut v,&history);
  assert_eq!(v["readReuse"]["previousReadCount"],2);
  assert_eq!(v["readReuse"]["allReturnedBefore"],false);
  assert_eq!(v["lines"][0]["text"],"code");
  let mut changed=row(3);changed["sha256"]=json!("v2");read_reuse("read_file",&mut changed,&history);
  assert!(changed.get("readReuse").is_none());
 }
}
