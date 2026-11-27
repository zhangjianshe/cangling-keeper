//! Validate the structural subset used by built-in tool declarations.
//! Path restrictions, skill-name patterns and semantic checks stay in handlers.
use serde_json::Value;
pub(crate) fn validate(name: &str, args: &Value) -> Result<(),String> {
 let definition=crate::agent_files::definitions().into_iter().find(|d|d["function"]["name"]==name).ok_or("工具未注册，未执行")?;
 check(&definition["function"]["parameters"],args,"参数")?;
 if name=="search_files" && (args.get("query").is_some()==args.get("pattern").is_some()){return Err("未执行：query（字面字符串）与兼容别名pattern必须且只能提供一个，例如 {query:函数名,path:文件路径,mode:content}".into());}
 if name=="apply_patch" {
  if args.get("expectedOccurrences").is_some() && (args.get("expectedSha256").is_none() || ["edits","startLine","endLine","newLines"].iter().any(|k|args.get(k).is_some())){return Err("计数替换必须提供expectedSha256，且不能与其他补丁模式混用".into());}
  if ["startLine","endLine","newLines"].iter().any(|k|args.get(k).is_some()){
   for k in ["startLine","endLine","newLines","expectedSha256"] {if args.get(k).is_none(){return Err(format!("行补丁缺少 {k}"));}}
   if ["oldText","newText","edits"].iter().any(|k|args.get(k).is_some()){return Err("行补丁不能与文本替换混用".into());}
  }else if args.get("edits").is_some(){
   if args.get("oldText").is_some()||args.get("newText").is_some(){return Err("未执行：edits 与 oldText/newText 不能混用；单次修改请用 path、oldText、newText、expectedSha256。".into());}
  }else{
   for key in ["oldText","newText"]{if args.get(key).is_none(){return Err(format!("未执行：缺少 {key}；单次补丁格式为 path、oldText、newText、expectedSha256，newText 允许空字符串。"));}}
  }
 }
 Ok(())
}
fn check(schema: &Value, value: &Value, field: &str) -> Result<(),String> {
 let fail=|message:&str|format!("工具参数错误：{field} {message}；请修正参数后再调用，未执行操作。");
 let valid=match schema["type"].as_str(){Some("object")=>value.is_object(),Some("array")=>value.is_array(),Some("string")=>value.is_string(),Some("boolean")=>value.is_boolean(),Some("number")=>value.as_f64().is_some_and(|n|n.is_finite()),Some("integer")=>value.as_i64().is_some()||value.as_u64().is_some(),_=>false};
 if !valid{return Err(fail(&format!("须为 {}",schema["type"].as_str().unwrap_or("有效类型"))));}
 if let Some(values)=schema["enum"].as_array(){if !values.contains(value){return Err(fail(&format!("须为以下值之一：{}",schema["enum"])));}}
 if let Some(text)=value.as_str(){let count=text.chars().count() as u64;
  if schema["minLength"].as_u64().is_some_and(|min|count<min)||schema["maxLength"].as_u64().is_some_and(|max|count>max){return Err(fail(&format!("字符长度须在 {} 到 {} 之间",schema["minLength"].as_u64().unwrap_or(0),schema["maxLength"].as_u64().map(|v|v.to_string()).unwrap_or("不限".into()))));}
 }
 if let Some(number)=value.as_f64(){if schema["minimum"].as_f64().is_some_and(|min|number<min)||schema["maximum"].as_f64().is_some_and(|max|number>max){return Err(fail(&format!("数值须在 {} 到 {} 之间",schema["minimum"].as_f64().map(|v|v.to_string()).unwrap_or("不限".into()),schema["maximum"].as_f64().map(|v|v.to_string()).unwrap_or("不限".into()))));}}
 if let Some(items)=value.as_array(){let count=items.len() as u64;
  if schema["minItems"].as_u64().is_some_and(|min|count<min)||schema["maxItems"].as_u64().is_some_and(|max|count>max){return Err(fail(&format!("条目数量须在 {} 到 {} 之间",schema["minItems"].as_u64().unwrap_or(0),schema["maxItems"].as_u64().map(|v|v.to_string()).unwrap_or("不限".into()))));}
  for (index,item) in items.iter().enumerate(){check(&schema["items"],item,&format!("{field}[{index}]"))?;}
 }
 if let Some(object)=value.as_object(){
  if let Some(required)=schema["required"].as_array(){for key in required.iter().filter_map(Value::as_str){if !object.contains_key(key){return Err(fail(&format!("缺少必填字段 {key}")));}}}
  for (key,item) in object{if let Some(child)=schema["properties"].get(key){check(child,item,&format!("{field}.{key}"))?;}else if schema["additionalProperties"]==false{return Err(fail(&format!("包含未声明字段 {key}，请删除该字段；允许字段：{}",schema["properties"].as_object().map(|p|p.keys().cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default())));}}
 }
 Ok(())
}
#[cfg(test)]
mod tests{
 use super::*;use serde_json::json;
 #[test]fn simple_patch_requires_both_text_fields(){assert!(validate("apply_patch",&json!({"path":"a","oldText":"x"})).unwrap_err().contains("newText"));assert!(validate("apply_patch",&json!({"path":"a","oldText":"x","newText":""})).is_ok());}
 #[test]fn rejects_missing_wrong_types_and_extra_fields(){
  for args in [json!({}),json!({"path":5}),json!({"path":"a","content":false}),json!({"path":"a","content":"ok","execute":true})]{assert!(validate("write_file",&args).is_err());}
  assert!(validate("write_file",&json!({"path":"a","content":""})).is_ok());
  assert!(validate("list_directory",&json!({})).is_ok());
  assert!(validate("not_registered",&json!({})).is_err());
 }
 #[test]fn enforces_integer_bounds_and_unicode_lengths(){
  for value in [json!(0),json!(201),json!(1.5),json!("10")]{assert!(validate("read_file",&json!({"path":"a","lineCount":value})).is_err());}
  assert!(validate("read_file",&json!({"path":"a","lineCount":200})).is_ok());
  assert!(validate("search_files",&json!({"query":"文".repeat(200)})).is_ok());
  assert!(validate("search_files",&json!({"query":"文".repeat(201)})).is_err());
 }
 #[test]fn validates_nested_plans(){
  assert!(validate("update_plan",&json!({"plan":[{"step":"a","status":"done"}]})).is_err());
  assert!(validate("update_plan",&json!({"plan":[{"step":"a","status":"pending"}]})).is_ok());
  assert!(validate("update_plan",&json!({"plan":[]})).is_err());
 }
}
