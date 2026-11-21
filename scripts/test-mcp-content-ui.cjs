const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),{JSDOM}=require('jsdom');
const dom=new JSDOM('<!doctype html><body><div id="mcp-list"></div><p id="mcp-status"></p><textarea id="agent-input"></textarea></body>',{runScripts:'outside-only',url:'http://localhost:4301'}),context=dom.getInternalVMContext(),d=dom.window.document;
const source=fs.readFileSync('ui/agent.js','utf8'),start=source.indexOf('  async function useMcpCapability('),end=source.indexOf('  let mcpLoadSequence=',start);assert.ok(start>0 && end>start);
const calls=[],notes=[];let shown=0,newSessions=0,resolveRefresh=null,delayRefresh=false,resolveConfirm=null,delayConfirm=false;
const clone=value=>JSON.parse(JSON.stringify(value));
const tools=[{name:'echo',title:'<img src=x onerror=alert(1)>',description:'<script>unsafe()</script>',enabled:true,inputSchema:{type:'object'}},
 ...[['resources/list','列出 MCP 资料'],['resources/read','读取 MCP 资料'],['resources/templates/list','列出 MCP 资料模板'],['prompts/list','列出 MCP 提示模板'],['prompts/get','获取 MCP 提示模板']].map(([kind,title],i)=>({name:'__keeper_mcp_native_'+i,kind,title,description:'Read explicitly selected external data',...(i?{enabled:false}:{}),inputSchema:{type:'object'}}))];
let server={id:'fixture',name:'<b>Fixture</b>',transport:'stdio',command:'/tmp/mock-server',args:[],enabled:true,connectedSessions:0,tools};
const agent={identityEpoch:0,selected:'session',detail:{session:{projectId:'general'}},sessions:[]};
function element(tag,cls,text){const node=d.createElement(tag);if(cls)node.className=cls;if(text!==undefined)node.textContent=text;return node;}
Object.assign(context,{$:s=>d.querySelector(s),agent,element,notice:x=>notes.push(x),showConversation(){shown++;},newSession:async()=>{newSessions++;agent.selected='new-session';},openMcp(){},uiConfirm:async()=>delayConfirm?new Promise(resolve=>{resolveConfirm=resolve;}):true,invoke:async(name,args)=>{
 calls.push({name,args:clone(args)});
 if(name==='agent_mcp_tools'){server={...server,tools:server.tools.map(t=>({...t,enabled:args.tools.includes(t.name)}))};return [clone(server)];}
 if(name==='agent_mcp_refresh'){if(delayRefresh)return new Promise(resolve=>{resolveRefresh=resolve;});return [clone(server)];}
 if(name==='agent_mcp_save')return {servers:[clone(server)]};
 if(name==='agent_mcp_delete')return [];
 throw new Error('unexpected operation '+name);
}});
vm.runInContext(source.slice(start,end),context);
const tick=()=>new Promise(resolve=>setTimeout(resolve,0)),button=label=>[...d.querySelectorAll('button')].find(b=>b.textContent===label);
const nativeOptions=()=>[...d.querySelectorAll('.mcp-capability-section')].find(s=>s.querySelector('h4').textContent==='资料与提示模板').querySelectorAll('.mcp-capability-option');
(async()=>{
 context.renderMcp([server]);assert.equal(calls.length,0,'render must not connect or fetch native catalogs');assert.equal(d.querySelectorAll('.mcp-capability-section').length,2);assert.equal(d.querySelectorAll('input:checked').length,1,'only ordinary tool defaults on');assert.ok([...nativeOptions()].every(n=>!n.querySelector('input').checked && n.querySelector('button').disabled),'native capabilities default off even without enabled flag');assert.equal(d.querySelector('img,script,b'),null,'remote metadata must render as plain text');assert.match(d.querySelector('#mcp-list').textContent,/<img src=x onerror=alert\(1\)>/);assert.match(d.querySelector('.llm-server-meta').textContent,/资料与模板 0\/5/);
 const first=nativeOptions()[0];first.querySelector('input').checked=true;assert.equal(first.querySelector('button').disabled,true,'unsaved selection must not activate draft action');button('保存能力选择').click();await tick();assert.equal(calls.length,1);assert.equal(calls[0].name,'agent_mcp_tools');assert.deepEqual(calls[0].args.tools,['echo','__keeper_mcp_native_0']);assert.equal(nativeOptions()[0].querySelector('button').disabled,false);assert.match(d.querySelector('.llm-server-meta').textContent,/资料与模板 1\/5/);
 const input=d.querySelector('#agent-input');input.value='  preserve indentation\n';nativeOptions()[0].querySelector('button').click();await tick();assert.equal(input.value,'  preserve indentation\n请列出 MCP 服务「<b>Fixture</b>」提供的资料。');assert.equal(shown,1);assert.equal(calls.length,1,'draft insertion must not send a message or make MCP call');assert.equal(d.querySelector('img,b'),null);
 agent.detail.historyPreview=true;context.renderMcp([server]);assert.equal(nativeOptions()[0].querySelector('button').disabled,true);nativeOptions()[0].querySelector('button').onclick();await tick();assert.match(notes.at(-1),/当前版本/);delete agent.detail.historyPreview;agent.detail.session.archivedAt=1;context.renderMcp([server]);assert.equal(nativeOptions()[0].querySelector('button').disabled,true);agent.detail.session.archivedAt=0;
 server={...server,enabled:false};context.renderMcp([server]);assert.equal(nativeOptions()[0].querySelector('button').disabled,true);server={...server,enabled:true};context.renderMcp([server]);
 const stale=nativeOptions()[0].querySelector('button'),draft=input.value;agent.identityEpoch++;stale.onclick();await tick();assert.equal(input.value,draft,'stale previous-account capability cannot change draft');context.renderMcp([server]);
 agent.selected=null;nativeOptions()[0].querySelector('button').click();await tick();assert.equal(newSessions,1);assert.equal(calls.length,1,'creating conversation must not invoke model');assert.equal(agent.selected,'new-session');
 // A discovery confirmation opened by another account must not start its configured process.
 delayConfirm=true;button('连接并发现').click();await tick();assert.equal(typeof resolveConfirm,'function');agent.identityEpoch++;resolveConfirm(true);await tick();assert.equal(calls.length,1);
 delayConfirm=false;delayRefresh=true;context.renderMcp([server]);button('连接并发现').click();await tick();assert.equal(typeof resolveRefresh,'function');agent.identityEpoch++;d.querySelector('#mcp-list').replaceChildren();resolveRefresh([clone(server)]);await tick();assert.equal(d.querySelector('#mcp-list').children.length,0,'late discovery cannot restore previous-account metadata');
 dom.window.close();console.log('PASS: native MCP capabilities explicitly opt in; friendly safe metadata; saved internal selections; no eager native calls; draft-only shortcuts preserve text; readonly and account-change guards.');
})().catch(error=>{console.error(error);dom.window.close();process.exitCode=1;});
