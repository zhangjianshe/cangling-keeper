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
