//! DeepSeek Chat Completions. Local configuration is never included in sync payloads.
pub(crate) const SYSTEM_PROMPT: &str = "你是苍灵智能助手。用用户使用的语言回答。你只能提供文字回复，没有自动执行工具或访问本机文件的能力；不能声称已经执行命令。需要操作主机时，请用户通过界面的工具入口操作，远程命令必须由用户确认。";
use crate::store::Store;
use serde_json::{json, Value};
const CONFIG: &str = "agent_deepseek_config";
const KEY: &str = "agent_deepseek_api_key";
const DEFAULT_URL: &str = "https://api.deepseek.com";
const DEFAULT_MODEL: &str = "deepseek-flash";

pub(crate) fn initialize(store:&Store)->Result<(),String>{
    store.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_llm_servers (id TEXT PRIMARY KEY,name TEXT NOT NULL,provider TEXT NOT NULL,base_url TEXT NOT NULL,model TEXT NOT NULL,api_key TEXT NOT NULL DEFAULT '',enabled INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL);").map_err(|e|e.to_string())?;
    if store.get_setting("agent_llm_migrated")?.is_none(){
        let tx=store.conn.unchecked_transaction().map_err(|e|e.to_string())?;
        let old=store.get_setting(CONFIG)?;let key=store.get_setting(KEY)?.unwrap_or_default();
        if old.is_some() || !key.is_empty(){
            let c:Value=old.and_then(|v|serde_json::from_str(&v).ok()).unwrap_or(json!({}));
            store.conn.execute("INSERT OR IGNORE INTO agent_llm_servers VALUES ('legacy-deepseek','DeepSeek','deepseek',?1,?2,?3,?4,?5)",rusqlite::params![c["baseUrl"].as_str().unwrap_or(DEFAULT_URL),c["model"].as_str().unwrap_or(DEFAULT_MODEL),key,c["enabled"].as_bool().unwrap_or(false),chrono::Utc::now().timestamp_millis()]).map_err(|e|e.to_string())?;
            store.set_setting("agent_llm_active","legacy-deepseek")?;
        }
        store.delete_setting(CONFIG)?;store.delete_setting(KEY)?;store.set_setting("agent_llm_migrated","1")?;tx.commit().map_err(|e|e.to_string())?;
    }
    Ok(())
}
pub(crate) fn config(store:&Store)->Result<Value,String>{
    initialize(store)?;
    let mut stmt=store.conn.prepare("SELECT id,name,provider,base_url,model,enabled,length(api_key)>0 FROM agent_llm_servers ORDER BY created_at,rowid").map_err(|e|e.to_string())?;
    let servers=stmt.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"provider":r.get::<_,String>(2)?,"baseUrl":r.get::<_,String>(3)?,"model":r.get::<_,String>(4)?,"enabled":r.get::<_,bool>(5)?,"hasApiKey":r.get::<_,bool>(6)?}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    let active=store.get_setting("agent_llm_active")?.unwrap_or_default();
    let mut result=servers.iter().find(|v|v["id"]==active).cloned().unwrap_or(json!({"enabled":false,"hasApiKey":false,"baseUrl":DEFAULT_URL,"model":DEFAULT_MODEL}));
    result["activeServerId"]=json!(active);result["servers"]=json!(servers);Ok(result)
}
pub(crate) fn session_config(store:&Store,session:&str)->Result<Value,String>{
    let all=config(store)?;
    let selected:String=store.conn.query_row("SELECT server_id FROM agent_sessions WHERE id=?1",[session],|r|r.get(0)).map_err(|e|e.to_string())?;
    let mut result=all["servers"].as_array().unwrap().iter().find(|s|s["id"]==selected).cloned().unwrap_or(json!({"enabled":false,"hasApiKey":false,"name":"模型不可用"}));
    result["activeServerId"]=json!(selected);result["defaultServerId"]=all["activeServerId"].clone();result["servers"]=all["servers"].clone();Ok(result)
}
pub(crate) fn connected(store:&Store)->bool { config(store).map(|c| c["enabled"]==true && c["hasApiKey"]==true).unwrap_or(false) }
fn base(value:&str)->Result<String,String> {
    let url=reqwest::Url::parse(value.trim()).map_err(|_|"API 地址无效")?;
    let local=matches!(url.host_str(),Some("127.0.0.1"|"localhost"|"[::1]"));
    if (url.scheme()!="https" && !(url.scheme()=="http" && local)) || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some(){return Err("API 地址需要 HTTPS；本机测试允许 HTTP。不允许包含凭证或查询参数。".into());}
    Ok(url.to_string().trim_end_matches('/').to_owned())
}
pub(crate) fn save(store:&Store,args:&Value)->Result<Value,String>{
    let all=config(store)?;
    let supplied_id=args["id"].as_str().unwrap_or("");
    let id=if supplied_id.is_empty(){if args.get("name").is_none(){all["activeServerId"].as_str().filter(|s|!s.is_empty()).map(str::to_owned).unwrap_or_else(||uuid::Uuid::new_v4().to_string())}else{uuid::Uuid::new_v4().to_string()}}else{supplied_id.to_owned()};
    let previous=all["servers"].as_array().unwrap().iter().find(|v|v["id"]==id);
    if !supplied_id.is_empty() && previous.is_none(){return Err("LLM Server 不存在".into());}
    let name=args["name"].as_str().unwrap_or_else(||previous.and_then(|v|v["name"].as_str()).unwrap_or("DeepSeek")).trim();
    if name.is_empty() || name.chars().count()>80 || name.chars().any(char::is_control){return Err("服务名称不能为空或超过80字".into());}
    let provider=args["provider"].as_str().unwrap_or("deepseek");
    if !matches!(provider,"deepseek"|"openai-compatible"){return Err("不支持的 API 类型".into());}
    let url=base(args["baseUrl"].as_str().unwrap_or(DEFAULT_URL))?;
    let model=args["model"].as_str().unwrap_or(DEFAULT_MODEL).trim();
    if model.is_empty() || model.len()>128 || model.chars().any(char::is_control){return Err("模型名称无效".into());}
    let enabled=args["enabled"].as_bool().unwrap_or(false);let supplied=args["apiKey"].as_str().unwrap_or("").trim();
    if supplied.len()>4096 || supplied.chars().any(char::is_control){return Err("API Key 无效".into());}
    let key=if args["clearApiKey"].as_bool().unwrap_or(false){String::new()}else if !supplied.is_empty(){supplied.to_owned()}else if previous.is_some_and(|v|v["baseUrl"]==url){store.conn.query_row("SELECT api_key FROM agent_llm_servers WHERE id=?1",[&id],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?}else{String::new()};
    if enabled && key.is_empty(){return Err("启用前请填写 API Key；更换 API 地址需要重新输入密钥。".into());}
    let tx=store.conn.unchecked_transaction().map_err(|e|e.to_string())?;
    store.conn.execute("INSERT INTO agent_llm_servers VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET name=excluded.name,provider=excluded.provider,base_url=excluded.base_url,model=excluded.model,api_key=excluded.api_key,enabled=excluded.enabled",rusqlite::params![id,name,provider,url,model,key,enabled,chrono::Utc::now().timestamp_millis()]).map_err(|e|e.to_string())?;
    if all["activeServerId"].as_str().unwrap_or("").is_empty(){store.set_setting("agent_llm_active",&id)?;}
    tx.commit().map_err(|e|e.to_string())?;let mut result=config(store)?;result["savedServerId"]=json!(id);Ok(result)
}
pub(crate) fn select(store:&Store,id:&str)->Result<Value,String>{
    let all=config(store)?;let server=all["servers"].as_array().unwrap().iter().find(|s|s["id"]==id).ok_or("LLM Server 不存在")?;
    if server["hasApiKey"]!=true{return Err("请先为该服务配置 API Key".into());}
    let tx=store.conn.unchecked_transaction().map_err(|e|e.to_string())?;
    store.conn.execute("UPDATE agent_llm_servers SET enabled=1 WHERE id=?1",[id]).map_err(|e|e.to_string())?;store.set_setting("agent_llm_active",id)?;tx.commit().map_err(|e|e.to_string())?;config(store)
}
fn client()->Result<reqwest::Client,String>{
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).connect_timeout(std::time::Duration::from_secs(15)).timeout(std::time::Duration::from_secs(120)).build().map_err(|_|"无法创建模型连接".into())
}
fn credentials_for(store:&Store,id:Option<&str>)->Result<(String,String,String,String),String>{
    let all=config(store)?;let id=id.unwrap_or_else(||all["activeServerId"].as_str().unwrap_or(""));
    let c=all["servers"].as_array().unwrap().iter().find(|v|v["id"]==id).ok_or("请先添加并选择 LLM Server")?;
    let key:String=store.conn.query_row("SELECT api_key FROM agent_llm_servers WHERE id=?1",[id],|r|r.get(0)).map_err(|e|e.to_string())?;
    if key.is_empty(){return Err("请先保存 API Key".into());}
    Ok((base(c["baseUrl"].as_str().unwrap_or(DEFAULT_URL))?,c["model"].as_str().unwrap_or(DEFAULT_MODEL).to_owned(),key,c["provider"].as_str().unwrap_or("deepseek").to_owned()))
}
fn status_error(status:reqwest::StatusCode)->String {
    match status.as_u16(){401|403=>"LLM 服务认证失败，请检查 API Key 和权限".into(),402=>"LLM 服务余额不足".into(),429=>"LLM 服务请求过于频繁，请稍后重试".into(),_=>format!("LLM 服务请求失败（HTTP {}）",status.as_u16())}
}
pub(crate) async fn test(core:&crate::AppState,id:Option<&str>)->Result<Value,String>{
    let (url,_,key,_)={let store=core.store.lock().map_err(|e|e.to_string())?;credentials_for(&store,id)?};
    let response=client()?.get(format!("{url}/models")).bearer_auth(key).send().await.map_err(|_|"无法连接 LLM Server，请检查地址、代理或网络")?;
    if !response.status().is_success(){return Err(status_error(response.status()));}
    let data:Value=response.json().await.map_err(|_|"模型列表响应格式错误")?;
    let models=data["data"].as_array().ok_or("模型列表响应格式错误")?.iter().filter_map(|v|v["id"].as_str().map(str::to_owned)).collect::<Vec<_>>();
    Ok(json!({"models":models}))
}
fn text_field(value:&Value,key:&str)->Result<String,String>{value[key].as_str().filter(|s|!s.is_empty()).map(str::to_owned).ok_or_else(||format!("原回复缺少 {key}"))}
#[derive(Debug,PartialEq)]
pub(crate) enum ReplyFinish { Complete, Stopped }
pub(crate) async fn reply(core:&crate::AppState,session:&str,response_id:&str,mut cancel:tokio::sync::oneshot::Receiver<()>,mut progress:impl FnMut(&str)->Result<(),String>+Send)->Result<ReplyFinish,String>{
    let (url,model,key,provider,messages)={
        let store=core.store.lock().map_err(|e|e.to_string())?;
        let raw:String=store.conn.query_row("SELECT model_metadata FROM agent_messages WHERE id=?1 AND session_id=?2",rusqlite::params![response_id,session],|r|r.get(0)).map_err(|e|e.to_string())?;
        let mut metadata:Value=serde_json::from_str(&raw).map_err(|_|"模型信息读取失败")?;
        let original=metadata["regenerateOf"].as_str().map(str::to_owned);
        let selected:String=if original.is_some(){text_field(&metadata,"serverId")?}else{store.conn.query_row("SELECT server_id FROM agent_sessions WHERE id=?1",[session],|r|r.get(0)).map_err(|e|e.to_string())?};
        let (url,current_model,key,provider)=credentials_for(&store,Some(&selected))?;
        let config=config(&store)?;let server=config["servers"].as_array().unwrap().iter().find(|s|s["id"]==selected && s["enabled"]==true && s["hasApiKey"]==true).ok_or("模型服务不可用")?;
        let model=if original.is_some(){
            if metadata.get("baseUrl").is_some_and(|v|v!=&json!(url)) || metadata.get("provider").is_some_and(|v|v!=&json!(provider)){return Err("原模型服务配置已变更，未发送重新生成请求。".into());}
            text_field(&metadata,"requestedModel")?
        }else{current_model};
        if original.is_none(){metadata=json!({"source":"llm","generationStatus":"streaming","serverId":selected,"serverName":server["name"],"baseUrl":url,"provider":provider,"requestedModel":model,"model":model});}
        store.conn.execute("UPDATE agent_messages SET model_metadata=?2 WHERE id=?1",rusqlite::params![response_id,metadata.to_string()]).map_err(|e|e.to_string())?;
        let saved_context=store.conn.query_row("SELECT messages FROM agent_model_contexts WHERE message_id=?1",[response_id],|r|r.get::<_,String>(0)).ok().or_else(||original.as_ref().and_then(|id|store.conn.query_row("SELECT messages FROM agent_model_contexts WHERE message_id=?1",[id],|r|r.get::<_,String>(0)).ok()));
        let messages:Vec<Value>=if let Some(saved)=saved_context{serde_json::from_str(&saved).map_err(|_|"原始上下文读取失败")?}else{
            let boundary=original.as_deref().unwrap_or(response_id);
            let mut messages=crate::agent::context_messages(&store,session,boundary)?;
            while messages.len()>1 && messages.iter().map(|v|v["content"].as_str().unwrap_or("").len()).sum::<usize>()>64000{messages.remove(0);}
            while messages.first().is_some_and(|v|v["role"]!="user"){messages.remove(0);}
            if messages.is_empty(){return Err("没有可用于重新生成的原始对话上下文".into());}
            messages.insert(0,json!({"role":"system","content":SYSTEM_PROMPT}));messages
        };
        store.conn.execute("INSERT OR REPLACE INTO agent_model_contexts(message_id,messages) VALUES (?1,?2)",rusqlite::params![response_id,serde_json::to_string(&messages).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
        (url,model,key,provider,messages)
    };
    let mut body=json!({"model":model,"messages":messages,"stream":true,"max_tokens":4096});
    if provider=="deepseek"{body["thinking"]=json!({"type":"disabled"});}
    let request=client()?.post(format!("{url}/chat/completions")).bearer_auth(key).json(&body);
    let response=tokio::select!{biased;_=&mut cancel=>return Ok(ReplyFinish::Stopped),response=request.send()=>response.map_err(|_|"LLM Server 连接失败或等待超时；消息已保存在本机，请稍后重试。")?};
    if !response.status().is_success(){return Err(status_error(response.status()));}
    use futures_util::StreamExt;
    let mut stream=response.bytes_stream();let mut pending=Vec::new();let mut event=String::new();let mut content=String::new();let mut last_flush=std::time::Instant::now();let mut done=false;let mut reported_model=String::new();
    loop{
        let chunk=tokio::select!{biased;_=&mut cancel=>{progress(&content)?;return Ok(ReplyFinish::Stopped)},chunk=stream.next()=>chunk};
        let Some(chunk)=chunk else{break};
        let chunk=match chunk{Ok(chunk)=>chunk,Err(_)=>{progress(&content)?;return Err("模型流连接中断，已生成内容已保存；请重新发送消息。".into());}};
        pending.extend_from_slice(&chunk);
        if pending.len()>1048576{progress(&content)?;return Err("模型流事件过大".into());}
        while let Some(end)=pending.iter().position(|b|*b==b'\n'){
            let line=String::from_utf8(pending.drain(..=end).collect()).map_err(|_|"模型流字符编码错误")?;
            let line=line.trim_end_matches(['\r','\n']);
            if line.is_empty(){
                if !event.is_empty(){
                    if event.trim()=="[DONE]"{done=true;event.clear();break;}
                    let value:Value=serde_json::from_str(&event).map_err(|_|"模型流响应格式错误")?;event.clear();
                    if let Some(actual)=value["model"].as_str().filter(|m|!m.is_empty() && m.len()<=256){
                        if actual!=reported_model{
                            let store=core.store.lock().map_err(|e|e.to_string())?;
                            let saved:String=store.conn.query_row("SELECT model_metadata FROM agent_messages WHERE id=?1",[response_id],|r|r.get(0)).map_err(|e|e.to_string())?;
                            let mut metadata:Value=serde_json::from_str(&saved).map_err(|_|"模型信息读取失败")?;metadata["model"]=json!(actual);
                            store.conn.execute("UPDATE agent_messages SET model_metadata=?2 WHERE id=?1",rusqlite::params![response_id,metadata.to_string()]).map_err(|e|e.to_string())?;reported_model=actual.to_owned();
                        }
                    }
                    if value.get("error").is_some(){progress(&content)?;return Err("LLM Server 流式请求失败，已生成内容已保留。".into());}
                    if let Some(delta)=value["choices"][0]["delta"]["content"].as_str(){
                        let first=content.is_empty();content.push_str(delta);
                        if content.len()>262144{progress(&content[..content.floor_char_boundary(262144)])?;return Err("模型回复过长，已保留生成内容".into());}
                        if first || last_flush.elapsed()>=std::time::Duration::from_millis(60){progress(&content)?;last_flush=std::time::Instant::now();}
                    }
                }
            }else if let Some(data)=line.strip_prefix("data:"){
                if !event.is_empty(){event.push('\n');}event.push_str(data.strip_prefix(' ').unwrap_or(data));
            }
        }
        if done{break;}
    }
    progress(&content)?;
    if !done{return Err("模型流未正常结束，已生成内容已保存。".into());}
    if content.trim().is_empty(){return Err("LLM Server 未返回文字内容".into());}
    Ok(ReplyFinish::Complete)
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test] fn key_is_redacted_preserved_and_cleared_on_endpoint_change(){
  let path=std::env::temp_dir().join(format!("keeper-model-{}.sql",uuid::Uuid::new_v4()));let store=Store::load(path.clone()).unwrap();
  let c=save(&store,&json!({"enabled":true,"apiKey":"secret-test-key","baseUrl":DEFAULT_URL,"model":DEFAULT_MODEL})).unwrap();assert!(c["hasApiKey"]==true);assert!(!c.to_string().contains("secret-test-key"));
  save(&store,&json!({"enabled":true,"baseUrl":DEFAULT_URL,"model":"deepseek-v4-pro"})).unwrap();assert_eq!(store.conn.query_row("SELECT api_key FROM agent_llm_servers",[],|r|r.get::<_,String>(0)).unwrap(),"secret-test-key");
  assert!(save(&store,&json!({"enabled":true,"baseUrl":"https://example.test","model":DEFAULT_MODEL})).is_err());
  save(&store,&json!({"enabled":false,"baseUrl":"https://example.test","model":DEFAULT_MODEL})).unwrap();assert!(!connected(&store));assert_eq!(store.conn.query_row("SELECT api_key FROM agent_llm_servers",[],|r|r.get::<_,String>(0)).unwrap(),"");drop(store);let _=std::fs::remove_file(path);
 }
 #[test] fn migration_preserves_legacy_key_and_is_idempotent(){
  let path=std::env::temp_dir().join(format!("keeper-model-migrate-{}.sql",uuid::Uuid::new_v4()));let store=Store::load(path.clone()).unwrap();
  store.set_setting(CONFIG,&json!({"enabled":true,"baseUrl":DEFAULT_URL,"model":"deepseek-v4-pro"}).to_string()).unwrap();store.set_setting(KEY,"legacy-test-secret").unwrap();
  initialize(&store).unwrap();initialize(&store).unwrap();let c=config(&store).unwrap();assert_eq!(c["servers"].as_array().unwrap().len(),1);assert_eq!(c["name"],"DeepSeek");assert!(connected(&store));assert!(!c.to_string().contains("legacy-test-secret"));assert!(store.get_setting(KEY).unwrap().is_none());
  assert_eq!(store.conn.query_row("SELECT api_key FROM agent_llm_servers",[],|r|r.get::<_,String>(0)).unwrap(),"legacy-test-secret");drop(store);let _=std::fs::remove_file(path);
 }
 #[test] fn servers_edit_keys_independently_and_select_explicitly(){
  let path=std::env::temp_dir().join(format!("keeper-model-list-{}.sql",uuid::Uuid::new_v4()));let store=Store::load(path.clone()).unwrap();
  let a=save(&store,&json!({"name":"A","enabled":true,"baseUrl":DEFAULT_URL,"model":DEFAULT_MODEL,"apiKey":"key-a"})).unwrap();let aid=a["savedServerId"].as_str().unwrap();
  let b=save(&store,&json!({"name":"B","provider":"openai-compatible","enabled":true,"baseUrl":"https://llm.example.test/v1","model":"custom","apiKey":"key-b"})).unwrap();let bid=b["savedServerId"].as_str().unwrap();assert_eq!(b["activeServerId"],aid);
  save(&store,&json!({"id":bid,"name":"B edited","provider":"openai-compatible","enabled":true,"baseUrl":"https://llm.example.test/v1","model":"custom-2"})).unwrap();
  let selected=select(&store,bid).unwrap();assert_eq!(selected["model"],"custom-2");assert_eq!(credentials_for(&store,None).unwrap().2,"key-b");assert_eq!(credentials_for(&store,Some(aid)).unwrap().2,"key-a");
  assert!(!selected.to_string().contains("key-b"));drop(store);let _=std::fs::remove_file(path);
 }
 #[test]fn endpoint_validation(){assert!(base("https://api.deepseek.com/v1/").is_ok());for value in ["http://external.test","https://key@host","https://host?token=x"]{assert!(base(value).is_err());}}
}
