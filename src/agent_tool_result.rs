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
    let read_only=matches!(tool,"list_directory"|"read_file"|"search_files"|"load_skill"|"read_skill_file");
    let advice=match outcome {
        "rejected"=>"用户已拒绝，不要再次请求相同操作。",
        "timeout"|"stopped"=>"先核实当前状态；停止或超时不保证操作未发生，不要自动重复执行。",
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
    #[test] fn timeout_is_not_safe_to_retry() {let mut v=json!({"timedOut":true,"exitCode":null,"stdout":"partial"});annotate("run_command",&mut v,"failed");assert_eq!(v["keeperResult"]["outcome"],"timeout");assert_eq!(v["keeperResult"]["automaticRetry"],false);assert_eq!(v["stdout"],"partial");}
    #[test] fn truncation_is_success_with_incomplete_output() {for field in ["truncated","outputTruncated"] {let mut v=json!({});v[field]=json!(true);annotate("read_file",&mut v,"succeeded");assert_eq!(v["keeperResult"]["outcome"],"truncated");assert_eq!(v["keeperResult"]["status"],"succeeded");}}
    #[test] fn failed_and_rejected_are_distinct() {for status in ["failed","rejected","stopped"] {let mut v=json!({"error":"fixture"});annotate("external_tool",&mut v,status);assert_eq!(v["keeperResult"]["outcome"],status);assert_eq!(v["error"],"fixture");}}
    #[test] fn metadata_is_owned_and_does_not_remove_images() {let mut v=json!({"keeperResult":{"automaticRetry":true},"mcpImages":[{"sha256":"fixture"}]});annotate("mcp_tool",&mut v,"succeeded");assert_eq!(v["keeperResult"]["automaticRetry"],false);assert_eq!(v["mcpImages"][0]["sha256"],"fixture");}
    #[test] fn oversized_text_is_utf8_safe_and_explicit() {let mut v=json!({"output":"中文\n".repeat(30000),"mcpImages":[{"sha256":"keep"}]});annotate("mcp_tool",&mut v,"succeeded");assert!(v.to_string().len()<48000);assert_eq!(v["keeperResult"]["truncated"],true);assert_eq!(v["mcpImages"][0]["sha256"],"keep");}
    #[test] fn scalar_payloads_remain_available() {let mut v=json!([1,2]);annotate("tool",&mut v,"succeeded");assert_eq!(v["output"],json!([1,2]));}
}
