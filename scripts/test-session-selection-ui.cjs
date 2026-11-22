const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),{JSDOM}=require('jsdom');
const dom=new JSDOM(fs.readFileSync('ui/index.html','utf8'),{runScripts:'outside-only',url:'http://localhost/'}),c=dom.getInternalVMContext(),d=dom.window.document,source=fs.readFileSync('ui/agent.js','utf8');
const sessions=[{id:'one',projectId:'p',title:'First',pinned:true},{id:'two',projectId:'p',title:'Second',pinned:true}],agent={projects:[{id:'p',name:'Project',directory:'/tmp'}],project:'p',sessions,selected:'one',generation:0,detail:{messages:[],runs:[]}};
let pending=[];
Object.assign(c,{agent,$:s=>d.querySelector(s),showConversation(){},renderConversation(){},renderModel(){},poll(){},notice(){},sessionStorageKey:()=> 'fixture',invoke:(name,args)=>new Promise(resolve=>pending.push(()=>resolve({sessionId:args.sessionId,session:sessions.find(s=>s.id===args.sessionId),messages:[],runs:[]})))});
for(const [start,end] of [['  function element(','  function relativeMessageTime('],['  function sessionRow(',"  $('#agent-session-title').onclick="],['  function renderProjects(','  async function switchProject('],['  async function select(','  function poll(']])vm.runInContext(source.slice(source.indexOf(start),source.indexOf(end,source.indexOf(start))),c);
function check(title){for(const selector of ['#agent-projects','#agent-sessions','#agent-pinned']){const active=d.querySelectorAll(selector+' .agent-session.active');assert.equal(active.length,1);assert.equal(active[0].textContent,title);assert.equal(active[0].getAttribute('aria-current'),'true');}}
const tick=()=>new Promise(r=>setTimeout(r,0));
(async()=>{
 c.renderProjects();c.renderSessions();check('First');
 d.querySelectorAll('#agent-projects .agent-session')[1].click();pending.shift()();await tick();check('Second');
 d.querySelector('#agent-sessions .agent-session').click();pending.shift()();await tick();check('First');
 // A slow earlier response must not restore its highlight after a later click.
 const old=c.select('two'),latest=c.select('one');pending.pop()();await latest;pending.shift()();await old;check('First');
 agent.drafts={load:async()=>false};await c.select('two');check('First');
 dom.window.close();console.log('PASS: project/recent/pinned selection stays synchronized; stale responses and blocked draft switches preserve selection');
})().catch(e=>{console.error(e);dom.window.close();process.exitCode=1});
