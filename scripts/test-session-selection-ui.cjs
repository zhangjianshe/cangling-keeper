const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),{JSDOM}=require('jsdom');
const dom=new JSDOM(fs.readFileSync('ui/index.html','utf8'),{runScripts:'outside-only',url:'http://localhost/'}),c=dom.getInternalVMContext(),d=dom.window.document,source=fs.readFileSync('ui/agent.js','utf8');
const sessions=[{id:'one',projectId:'p',title:'First',pinned:true},{id:'two',projectId:'p',title:'Second',pinned:true}],agent={projects:[{id:'p',name:'Project',directory:'/tmp'}],project:'p',sessions,selected:'one',generation:0,detail:{messages:[],runs:[]}};
let pending=[];
Object.assign(c,{agent,$:s=>d.querySelector(s),showConversation(){},renderConversation(){},renderModel(){},poll(){},notice(){},sessionStorageKey:()=> 'fixture',invoke:(name,args)=>new Promise(resolve=>pending.push(()=>resolve({sessionId:args.sessionId,session:sessions.find(s=>s.id===args.sessionId),messages:[],runs:[]})))});
for(const [start,end] of [['  function element(','  function relativeMessageTime('],['  function sessionRow(',"  $('#agent-session-title').onclick="],['  function projectRow(','  async function switchProject('],['  async function select(','  function poll(']])vm.runInContext(source.slice(source.indexOf(start),source.indexOf(end,source.indexOf(start))),c);
function check(title){for(const selector of ['#agent-projects','#agent-sessions','#agent-pinned']){const active=d.querySelectorAll(selector+' .agent-session.active');assert.equal(active.length,1);assert.equal(active[0].textContent,title);assert.equal(active[0].getAttribute('aria-current'),'true');}}
const tick=()=>new Promise(r=>setTimeout(r,0));
(async()=>{
 c.renderProjects();c.renderSessions();check('First');assert.equal(d.querySelector('.agent-project-group.has-selected-session').dataset.projectId,'p');
 d.querySelectorAll('#agent-projects .agent-session')[1].click();pending.shift()();await tick();check('Second');assert.equal(d.activeElement,d.querySelector('#agent-input'));
 d.querySelector('#agent-sessions .agent-session').click();pending.shift()();await tick();check('First');assert.equal(d.activeElement,d.querySelector('#agent-input'));
 // A slow earlier response must not restore its highlight after a later click.
 const old=c.select('two'),latest=c.select('one');pending.pop()();await latest;const title=d.querySelector('#agent-session-title');title.focus();pending.shift()();await old;check('First');assert.equal(d.activeElement,title);
 // Refreshes of the current session must not steal focus from another control.
 const refresh=c.select('one');pending.shift()();await refresh;assert.equal(d.activeElement,title);
 agent.drafts={load:async()=>false};await c.select('two');check('First');
 agent.projects.push({id:'other',name:'Other',directory:'/tmp/other'});agent.detail.session={id:agent.selected,projectId:'other'};c.renderProjects();assert.equal(d.querySelector('.has-selected-session').dataset.projectId,'other');agent.selected=null;agent.detail={messages:[],runs:[]};c.renderProjects();assert.equal(d.querySelector('.has-selected-session'),null);
 // Large project lists stay bounded; redrawing must restore the actual scroll container.
 const scroller=d.querySelector('.agent-navigation-sections');for(const id of ['agent-projects','agent-sessions','agent-pinned']){const node=d.getElementById(id),replace=node.replaceChildren.bind(node);node.replaceChildren=(...args)=>{scroller.scrollTop=0;replace(...args);};}
 agent.projects=Array.from({length:27},(_,i)=>({id:'p'+i,name:'Project '+i,directory:'/tmp/'+i}));agent.project='p26';agent.ownerKey='alice';scroller.scrollTop=350;c.renderProjects();assert.equal(scroller.scrollTop,350);assert.equal(d.querySelectorAll('.agent-project-group').length,11);assert(d.querySelector('[data-project-id="p26"]'));
 d.querySelector('#agent-projects .history-load-more').click();assert.equal(d.querySelectorAll('.agent-project-group').length,21);assert.equal(scroller.scrollTop,350);c.renderSessions();assert.equal(scroller.scrollTop,350);
 d.querySelector('#agent-projects .history-load-more').click();assert.equal(d.querySelectorAll('.agent-project-group').length,27);[...d.querySelectorAll('#agent-projects button')].find(b=>b.textContent==='收起项目').click();assert.equal(d.querySelectorAll('.agent-project-group').length,11);
 d.querySelector('#agent-projects .history-load-more').click();agent.ownerKey='bob';c.renderProjects();assert.equal(d.querySelectorAll('.agent-project-group').length,11);assert.equal(d.querySelector('#agent-projects').dataset.limit,'10');
 agent.drafts=null;agent.projects.push({id:'p',name:'Project'});const followed=[];agent.earth={followProject:async p=>followed.push(p.id)};
 const cross=c.select('two');pending.shift()();await cross;assert.equal(agent.project,'p');assert.deepEqual(followed,['p'],'session selection also synchronizes the Earth project');
 agent.drafts={load:async()=>false};await c.select('one');assert.deepEqual(followed,['p'],'cancelled drafts do not switch Earth');
 dom.window.close();console.log('PASS: project/recent/pinned selection stays synchronized; stale responses and blocked draft switches preserve selection');
})().catch(e=>{console.error(e);dom.window.close();process.exitCode=1});
