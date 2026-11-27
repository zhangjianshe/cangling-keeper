//! Per-generation no-progress detection. Tool IDs and prose are not progress.
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Default)]
pub(super) struct LoopGuard {
    recent: std::collections::VecDeque<[u8; 32]>,
}
impl LoopGuard {
    /// Compare ordered batches, retaining only the last six result hashes.
    /// Three equal batches or three equal two-batch cycles indicate no progress.
    pub(super) fn observe(&mut self, batch: &Value) -> Result<(), String> {
        let fingerprint: [u8; 32] = Sha256::digest(batch.to_string().as_bytes()).into();
        if self.recent.len() == 6 { self.recent.pop_front(); }
        self.recent.push_back(fingerprint);
        for period in [1, 2] {
            let count = period * 3;
            if self.recent.len() < count { continue; }
            let start = self.recent.len() - count;
            if (period..count).all(|i| self.recent[start+i] == self.recent[start+i%period]) {
                let detail = if period == 1 { "连续 3 轮调用相同工具且结果未变化" }
                    else { "连续 6 轮重复相同的双步骤工具循环且结果未变化" };
                return Err(format!("Agent {detail}，已停止无进展循环；已生成内容和执行记录已保留。请调整任务或检查工具结果后重试。"));
            }
        }
        Ok(())
    }
}

pub(super) fn canonical(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.into()))
}

/// Advisory reuse metadata does not represent filesystem progress.
pub(super) fn result(tool:&str,text:&str)->Value {
    let mut value=canonical(text);
    if tool=="read_file" {if let Some(v)=value.as_object_mut(){v.remove("readReuse");}}
    if tool=="read_files" {if let Some(items)=value["results"].as_array_mut(){for item in items {if let Some(v)=item["result"].as_object_mut(){v.remove("readReuse");}}}}
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn read_advice_does_not_reset_no_progress_guard(){
        let mut guard=LoopGuard::default();
        for i in 0..3 {let mut v=json!({"text":"same","sha256":"v1"});if i>0{v["readReuse"]=json!({"allReturnedBefore":true});}let r=guard.observe(&json!([["read_file",{},result("read_file",&v.to_string())]]));assert_eq!(r.is_err(),i==2);}
        assert_ne!(result("read_file",r#"{"sha256":"v1"}"#),result("read_file",r#"{"sha256":"v2"}"#));
        assert!(result("run_command",r#"{"readReuse":"real output"}"#).get("readReuse").is_some());
    }
    #[test]
    fn identical_results_stop_on_third_round() {
        let mut guard=LoopGuard::default();let batch=json!([["read_file",canonical(r#"{"path":"a"}"#),{"text":"same"}]]);
        assert!(guard.observe(&batch).is_ok());assert!(guard.observe(&batch).is_ok());assert!(guard.observe(&batch).unwrap_err().contains("无进展"));
    }
    #[test]
    fn changed_result_or_arguments_reset_streak() {
        let mut guard=LoopGuard::default();
        for batch in [json!([1]),json!([1]),json!([2]),json!([2]),json!([3]),json!([3])] {assert!(guard.observe(&batch).is_ok());}
    }
    #[test]
    fn alternating_cycle_stops_after_three_complete_repetitions() {
        let mut guard=LoopGuard::default();
        for value in [1,2,1,2,1] {assert!(guard.observe(&json!([value])).is_ok());}
        assert!(guard.observe(&json!([2])).unwrap_err().contains("双步骤"));
        assert_eq!(guard.recent.len(),6);
    }
    #[test]
    fn changed_cycle_and_batch_order_do_not_count_as_repetition() {
        let mut guard=LoopGuard::default();
        for value in [1,2,1,2,3,2,1,2,1] {assert!(guard.observe(&json!([value])).is_ok());}
        for i in 0..30 {assert!(guard.observe(&json!([i,i+1])).is_ok());assert!(guard.recent.len()<=6);}
        let mut guard=LoopGuard::default();
        for value in [json!([1,2]),json!([1,2]),json!([2,1]),json!([1,2]),json!([2,1])] {assert!(guard.observe(&value).is_ok());}
    }
    #[test]
    fn json_spacing_and_key_order_do_not_bypass_guard() {
        assert_eq!(canonical(r#"{"b":2,"a":1}"#),canonical(r#"{ "a": 1, "b": 2 }"#));
        assert_eq!(canonical("plain output"),json!("plain output"));
    }
    #[test]
    fn new_generation_has_independent_budget() {
        for _ in 0..2 {let mut guard=LoopGuard::default();assert!(guard.observe(&json!([1])).is_ok());assert!(guard.observe(&json!([1])).is_ok());}
    }
}

/// A refusal applies to the exact tool and normalized JSON arguments within
/// this generation. It cannot be bypassed merely by changing a tool-call ID.
#[derive(Default)]
pub(super) struct RefusalGuard {
    rejected: std::collections::HashSet<[u8; 32]>,
}
impl RefusalGuard {
    fn key(name: &str, arguments: &str) -> [u8; 32] {
        Sha256::digest(serde_json::json!([name, canonical(arguments)]).to_string().as_bytes()).into()
    }
    pub(super) fn check(&self, name: &str, arguments: &str) -> Result<(), String> {
        if self.rejected.contains(&Self::key(name, arguments)) {
            Err("模型再次提出本轮已被你拒绝的相同操作，任务已停止；未再次请求批准或执行该操作。若希望重新尝试，请发送新的消息。".into())
        } else { Ok(()) }
    }
    pub(super) fn observe(&mut self, name: &str, arguments: &str, content: &str) {
        if serde_json::from_str::<Value>(content).ok().is_some_and(|v| v["keeperResult"]["outcome"] == "rejected") {
            self.rejected.insert(Self::key(name, arguments));
        }
    }
}
#[cfg(test)]
mod refusal_tests {
    use super::*;
    #[test]
    fn rejected_arguments_cannot_be_reordered_to_retry() {
        let mut guard=RefusalGuard::default();
        guard.observe("write_file",r#"{"path":"a","content":"b"}"#,r#"{"keeperResult":{"outcome":"rejected"}}"#);
        assert!(guard.check("write_file",r#"{ "content":"b", "path":"a" }"#).is_err());
        assert!(guard.check("write_file",r#"{"path":"c","content":"b"}"#).is_ok());
        assert!(guard.check("read_file",r#"{"path":"a","content":"b"}"#).is_ok());
        assert!(RefusalGuard::default().check("write_file",r#"{"path":"a","content":"b"}"#).is_ok());
    }
    #[test]
    fn failures_and_external_text_are_not_user_refusals() {
        let mut guard=RefusalGuard::default();
        for content in ["invalid",r#"{"rejected":true}"#,r#"{"keeperResult":{"outcome":"failed"}}"#] {
            guard.observe("run_command","{}",content);
        }
        assert!(guard.check("run_command","{}").is_ok());
    }
}

/// Conservative prose-loop detector; never inspect tool arguments or code blocks.
pub(super) fn repeated_prose(text: &str) -> bool {
    if text.len()<1200 || text.contains("```") || text.contains("~~~") {return false;}
    let parts:Vec<_>=text.split("\n\n").collect();
    let complete=&parts[..parts.len().saturating_sub(1)];
    let recent=&complete[complete.len().saturating_sub(12)..];
    recent.iter().any(|p| {
        let p=p.trim();
        p.chars().count()>=60 && !p.starts_with(['>','|']) && recent.iter().filter(|q|q.trim()==p).count()>=3
    })
}
#[cfg(test)] mod prose_tests {
 use super::*;
 #[test] fn stops_repeated_long_prose_but_not_code_quotes_or_short_lists(){
  let p="这是一个反复出现且没有新增证据的分析段落。".repeat(8);
  assert!(repeated_prose(&format!("{p}\n\n{p}\n\n{p}\n\n")));
  assert!(!repeated_prose(&format!("```text\n{p}\n\n{p}\n\n{p}\n\n```")));
  assert!(!repeated_prose(&format!(">{p}\n\n>{p}\n\n>{p}\n\n")));
  assert!(!repeated_prose(&"OK\n\n".repeat(500)));
  assert!(!repeated_prose(&(0..30).map(|i|format!("{i}{p}\n\n")).collect::<String>()));
 }
}
