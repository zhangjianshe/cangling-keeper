//! Per-generation no-progress detection. Tool IDs and prose are not progress.
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Default)]
pub(super) struct LoopGuard {
    previous: Option<[u8; 32]>,
    repeats: usize,
}
impl LoopGuard {
    /// Compare whole, ordered tool batches. Changed results or arguments reset
    /// the streak; only hashes are retained, never another copy of tool output.
    pub(super) fn observe(&mut self, batch: &Value) -> Result<(), String> {
        let fingerprint: [u8; 32] = Sha256::digest(batch.to_string().as_bytes()).into();
        self.repeats = if self.previous == Some(fingerprint) { self.repeats + 1 } else { 1 };
        self.previous = Some(fingerprint);
        if self.repeats >= 3 {
            return Err("Agent 连续 3 轮调用相同工具且结果未变化，已停止无进展循环；已生成内容和执行记录已保留。请调整任务或检查工具结果后重试。".into());
        }
        Ok(())
    }
}

pub(super) fn canonical(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
