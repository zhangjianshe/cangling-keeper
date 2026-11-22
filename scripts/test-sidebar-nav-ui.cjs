const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),{JSDOM}=require('jsdom');
const dom=new JSDOM(fs.readFileSync('ui/index.html','utf8'),{runScripts:'outside-only',url:'http://localhost/',pretendToBeVisual:true}),c=dom.getInternalVMContext(),d=dom.window.document,source=fs.readFileSync('ui/agent.js','utf8');
const clone=value=>JSON.parse(JSON.stringify(value)),calls=[],notices=[];
const unsafeTitle='<img src=x onerror=bad()> private title';
const records=[{id:'one',title:unsafeTitle,projectId:'p',pinned:false,updatedAt:20,archivedAt:0,deletedAt:0},{id:'two',title:'Already pinned',projectId:'p',pinned:true,updatedAt:10,archivedAt:0,deletedAt:0}];
const agent={identityEpoch:0,ownerKey:'alice',projects:[{id:'p',name:'<svg onload=bad()> project',directory:'/tmp/project'}],project:'p',sessions:[],selected:'one',detail:{sessionId:'one',session:clone(records[0]),messages:[{id:'old-answer',content:'Historical content'}],runs:[],historyPreview:true}};
let history,bootstraps=0,selections=0,pendingPin=null,deferPin=false;
const page=args=>({items:clone(records.filter(item=>!item.archivedAt && !item.deletedAt && (args.pinned===undefined || item.pinned===args.pinned) && (!args.projectId || item.projectId===args.projectId))),hasMore:false,nextCursor:null});
async function invoke(name,args){
 calls.push({name,args:clone(args)});
 if(name==='agent_sessions')return page(args);
 if(name==='agent_pin_session'){
  const item=records.find(item=>item.id===args.sessionId);item.pinned=args.pinned;
  if(deferPin)return new Promise(resolve=>pendingPin=resolve);
  return clone(item);
 }
 if(name==='agent_archive_session'){records.find(item=>item.id===args.sessionId).archivedAt=args.archived?1:0;return;}
 throw new Error('Unexpected command '+name);
}
async function bootstrap(){bootstraps++;const recent=page({}),pinned=page({pinned:true});history.acceptBootstrap({accountKey:agent.ownerKey,sessions:recent.items,sessionPage:recent,pinnedSessions:pinned.items,pinnedPage:pinned});c.renderSessions();c.renderProjects();}
Object.assign(c,{$:selector=>d.querySelector(selector),agent,invoke,bootstrap,notice:message=>notices.push(message),showConversation(){},select:async()=>{selections++;},newSession(){},uiConfirm:async()=>true});
vm.runInContext(source.slice(source.indexOf('  function element('),source.indexOf('  function notice(')),c);
vm.runInContext(source.slice(source.indexOf('  function sessionRow('),source.indexOf("  $('#agent-session-title').onclick=")),c);
vm.runInContext(source.slice(source.indexOf('  function renderProjects('),source.indexOf('  async function switchProject(')),c);
vm.runInContext(fs.readFileSync('ui/history.js','utf8'),c);
history=c.createKeeperHistory({agent,invoke,element:c.element,notice:c.notice,bootstrap,select:c.select,showConversation:c.showConversation,renderProjects:()=>c.renderProjects(),renderSessions:()=>c.renderSessions(),renderConversation(){},clearSelected(){agent.selected=null;}});agent.history=history;
const tick=()=>new Promise(resolve=>setTimeout(resolve,0)),flush=async()=>{await tick();await tick();await tick();};
const findRow=(section,title)=>[...d.querySelectorAll(section+' .agent-session-row')].find(row=>row.querySelector('.agent-session').textContent===title);
const menuButton=label=>[...d.querySelectorAll('.history-row-menu button')].find(button=>button.textContent===label);
const openMenu=row=>row.querySelector('.history-row-more').click();
(async()=>{
 await bootstrap();await flush();history.updateDetail();
 assert.equal(d.querySelectorAll('.agent-pin').length,0,'inline stars are removed from recent, pinned and project rows');
 assert.equal(d.querySelectorAll('.agent-session-row').length,5);
 assert.equal(d.querySelector('.agent-project-name').textContent,agent.projects[0].name);
 assert.equal(d.querySelector('#agent-projects svg[onload]'),null,'project names remain plain text');
 const row=findRow('#agent-sessions',unsafeTitle),more=row.querySelector('.history-row-more');
 assert.equal(row.querySelector('.agent-session').title,agent.projects[0].name+' · '+unsafeTitle);
 assert.equal(more.getAttribute('aria-label'),'会话操作：'+unsafeTitle);
 assert.equal(row.querySelector('img'),null,'session titles remain plain text');
 openMenu(row);assert.equal(more.getAttribute('aria-expanded'),'true');
 assert.deepEqual([...d.querySelectorAll('.history-row-menu button')].map(button=>button.textContent),['置顶会话','导出 Markdown','归档','移到回收站']);
 assert.equal(d.querySelector('.history-row-menu').getAttribute('aria-label'),unsafeTitle+'的操作');
 assert.equal(d.querySelector('.history-row-menu img'),null);assert.ok(menuButton('置顶会话').querySelector('svg path'));
 assert.equal(d.activeElement,menuButton('置顶会话'));
 d.activeElement.dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true}));assert.equal(d.activeElement,menuButton('导出 Markdown'));
 d.activeElement.dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:'ArrowUp',bubbles:true}));assert.equal(d.activeElement,menuButton('置顶会话'));
 d.activeElement.dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:'Escape',bubbles:true}));assert.equal(d.querySelector('.history-row-menu'),null);assert.equal(d.activeElement,more);assert.equal(more.getAttribute('aria-expanded'),'false');
 const contextMenu=new dom.window.MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:120,clientY:80});row.dispatchEvent(contextMenu);assert.equal(contextMenu.defaultPrevented,true);assert.ok(menuButton('置顶会话'));
 const readingDetail=agent.detail,readingMessages=agent.detail.messages;records[0].title='服务端更新后的标题';menuButton('置顶会话').click();await flush();
 assert.deepEqual(calls.filter(call=>call.name==='agent_pin_session').map(call=>call.args),[{sessionId:'one',pinned:true}]);
 assert.equal(records[0].pinned,true);assert.ok(findRow('#agent-pinned',records[0].title));assert.equal(d.querySelector('#agent-session-title').textContent,records[0].title,'bootstrap refreshes the header title');
 assert.equal(agent.detail,readingDetail);assert.equal(agent.detail.messages,readingMessages);assert.equal(agent.detail.historyPreview,true);assert.equal(selections,0,'pinning does not reload or exit a historical reading preview');
 assert.equal(d.querySelector('#agent-input').disabled,true);assert.equal(d.querySelector('#agent-session-title').disabled,true);assert.equal(d.querySelector('.history-readonly').hidden,false);
 openMenu(findRow('#agent-pinned',records[0].title));assert.ok(menuButton('取消置顶'));menuButton('取消置顶').click();await flush();assert.equal(records[0].pinned,false);assert.equal(findRow('#agent-pinned',records[0].title),undefined);assert.ok(findRow('#agent-sessions',records[0].title));
 const projectRow=findRow('#agent-projects',records[0].title);projectRow.querySelector('.agent-session').focus();projectRow.dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:'F10',shiftKey:true,bubbles:true,cancelable:true}));assert.ok(menuButton('置顶会话'));menuButton('置顶会话').click();await flush();assert.equal(records[0].pinned,true,'project child rows can also pin a session');
 openMenu(findRow('#agent-sessions',records[0].title));assert.ok(menuButton('取消置顶'));menuButton('取消置顶').click();await flush();assert.equal(records[0].pinned,false,'recent rows can also unpin a session');
 openMenu(findRow('#agent-pinned','Already pinned'));menuButton('归档').click();await flush();assert.equal(records[1].archivedAt,1,'existing lifecycle menu action is retained');assert.equal(findRow('#agent-pinned','Already pinned'),undefined);
 const current=findRow('#agent-sessions',records[0].title);current.dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:'ContextMenu',bubbles:true,cancelable:true}));assert.ok(menuButton('置顶会话'));d.body.click();assert.equal(d.querySelector('.history-row-menu'),null,'outside clicks dismiss the menu');
 deferPin=true;openMenu(current);menuButton('置顶会话').click();assert.equal(typeof pendingPin,'function');const before=bootstraps;agent.identityEpoch++;history.reset();agent.ownerKey='bob';agent.projects=[];agent.sessions=[];agent.selected=null;agent.detail={messages:[],runs:[]};c.renderSessions();c.renderProjects();pendingPin();await flush();
 assert.equal(bootstraps,before,'a previous account pin response cannot bootstrap the new account');assert.equal(history.recent().length,0);assert.equal(history.pinned().length,0);assert.equal(d.querySelectorAll('.agent-session-row').length,0);assert.equal(d.querySelector('.history-row-menu'),null);assert.equal(selections,0);assert.equal(notices.filter(Boolean).length,0);
 dom.window.close();console.log('PASS: shared sidebar rows have no inline stars; safe title metadata; keyboard/right-click pin/unpin from recent, pinned and project menus; title refresh preserves read-only history; lifecycle actions; stale account pin isolation.');
})().catch(error=>{console.error(error);dom.window.close();process.exitCode=1;});
