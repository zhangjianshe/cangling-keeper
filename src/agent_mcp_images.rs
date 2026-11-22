//! Validated, content-addressed MCP images. Bytes never enter tool logs or summaries.
use crate::store::Store;
use serde_json::{Value,json};
use base64::{Engine,engine::general_purpose::STANDARD};
use rusqlite::params;
pub(crate) fn initialize(store:&Store)->Result<(),String>{store.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_mcp_images(session_id TEXT NOT NULL,sha256 TEXT NOT NULL,image TEXT NOT NULL,PRIMARY KEY(session_id,sha256));").map_err(|e|e.to_string())}
pub(crate) fn validate(image:&Value)->Result<Value,String>{
    let mime=image["mimeType"].as_str().ok_or("MCP 图片缺少格式")?;
    if !matches!(mime,"image/png"|"image/jpeg"){return Err("MCP 图片仅支持 PNG/JPEG".into());}
    let data=image["data"].as_str().ok_or("MCP 图片缺少数据")?;
    if data.len()>1398104{return Err("MCP 单张图片最多 1 MiB".into());}
    let bytes=STANDARD.decode(data).map_err(|_|"MCP 图片 Base64 无效")?;
    if bytes.len()>1024*1024{return Err("MCP 单张图片最多 1 MiB".into());}
    let metadata=crate::agent_attachments::validate("mcp-image",mime,&bytes)?;
    if metadata["mimeType"]!=mime || metadata["kind"]!="image"{return Err("MCP 图片格式不匹配".into());}
    Ok(json!({"sha256":metadata["sha256"],"mimeType":mime,"size":metadata["size"],"width":metadata["width"],"height":metadata["height"],"data":STANDARD.encode(bytes)}))
}
pub(crate) fn metadata(image:&Value)->Value{let mut value=image.clone();value.as_object_mut().unwrap().remove("data");value}
pub(crate) fn extract(method:&str,response:&Value)->Result<Vec<Value>,String>{
    let mut blocks=Vec::new();
    match method {
        "tools/call"=>{if let Some(items)=response["content"].as_array(){for item in items{if item["type"]=="image"{blocks.push(item.clone());}}}},
        "resources/read"=>{if let Some(items)=response["contents"].as_array(){for item in items{if item["mimeType"].as_str().is_some_and(|m|m.starts_with("image/")){blocks.push(json!({"mimeType":item["mimeType"],"data":item["blob"]}));}}}},
        "prompts/get"=>{if let Some(items)=response["messages"].as_array(){for item in items{if item["content"]["type"]=="image"{blocks.push(item["content"].clone());}}}},_=>{}
    }
    if blocks.len()>4{return Err("MCP 每次最多返回4张图片".into());}
    blocks.iter().map(validate).collect()
}
pub(crate) fn save(store:&Store,session:&str,images:&[Value])->Result<Vec<Value>,String>{
    let mut refs=Vec::new();
    for image in images{let image=validate(image)?;store.conn.execute("INSERT OR IGNORE INTO agent_mcp_images VALUES (?1,?2,?3)",params![session,image["sha256"].as_str(),image.to_string()]).map_err(|e|e.to_string())?;refs.push(metadata(&image));}Ok(refs)
}
pub(crate) fn read(store:&Store,session:&str,hash:&str)->Result<Value,String>{
    if !crate::agent_account::owns(store,session){return Err("会话不属于当前账号".into());}
    let raw:String=store.conn.query_row("SELECT image FROM agent_mcp_images WHERE session_id=?1 AND sha256=?2",params![session,hash],|r|r.get(0)).map_err(|_|"MCP 图片不存在")?;
    let image:Value=serde_json::from_str(&raw).map_err(|_|"MCP 图片损坏")?;
    let checked=validate(&image)?;if checked!=image || image["sha256"]!=hash{return Err("MCP 图片校验失败".into());}Ok(image)
}
pub(crate) fn references(content:&str)->Vec<Value>{serde_json::from_str::<Value>(content).ok().and_then(|v|v["mcpImages"].as_array().cloned()).unwrap_or_default()}
pub(crate) fn export(store:&Store,session:&str)->Result<Vec<Value>,String>{let mut q=store.conn.prepare("SELECT image FROM agent_mcp_images WHERE session_id=?1 ORDER BY sha256").map_err(|e|e.to_string())?;q.query_map([session],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?.map(|r|serde_json::from_str(&r.map_err(|e|e.to_string())?).map_err(|_|"MCP 图片记录损坏".into())).collect()}

pub(crate) fn mark_placeholders(value:&mut Value){
    if value["unsupported"]==true && matches!(value["mimeType"].as_str(),Some("image/png"|"image/jpeg")){
        value["unsupported"]=json!(false);value["reason"]=json!("图片已单独校验保存；是否提供图像由模型能力与上下文预算决定。");
    }
    match value {Value::Object(items)=>{for item in items.values_mut(){mark_placeholders(item);}},Value::Array(items)=>{for item in items{mark_placeholders(item);}},_=>{}}
}

#[cfg(test)]
mod tests {
 use super::*;
 fn png()->Value{json!({"type":"image","mimeType":"image/png","data":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAAD0lEQVR4nGNQaDgAAAHcAWF9gB3zAAAAAElFTkSuQmCC"})}
 #[test] fn rejects_untrusted_formats_limits_and_invalid_data(){
  assert!(validate(&json!({"mimeType":"image/svg+xml","data":"PHN2Zy8+"})).is_err());
  assert!(validate(&json!({"mimeType":"image/png","data":"%%%"})).is_err());
  assert!(validate(&json!({"mimeType":"image/png","data":"A".repeat(1398105)})).is_err());
  assert!(extract("tools/call",&json!({"content":vec![png();5]})).is_err());
  assert!(validate(&json!({"mimeType":"image/jpeg","data":png()["data"]})).is_err());
 }
}
