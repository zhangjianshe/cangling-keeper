//! Model-reported task plan. This never executes work or grants approval.
use serde::Deserialize;
use serde_json::{json, Value};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan { plan: Vec<Step>, #[serde(default)] explanation: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step { step: String, status: String }
pub(crate) fn definition() -> Value {
 json!({"type":"function","function":{"name":"update_plan","description":"更新当前任务的完整步骤清单（1–12步），最多一个进行中步骤。状态 pending/in_progress/completed。计划只是模型报告，不执行工作、不授予权限；完成状态须依据真实执行结果。简单问答无需计划。","parameters":{"type":"object","properties":{"plan":{"type":"array","minItems":1,"maxItems":12,"items":{"type":"object","properties":{"step":{"type":"string","maxLength":200},"status":{"type":"string","enum":["pending","in_progress","completed"]}},"required":["step","status"],"additionalProperties":false}},"explanation":{"type":"string","maxLength":1000}},"required":["plan"],"additionalProperties":false}}})
}
pub(crate) fn update(args: Value) -> Result<Value,String> {
 let plan: Plan=serde_json::from_value(args).map_err(|_|"计划参数格式无效")?;
 if plan.plan.is_empty() || plan.plan.len()>12 || plan.explanation.chars().count()>1000 {return Err("计划须为1–12步，说明最多1000字符".into());}
 let mut active=0;let mut steps=Vec::new();
 for item in plan.plan {
  let text=item.step.trim();
  if text.is_empty() || text.chars().count()>200 || text.chars().any(char::is_control) {return Err("步骤须为1–200字符的单行文字".into());}
  match item.status.as_str(){"in_progress"=>active+=1,"pending"|"completed"=>{},_=>return Err("无效的步骤状态".into())}
  steps.push(json!({"step":text,"status":item.status}));
 }
 if active>1 {return Err("同时只能有一个进行中步骤".into());}
 Ok(json!({"plan":steps,"explanation":plan.explanation,"modelReported":true}))
}
#[cfg(test)]
mod tests {
 use super::*;
 #[test] fn accepts_progress_and_completion(){for status in ["pending","in_progress","completed"]{let result=update(json!({"plan":[{"step":"Read files","status":status}]})).unwrap();assert_eq!(result["plan"][0]["status"],status);}}
 #[test] fn rejects_invalid_and_over_budget_plans(){
  for args in [json!({"plan":[]}),json!({"plan":[{"step":" ","status":"pending"}]}),json!({"plan":[{"step":"x","status":"done"}]}),json!({"plan":[{"step":"a","status":"in_progress"},{"step":"b","status":"in_progress"}]}),json!({"plan":[{"step":"x".repeat(201),"status":"pending"}]}),json!({"plan":vec![json!({"step":"x","status":"pending"});13]}),json!({"plan":[{"step":"x","status":"pending"}],"command":"not allowed"})]{assert!(update(args).is_err());}
 }
}
