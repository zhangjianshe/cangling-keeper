const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const {JSDOM}=require('jsdom');
const dom=new JSDOM('<!doctype html><body><aside><div class="agent-navigation-sections"></div></aside><main id="agent-main"><button id="agent-session-title"></button><p id="agent-notice"></p><textarea id="agent-input"></textarea></main></body>',{runScripts:'outside-only',url:'http://localhost:4301'});
const context=dom.getInternalVMContext(),document=dom.window.document;
const clone=value=>JSON.parse(JSON.stringify(value)),calls=[],jumps=[];
const agent={identityEpoch:0,ownerKey:'alice',projects:[{id:'p',name:'Test project',directory:'/tmp/fixture'}],project:'p',sessions:[],selected:null,detail:{messages:[],runs:[]}};
const seed=Array.from({length:35},(_,i)=>({id:'s'+i,title:'Conversation '+i,updatedAt:100-i,projectId:'p',pinned:i===34,archivedAt:0,deletedAt:0}));
seed[0].match={messageId:'answer-v2',rootId:'answer',role:'assistant',snippet:'<img src=x onerror=alert(1)> historical answer'};
let records=clone(seed),confirmed=true,pendingOld=null,pendingIdentity=null,history,redraws=0,cleared=0;
const element=(tag,cls,text)=>{const node=document.createElement(tag);if(cls)node.className=cls;if(text!==undefined)node.textContent=text;return node;};
function page(args={}){
 const state=args.state || 'active';let items=records.filter(r=>state==='deleted'?r.deletedAt:state==='archived'?r.archivedAt&&!r.deletedAt:!r.archivedAt&&!r.deletedAt);
 if(args.pinned!==undefined)items=items.filter(r=>r.pinned===args.pinned);
 if(args.projectId)items=items.filter(r=>r.projectId===args.projectId);
 if(args.q==='new')items=[{...records[0],title:'Newest query result',match:{snippet:'fresh result'}}];
 else if(args.q && !['old','identity-pending'].includes(args.q))items=items.filter(r=>r.title.includes(args.q));
 const start=args.cursor?items.findIndex(r=>r.id===args.cursor.id)+1:0,limit=args.limit || 30,part=items.slice(start,start+limit),more=start+limit<items.length;
 return {items:clone(part),hasMore:more,nextCursor:more?{updatedAt:part.at(-1).updatedAt,id:part.at(-1).id}:null};
}
async function invoke(name,args={}){
 calls.push({name,args:clone(args)});
 if(name==='agent_sessions'){
  if(args.q==='old')return new Promise(resolve=>{pendingOld=resolve;});
  if(args.q==='identity-pending')return new Promise(resolve=>{pendingIdentity=resolve;});
  return page(args);
 }
 const record=records.find(r=>r.id===args.sessionId);assert.ok(record,'mutation session exists');
 if(name==='agent_archive_session')record.archivedAt=args.archived?123:0;
 else if(name==='agent_delete_session')record.deletedAt=123;
 else if(name==='agent_restore_session')record.deletedAt=0;
 else if(name==='agent_purge_session'){assert.equal(args.confirm,true);records=records.filter(r=>r!==record);}
 else throw new Error('unexpected API '+name);
 return clone(record);
}
function accept(){const recent=page(),pinned=page({pinned:true});history.acceptBootstrap({accountKey:agent.ownerKey,sessions:recent.items,sessionPage:recent,pinnedSessions:pinned.items,pinnedPage:pinned});}
Object.assign(context,{uiConfirm:async()=>confirmed});context.keeperThreadNav={jump:id=>jumps.push(id)};
vm.runInContext(fs.readFileSync('ui/history.js','utf8'),context);
history=context.createKeeperHistory({agent,invoke,element,notice(){},bootstrap:async()=>accept(),showConversation(){},renderProjects(){redraws++;},renderSessions(){redraws++;},renderConversation(){redraws++;},clearSelected(){cleared++;agent.selected=null;agent.detail={messages:[],runs:[]};},select:async id=>{agent.selected=id;agent.detail={sessionId:id,session:clone(records.find(r=>r.id===id)),messages:[{id:'question',role:'user',modelMetadata:{}},{id:'answer',role:'assistant',modelMetadata:{}},{id:'question-v2',role:'user',modelMetadata:{editOf:'question'}},{id:'answer-v2',role:'assistant',modelMetadata:{regenerateOf:'answer',promptMessageId:'question-v2'}}],replyGroups:[{rootId:'question',versionIds:['question','question-v2'],selectedId:'question'},{rootId:'answer',versionIds:['answer','answer-v2'],selectedId:'answer'}],runs:[]};history.updateDetail();}});
const tick=()=>new Promise(resolve=>setTimeout(resolve,0)),delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const button=(root,label)=>[...root.querySelectorAll('button')].find(b=>b.textContent.trim()===label);
const row=id=>document.querySelector('[data-session-id="'+id+'"]');
async function switchState(state){document.querySelector('.history-tabs [data-state="'+state+'"]').click();await tick();}
(async()=>{
 accept();assert.equal(history.recent().length,30);assert.equal(history.pinned().at(0).id,'s34','pins beyond recent page stay available');
 const moreHost=document.createElement('div');history.appendRecent(moreHost);assert.equal(moreHost.querySelectorAll('button').length,1);moreHost.querySelector('button').click();await tick();assert.equal(history.recent().length,35);assert.equal(new Set(history.recent().map(r=>r.id)).size,35);
 const group=document.createElement('details');group.open=true;history.attachProject(group,'p');await tick();await tick();assert.equal(history.projectSessions('p').length,30);assert.ok(calls.some(c=>c.name==='agent_sessions'&&c.args.projectId==='p'));
 const group2=document.createElement('details');group2.open=true;history.attachProject(group2,'p');group2.querySelector('button').click();await tick();assert.equal(history.projectSessions('p').length,35);
 history.open();await tick();assert.equal(document.querySelector('#agent-history-modal').hidden,false);assert.equal(document.querySelectorAll('.history-result').length,30);
 assert.equal(document.querySelector('.history-snippet').textContent,'<img src=x onerror=alert(1)> historical answer');assert.equal(document.querySelector('#history-results img'),null,'message snippets must render as text');
 document.querySelector('#history-results .history-load-more').click();await tick();assert.equal(document.querySelectorAll('.history-result').length,35);
 row('s0').querySelector('.history-result-open').click();await tick();assert.equal(agent.selected,'s0');assert.deepEqual(jumps,['answer']);assert.equal(agent.detail.replyGroups[0].selectedId,'question-v2');assert.equal(agent.detail.replyGroups[1].selectedId,'answer-v2');assert.ok(!calls.some(c=>c.name==='agent_reply_version'),'search preview must not change persisted selected versions');
 assert.equal(agent.detail.historyPreview,true);history.updateDetail();assert.match(document.querySelector('.history-readonly').textContent,/正在预览历史版本/);assert.equal(document.querySelector('#agent-input').disabled,true);
 history.selectReadingVersion('question');assert.equal(agent.detail.historyPreview,false);history.updateDetail();assert.equal(document.querySelector('#agent-input').disabled,false);assert.equal(agent.detail.replyGroups[0].selectedId,'question');assert.equal(agent.detail.replyGroups[1].selectedId,'answer');assert.deepEqual(Array.from(agent.detail.replyGroups[1].versionIds),['answer']);assert.deepEqual(Array.from(agent.detail.replyGroups[1].allVersionIds),['answer','answer-v2']);
 history.selectReadingVersion('question-v2');assert.equal(agent.detail.replyGroups[1].selectedId,'answer-v2');assert.deepEqual(Array.from(agent.detail.replyGroups[1].versionIds),['answer-v2']);
 history.selectReadingVersion('answer');assert.equal(agent.detail.replyGroups[0].selectedId,'question','legacy answers pair with preceding original user');assert.equal(agent.detail.replyGroups[1].selectedId,'answer');
 history.selectReadingVersion('answer-v2');assert.equal(agent.detail.replyGroups[0].selectedId,'question-v2');assert.equal(agent.detail.replyGroups[1].selectedId,'answer-v2');assert.ok(!calls.some(c=>c.name==='agent_reply_version'),'local reading switches never mutate persisted versions');
 history.updateDetail();assert.equal(agent.detail.historyPreview,true);
 const guardContainer=element('div');guardContainer.innerHTML='<button class="agent-regenerate" data-available="true"></button><button class="agent-edit-message"></button><div data-review-run="fixture"><button></button></div><button class="agent-version-button" data-boundary="false"></button>';document.body.append(guardContainer);
 for(const id of ['agent-send','agent-stop','agent-model-picker','agent-security-picker','agent-security-menu']){const node=element('button');node.id=id;document.body.append(node);}
 Object.assign(context,{$:selector=>document.querySelector(selector),agent,closeModelMenu(){}});agent.history=history;agent.detail.runs=[];
 const agentSource=fs.readFileSync('ui/agent.js','utf8'),guardStart=agentSource.indexOf('  function renderPending(');vm.runInContext(agentSource.slice(guardStart,agentSource.indexOf('  async function saveEditedMessage(',guardStart)),context);context.renderPending(guardContainer);
 assert.equal(document.querySelector('#agent-send').disabled,true);assert.equal(guardContainer.querySelector('.agent-regenerate').disabled,true);assert.equal(guardContainer.querySelector('.agent-edit-message').disabled,true);assert.equal(guardContainer.querySelector('[data-review-run] button').disabled,true);assert.equal(guardContainer.querySelector('.agent-version-button').disabled,false,'read-only browsing still allows local version arrows');
 button(document.querySelector('.history-readonly'),'返回当前版本').click();await tick();assert.equal(!!agent.detail.historyPreview,false);assert.equal(document.querySelector('#agent-input').disabled,false);assert.equal(document.querySelector('.history-readonly').hidden,true);context.renderPending(guardContainer);assert.equal(document.querySelector('#agent-send').disabled,false);assert.equal(guardContainer.querySelector('.agent-regenerate').disabled,false);guardContainer.remove();
 const ensureStart=agentSource.indexOf('  async function ensureSession(');vm.runInContext(agentSource.slice(ensureStart,agentSource.indexOf('  async function select(',ensureStart)),context);agent.detail.historyPreview=true;await assert.rejects(context.ensureSession(),/正在预览历史版本/);agent.detail.historyPreview=false;
 history.open();await tick();button(row('s0'),'归档').click();await tick();await tick();assert.equal(records[0].archivedAt,123);assert.equal(document.querySelector('#agent-main').classList.contains('history-readonly-view'),true);assert.equal(document.querySelector('#agent-input').disabled,true);
 await switchState('archived');assert.ok(row('s0'));button(row('s0'),'取消归档').click();await tick();await tick();assert.equal(records[0].archivedAt,0);assert.equal(document.querySelector('#agent-input').disabled,false);
 await switchState('active');confirmed=false;const before=calls.filter(c=>c.name==='agent_delete_session').length;button(row('s0'),'删除').click();await tick();assert.equal(calls.filter(c=>c.name==='agent_delete_session').length,before,'cancelled delete does not call API');
 confirmed=true;button(row('s0'),'删除').click();await tick();await tick();assert.equal(cleared,1);assert.equal(agent.selected,null);
 await switchState('deleted');assert.ok(row('s0'));assert.equal(row('s0').querySelector('.history-result-open').disabled,true);button(row('s0'),'恢复').click();await tick();await tick();assert.equal(records[0].deletedAt,0);
 await switchState('active');button(row('s0'),'删除').click();await tick();await tick();await switchState('deleted');confirmed=false;button(row('s0'),'永久删除').click();await tick();assert.ok(records.some(r=>r.id==='s0'));confirmed=true;button(row('s0'),'永久删除').click();await tick();await tick();assert.ok(!records.some(r=>r.id==='s0'));
 await switchState('active');const search=document.querySelector('#history-search');search.value='old';search.dispatchEvent(new dom.window.Event('input'));await delay(280);assert.equal(typeof pendingOld,'function');search.value='new';search.dispatchEvent(new dom.window.Event('input'));await delay(280);assert.match(document.querySelector('#history-results').textContent,/Newest query result/);pendingOld({items:[{...seed[0],title:'Stale old result'}],hasMore:false,nextCursor:null});await tick();assert.doesNotMatch(document.querySelector('#history-results').textContent,/Stale old result/);
 search.value='identity-pending';search.dispatchEvent(new dom.window.Event('input'));await delay(280);agent.identityEpoch++;history.reset();agent.ownerKey='bob';pendingIdentity({items:[{...seed[0],title:'Private previous-account result'}],hasMore:false,nextCursor:null});await tick();assert.equal(document.querySelector('#agent-history-modal').hidden,true);assert.doesNotMatch(document.querySelector('#history-results').textContent,/Private previous-account result/);assert.equal(history.recent().length,0);assert.equal(history.pinned().length,0);
 dom.window.close();console.log('PASS: lazy recent/project paging; older pins; safe search snippets; historical version jump without mutation; archive composer read-only; delete/purge confirmation and restore; stale search and account-switch isolation.');
})().catch(error=>{console.error(error);dom.window.close();process.exitCode=1;});
