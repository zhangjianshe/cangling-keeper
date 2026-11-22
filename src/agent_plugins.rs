//! Versioned MCP configuration packages; installation never launches or downloads code.
use crate::store::Store;
use serde::{Deserialize,Serialize};
use serde_json::{Value,json};
#[derive(Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Manifest {schema_version:u32,id:String,name:String,version:String,source:String,permissions:Vec<String>,mcp:Mcp}
#[derive(Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct Mcp {transport:String,command:String,args:Vec<String>}
fn manifest(input:&Value)->Result<Value,String>{
    if input.to_string().len()>32768{return Err("插件清单最多32 KiB".into());}
    let m:Manifest=serde_json::from_value(input.clone()).map_err(|_|"插件清单字段无效或含不支持的字段")?;
    if m.schema_version!=1 || m.id.is_empty() || m.id.len()>80 || !m.id.chars().all(|c|c.is_ascii_alphanumeric()||matches!(c,'-'|'_'|'.')){return Err("插件ID或清单版本无效".into());}
    if m.name.trim().is_empty() || m.name.len()>80 || m.name.chars().any(char::is_control){return Err("插件名称无效".into());}
    let version=m.version.split('.').collect::<Vec<_>>();
    if version.len()!=3 || version.iter().any(|v|v.is_empty()||v.len()>8||!v.bytes().all(|c|c.is_ascii_digit())){return Err("插件版本须为数字主版本.次版本.修订版本".into());}
    let source=reqwest::Url::parse(&m.source).map_err(|_|"插件来源须为 HTTPS 地址")?;
    if source.scheme()!="https" || source.host_str().is_none() || !source.username().is_empty() || source.password().is_some() || m.source.len()>2048{return Err("插件来源须为不含凭证的 HTTPS 地址".into());}
    let permission=match m.mcp.transport.as_str(){"stdio"=>"executeLocal","http"|"sse"=>"connectRemote",_=>return Err("不支持的 MCP 传输类型".into())};
    if m.permissions!=vec![permission.to_string()]{return Err(format!("该连接必须明确声明权限 {permission}，不支持额外权限"));}
    if m.mcp.command.is_empty() || m.mcp.command.len()>2048 || m.mcp.command.chars().any(char::is_control) || m.mcp.args.len()>64 || m.mcp.args.iter().any(|a|a.len()>4096||a.contains('\0')){return Err("插件命令或参数无效".into());}
    if permission=="connectRemote"{crate::agent_mcp_http::validate_url(&m.mcp.command)?;if !m.mcp.args.is_empty(){return Err("远程插件清单不允许携带请求头；安装后在 MCP 配置中设置".into());}}
    serde_json::to_value(m).map_err(|e|e.to_string())
}
pub(crate) fn dispatch(store:&Store,args:&Value,install:bool)->Result<Value,String>{
    let manifest=manifest(&args["manifest"])?;
    let owner=crate::agent_account::owner(store)?;
    let projects=args["projectIds"].as_array().filter(|p|!p.is_empty()&&p.len()<=100).ok_or("安装时必须指定项目范围")?;
    for project in projects{let id=project.as_str().ok_or("项目ID无效")?;let exists:bool=store.conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_projects WHERE id=?1)",[id],|r|r.get(0)).map_err(|e|e.to_string())?;if !exists{return Err("项目不存在".into());}}
    let fingerprint=crate::agent_actions::hash(manifest.to_string().as_bytes());
    let review=crate::agent_actions::hash(json!([owner,manifest,projects]).to_string().as_bytes());
    if !install{return Ok(json!({"manifest":manifest,"sha256":fingerprint,"review":review,"sourceVerified":false}));}
    if args["confirmed"]!=true || args["review"]!=review{return Err("插件清单、账号或项目范围已变化，请重新审阅确认".into());}
    if crate::agent_mcp::list(store)?.as_array().unwrap().iter().any(|s|s["plugin"]["manifest"]["id"]==manifest["id"]){return Err("该插件已安装；升级前请先卸载旧配置，不会自动替换".into());}
    let tx=store.conn.unchecked_transaction().map_err(|e|e.to_string())?;
    let saved=crate::agent_mcp::save(store,&json!({"name":manifest["name"],"command":manifest["mcp"]["command"],"transport":manifest["mcp"]["transport"],"args":manifest["mcp"]["args"],"enabled":false,"projectIds":projects}))?;
    let receipt=json!({"manifest":manifest,"sha256":fingerprint,"sourceVerified":false});
    store.conn.execute("UPDATE agent_mcp_servers SET plugin_manifest=?2 WHERE id=?1",rusqlite::params![saved["id"].as_str(),receipt.to_string()]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e|e.to_string())?;Ok(json!({"id":saved["id"],"servers":crate::agent_mcp::list(store)?}))
}
