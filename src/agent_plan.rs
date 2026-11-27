//! Model-reported task plan. This never executes work or grants approval.
use serde::Deserialize;
use serde_json::{json, Value};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan { plan: Vec<Step>, #[serde(default)] explanation: String, #[serde(default)] acceptance: Vec<Criterion> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step { step: String, status: String, #[serde(default)] reason: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Criterion { requirement:String, check:String, status:String, #[serde(default)] evidence:String }
pub(crate) fn definition() -> Value {
 json!({"type":"function","function":{"name":"update_plan","description":"仅为需要跟踪实质阶段的复杂任务使用本工具。简单问答及少量文件的小修复，即使需要读写和测试几个工具步骤，通常无需计划；用户明确要求计划时遵从。用户指定阶段数量或内容时必须按用户要求，不得扩展成逐文件步骤；未指定时建议3–5个实质阶段（允许1–12步）。一个实现阶段可包含多个文件、函数和多次工具调用，不要按文件或单次工具拆分步骤；例如实现与集成、验证、收尾三个阶段。初始化直接将第一阶段设为 in_progress；完成实质阶段或方案变化时合并更新，禁止每次读写前后单独更新，未变化不调用。验证完成后一次合并已核实的最终状态，再给文字总结，不要为标记总结已完成额外调用。更新当前任务的完整步骤清单（1–12步），最多一个进行中步骤。状态 pending/in_progress/completed/blocked/skipped；受阻或跳过必须提供单行 reason（1–300字符）。计划只是模型报告，不执行工作、不授予权限；完成状态须依据真实执行结果。复杂开发同时提供 acceptance 需求验收清单：逐条 requirement、check、status(pending/passed/failed/unverified)、evidence；通过或失败须给实际证据；缺少证据仅记录为 unverified，不算验收通过。日常推进只提交 plan；acceptance 仅初始化或取得新证据时提交，省略会保留原清单。模型报告并非系统认证。每次提交完整步骤清单，不能因测试子集通过删去未覆盖需求。简单问答无需计划。","parameters":{"type":"object","properties":{"plan":{"type":"array","minItems":1,"maxItems":12,"items":{"type":"object","properties":{"step":{"type":"string","maxLength":200},"status":{"type":"string","enum":["pending","in_progress","completed","blocked","skipped"]},"reason":{"type":"string","maxLength":300}},"required":["step","status"],"additionalProperties":false}},"acceptance":{"type":"array","maxItems":20,"items":{"type":"object","properties":{"requirement":{"type":"string","minLength":1,"maxLength":300},"check":{"type":"string","minLength":1,"maxLength":300},"status":{"type":"string","enum":["pending","passed","failed","unverified","completed"]},"evidence":{"type":"string","maxLength":500}},"required":["requirement","check","status"],"additionalProperties":false}},"explanation":{"type":"string","maxLength":1000}},"required":["plan"],"additionalProperties":false}}})
}
pub(crate) fn update(args: Value) -> Result<Value,String> {
 let plan: Plan=serde_json::from_value(args).map_err(|_|"计划参数格式无效")?;
 if plan.plan.is_empty() || plan.plan.len()>12 || plan.explanation.chars().count()>1000 {return Err("计划须为1–12步，说明最多1000字符".into());}
 let mut active=0;let mut steps=Vec::new();
 for item in plan.plan {
  let text=item.step.trim();
  if text.is_empty() || text.chars().count()>200 || text.chars().any(char::is_control) {return Err("步骤须为1–200字符的单行文字".into());}
  match item.status.as_str(){"in_progress"=>active+=1,"pending"|"completed"|"blocked"|"skipped"=>{},_=>return Err("无效的步骤状态".into())}
  let reason=item.reason.trim();
  if reason.chars().count()>300 || reason.chars().any(char::is_control) {return Err("步骤原因须为最多300字符的单行文字".into());}
  if matches!(item.status.as_str(),"blocked"|"skipped") && reason.is_empty() {return Err("受阻或跳过的步骤必须说明原因".into());}
  let mut step=json!({"step":text,"status":item.status});
  if !reason.is_empty(){step["reason"]=json!(reason);}
  steps.push(step);
 }
 if active>1 {return Err("同时只能有一个进行中步骤".into());}
 if plan.acceptance.len()>20{return Err("验收清单最多20项".into());}
 let mut acceptance=Vec::new();
 for item in plan.acceptance {
  for (text,max) in [(&item.requirement,300),(&item.check,300),(&item.evidence,500)]{if text.chars().count()>max||text.chars().any(char::is_control){return Err("验收字段须为限长单行文字".into());}}
  if item.requirement.trim().is_empty()||item.check.trim().is_empty()||!matches!(item.status.as_str(),"pending"|"passed"|"failed"|"unverified"|"completed"){return Err("验收项缺少需求、验证方法或有效状态".into());}
  let status=if item.status=="completed" || matches!(item.status.as_str(),"passed"|"failed")&&item.evidence.trim().is_empty(){"unverified"}else{item.status.as_str()};
  acceptance.push(json!({"requirement":item.requirement,"check":item.check,"status":status,"evidence":item.evidence,"modelReported":true}));
 }
 Ok(json!({"acceptance":acceptance,"plan":steps,"explanation":plan.explanation,"modelReported":true}))
}
#[cfg(test)]
mod tests {
 use super::*;
 #[test] fn acceptance_requires_evidence_and_stays_model_reported(){
 let mut v=json!({"plan":[{"step":"implement","status":"pending"}],"acceptance":[{"requirement":"errors are JSON","check":"HTTP invalid request","status":"passed"}]});assert_eq!(update(v.clone()).unwrap()["acceptance"][0]["status"],"unverified");v["acceptance"][0]["evidence"]=json!("run abc: 12 tests passed");let r=update(v).unwrap();assert_eq!(r["acceptance"][0]["modelReported"],true);
 }
 #[test] fn accepts_progress_and_completion(){for status in ["pending","in_progress","completed"]{let result=update(json!({"plan":[{"step":"Read files","status":status}]})).unwrap();assert_eq!(result["plan"][0]["status"],status);}}
 #[test] fn rejects_invalid_and_over_budget_plans(){
  for args in [json!({"plan":[]}),json!({"plan":[{"step":" ","status":"pending"}]}),json!({"plan":[{"step":"x","status":"done"}]}),json!({"plan":[{"step":"a","status":"in_progress"},{"step":"b","status":"in_progress"}]}),json!({"plan":[{"step":"x".repeat(201),"status":"pending"}]}),json!({"plan":vec![json!({"step":"x","status":"pending"});13]}),json!({"plan":[{"step":"x","status":"pending"}],"command":"not allowed"})]{assert!(update(args).is_err());}
 }
 #[test] fn blocked_and_skipped_require_bounded_reasons(){
  for status in ["blocked","skipped"] {
   for reason in ["", "  ", "a\nb"] {assert!(update(json!({"plan":[{"step":"Deploy","status":status,"reason":reason}]})).is_err());}
   assert!(update(json!({"plan":[{"step":"Deploy","status":status,"reason":"中".repeat(301)}]})).is_err());
   let result=update(json!({"plan":[{"step":"Deploy","status":status,"reason":" 等待测试环境 "}]})).unwrap();
   assert_eq!(result["plan"][0]["reason"],"等待测试环境");assert_eq!(result["modelReported"],true);
  }
  let result=update(json!({"plan":[{"step":"Read","status":"in_progress"},{"step":"Deploy","status":"blocked","reason":"等待环境"},{"step":"Publish","status":"skipped","reason":"本轮不发布"}]})).unwrap();
  assert!(result["plan"][0].get("reason").is_none());assert_eq!(result["plan"].as_array().unwrap().len(),3);
 }

}

/// Feedback only: keep valid plans and all audit records, never expand permissions/budgets.
pub(crate) fn feedback(current:&mut Value,previous:Option<&Value>,work_since:bool){
 let unchanged=previous.is_some_and(|p|p["plan"]==current["plan"]&&p["acceptance"]==current["acceptance"]);
 current["unchanged"]=json!(unchanged);
 let all_pending=current["plan"].as_array().is_some_and(|steps|!steps.is_empty()&&steps.iter().all(|step|step["status"]=="pending"));
 current["nextAction"]=json!(if unchanged {"计划未发生实质变化，不必重复更新；继续执行当前步骤。"}else if previous.is_none()&&all_pending {"初始计划已保存；直接执行第一阶段，不要仅为把 pending 改成 in_progress 再单独调用。取得实际阶段结果后合并更新；同一实现阶段可连续处理多个文件和工具。"}else if previous.is_some()&&!work_since {"两次计划之间没有实际工具执行；若不是受阻或方案调整，请直接推进任务，不要用计划调用代替执行。"}else{"计划已记录；继续实际工作。同一实现阶段可连续处理多个文件和工具，不要每次读写后更新。验证结束后合并已核实的最终阶段状态，再给文字总结；不要为标记总结已完成额外调用。取得新验证证据后再更新 acceptance，缺证据仍为 unverified。"});
}
#[cfg(test)] mod feedback_tests {
 use super::*;
 #[test] fn initial_pending_plan_gets_actionable_feedback_without_status_rewrite(){
  let mut plan=update(json!({"plan":[{"step":"Implementation","status":"pending"},{"step":"Verify","status":"pending"}]})).unwrap();let before=plan["plan"].clone();
  feedback(&mut plan,None,false);
  assert!(plan["nextAction"].as_str().unwrap().contains("直接执行第一阶段"));
  assert_eq!(plan["plan"],before);assert_eq!(plan["unchanged"],false);
  let old=plan.clone();feedback(&mut plan,Some(&old),false);
  assert_eq!(plan["unchanged"],true);assert!(plan["nextAction"].as_str().unwrap().contains("不必重复更新"));
 }
 #[test] fn compares_content_not_explanation_and_preserves_changes(){let old=update(json!({"plan":[{"step":"实现","status":"in_progress"}]})).unwrap();let mut same=old.clone();same["explanation"]=json!("new wording");feedback(&mut same,Some(&old),false);assert_eq!(same["unchanged"],true);let mut next=old.clone();next["plan"][0]["status"]=json!("completed");feedback(&mut next,Some(&old),true);assert_eq!(next["unchanged"],false);assert_eq!(next["plan"][0]["status"],"completed");}
}

#[cfg(test)] mod unverified_plan_tests {
 use super::*;
 #[test] fn completed_alias_and_missing_evidence_never_certify_acceptance(){for status in ["completed","passed","failed"]{let v=update(json!({"plan":[{"step":"实现","status":"completed"}],"acceptance":[{"requirement":"功能","check":"测试","status":status}]})).unwrap();assert_eq!(v["acceptance"][0]["status"],"unverified");assert_eq!(v["modelReported"],true);}}
}
