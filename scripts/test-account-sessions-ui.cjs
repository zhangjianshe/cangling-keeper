const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),{JSDOM}=require('jsdom');
const dom=new JSDOM(fs.readFileSync('ui/index.html','utf8'),{runScripts:'outside-only',url:'http://localhost/'}),c=dom.getInternalVMContext(),d=dom.window.document,source=fs.readFileSync('ui/agent.js','utf8');let resolveOld,n=0;
Object.assign(c,{fullAccessFinish:null,llmRemote:false,closeLlm(){},$:s=>d.querySelector(s),agent:{ownerKey:'A',project:'general',selected:'A-chat',sessions:[{id:'A-chat'}],projects:[],detail:{messages:[{content:'private A'}],runs:[]},generation:0},renderSessions(){},renderProjects(){},renderConversation(){},renderModel(){},renderTools(){},closeSecurityMenu(){},notice(){},select:async id=>{c.agent.selected=id;},invoke:async()=>{if(++n===1)return new Promise(r=>resolveOld=r);return {accountKey:'B',projects:[],sessions:[{id:'B-chat'}],model:{},tools:[],hosts:[]};}});
let start=source.indexOf('  function sessionStorageKey(');vm.runInContext(source.slice(start,source.indexOf('  function titleFor(',start)),c);
start=source.indexOf('  async function bootstrap(');vm.runInContext(source.slice(start,source.indexOf('  async function ensureSession(',start)),c);
start=source.indexOf('  async function refreshAccountSessions(');vm.runInContext(source.slice(start,source.indexOf('  let loginIdentity=',start)),c);
(async()=>{
 c.localStorage.setItem('keeper-agent-session:A','A-chat');c.localStorage.setItem('keeper-agent-session:B','B-chat');d.querySelector('#agent-input').value='A draft';
 const stale=c.bootstrap(),refresh=c.refreshAccountSessions();assert.deepEqual(Array.from(c.agent.detail.messages),[]);assert.equal(c.agent.selected,null);assert.equal(d.querySelector('#agent-input').value,'');await refresh;assert.equal(c.agent.selected,'B-chat');assert.equal(c.agent.ownerKey,'B');
 resolveOld({accountKey:'A',projects:[],sessions:[{id:'A-chat'}],tools:[],hosts:[],model:{}});await stale;assert.equal(c.agent.ownerKey,'B');assert.equal(c.agent.sessions[0].id,'B-chat');assert.equal(c.sessionStorageKey(),'keeper-agent-session:B');
 dom.window.close();console.log('PASS: immediate account UI/draft clear, per-account selection restore, stale bootstrap cannot restore previous account');
})().catch(e=>{console.error(e);dom.window.close();process.exitCode=1;});
