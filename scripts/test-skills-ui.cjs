const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const {JSDOM}=require('jsdom');
const dom=new JSDOM('<!doctype html><body><div class="agent-header-tools"></div><textarea id="agent-input"></textarea></body>',{runScripts:'outside-only',url:'http://localhost:4301'}),context=dom.getInternalVMContext(),document=dom.window.document;
const agent={selected:'session-a',identityEpoch:0,project:'project',projects:[{id:'project',name:'Selected project'},{id:'different',name:'Wrong project'}],detail:{session:{id:'session-a',projectId:'project',archivedAt:0,deletedAt:0}}};
const element=(tag,cls,text)=>{const node=document.createElement(tag);if(cls)node.className=cls;if(text!==undefined)node.textContent=text;return node;};
const calls=[],notices=[];
const snapshot={rules:{path:'AGENTS.md',status:'loaded',content:'<img src=x onerror=alert(1)> RULES_ONE'},skills:[{name:'write-notes',description:'<svg onload=alert(2)> Useful notes',path:'.agents/skills/write-notes/SKILL.md',sha256:'catalog-v1'}],warnings:['A malformed skill was skipped.'],catalogTruncated:false};
let pendingScan=null,pendingSkill=null,scanDeferred=false,skillDeferred=false,hashChanged=false;
async function invoke(name,args){
 calls.push({name,args:JSON.parse(JSON.stringify(args))});
 if(name==='agent_project_guidance'){
  if(scanDeferred){scanDeferred=false;return new Promise(resolve=>{pendingScan=resolve;});}
  return structuredClone(snapshot);
 }
 if(name==='agent_skill_content'){
  if(skillDeferred){skillDeferred=false;return new Promise(resolve=>{pendingSkill=resolve;});}
  if(hashChanged)throw new Error('技能文件已变更，请重新扫描。');
  if(args.offset===0)return {name:'write-notes',instructions:'LAZY_BODY_PAGE_ONE\n',nextOffset:18,sha256:'catalog-v1'};
  assert.equal(args.offset,18);assert.equal(args.expectedSha256,'catalog-v1');
  return {name:'write-notes',instructions:'<script>SECOND_PAGE</script>',nextOffset:null,sha256:'catalog-v1'};
 }
 throw new Error('Unexpected operation '+name);
}
vm.runInContext(fs.readFileSync('ui/project-guidance.js','utf8'),context);
const ui=context.createKeeperGuidance({agent,invoke,element,notice:message=>notices.push(message)}),tick=()=>new Promise(resolve=>setTimeout(resolve,0));
const button=(root,label)=>[...root.querySelectorAll('button')].find(b=>b.textContent===label);
const content=()=>document.querySelector('#guidance-content'),skill=()=>document.querySelector('[data-skill-name="write-notes"]');
const open=async()=>{ui.update();document.querySelector('#agent-guidance-open').click();await tick();};
(async()=>{
 assert.equal(document.querySelector('#agent-guidance-open').disabled,true);await open();
 assert.equal(calls.length,1);assert.equal(calls[0].name,'agent_project_guidance');assert.equal(calls[0].args.sessionId,'session-a');
 assert.match(content().textContent,/RULES_ONE/);assert.equal(content().querySelector('img'),null);assert.equal(document.querySelector('#guidance-warnings').textContent,'A malformed skill was skipped.');
 document.querySelector('[data-tab="skills"]').click();assert.equal(calls.length,1,'switching tabs must not eagerly load skill bodies');assert.equal(skill().querySelector('svg'),null);assert.doesNotMatch(content().textContent,/LAZY_BODY/);
 button(skill(),'查看').click();await tick();assert.equal(calls.at(-1).name,'agent_skill_content');assert.deepEqual(calls.at(-1).args,{sessionId:'session-a',name:'write-notes',offset:0,expectedSha256:'catalog-v1'});assert.match(content().textContent,/LAZY_BODY_PAGE_ONE/);
 button(skill(),'继续读取').click();await tick();assert.match(content().textContent,/<script>SECOND_PAGE<\/script>/);assert.equal(content().querySelector('script'),null,'skill content must stay plain text');assert.equal(button(skill(),'继续读取'),undefined);
 const input=document.querySelector('#agent-input'),draft='  Preserve draft indentation  \n';input.value=draft;button(skill(),'用于任务').click();assert.equal(input.value,draft+'$write-notes');assert.equal(document.querySelector('#project-guidance-modal').hidden,true);assert.ok(!calls.some(c=>c.name==='agent_send_message'),'using a skill only edits the draft, never auto-sends');
 await open();document.querySelector('[data-tab="skills"]').click();agent.detail.historyPreview=true;ui.update();assert.equal(button(skill(),'用于任务').disabled,true,'same-session preview transition updates the visible control');agent.detail.historyPreview=false;ui.update();assert.equal(button(skill(),'用于任务').disabled,false);button(skill(),'用于任务').click();assert.equal(input.value.split('$write-notes').length-1,1,'repeated insertion must not duplicate skill mention');
 agent.detail.session.archivedAt=10;await open();document.querySelector('[data-tab="skills"]').click();assert.equal(button(skill(),'用于任务').disabled,true);const previous=input.value;button(skill(),'用于任务').click();assert.equal(input.value,previous);ui.reset();
 agent.detail.session.archivedAt=0;agent.detail.historyPreview=true;await open();document.querySelector('[data-tab="skills"]').click();assert.equal(button(skill(),'用于任务').disabled,true);ui.reset();agent.detail.historyPreview=false;
 await open();document.querySelector('[data-tab="skills"]').click();hashChanged=true;button(skill(),'查看').click();await tick();assert.match(document.querySelector('#guidance-status').textContent,/已变更/);assert.doesNotMatch(content().textContent,/LAZY_BODY/);hashChanged=false;
 // A result from the previous session may arrive after switching sessions.
 ui.reset();scanDeferred=true;await open();assert.equal(typeof pendingScan,'function');agent.selected='session-b';agent.detail={session:{id:'session-b',projectId:'project'}};ui.update();pendingScan({...snapshot,rules:{status:'loaded',content:'PRIVATE_PREVIOUS_SESSION'}});await tick();assert.equal(document.querySelector('#project-guidance-modal').hidden,true);assert.doesNotMatch(content().textContent,/PRIVATE_PREVIOUS_SESSION/);
 // Old account's lazy skill result cannot repopulate the reset modal.
 await open();document.querySelector('[data-tab="skills"]').click();skillDeferred=true;button(skill(),'查看').click();await tick();assert.equal(typeof pendingSkill,'function');agent.identityEpoch++;ui.reset();pendingSkill({instructions:'PRIVATE_PREVIOUS_ACCOUNT',nextOffset:null,sha256:'old'});await tick();assert.equal(document.querySelector('#project-guidance-modal').hidden,true);assert.doesNotMatch(content().textContent,/PRIVATE_PREVIOUS_ACCOUNT/);
 // The project label follows the session even when sidebar project selection differs.
 agent.project='different';await open();assert.match(document.querySelector('.guidance-hint').textContent,/Selected project/);assert.doesNotMatch(document.querySelector('.guidance-hint').textContent,/Wrong project/);
 document.querySelector('#project-guidance-modal').dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:'Escape',bubbles:true}));assert.equal(document.querySelector('#project-guidance-modal').hidden,true);
 dom.window.close();console.log('PASS: project guide metadata and lazy paged skill preview; safe text; changed-hash errors; preserved draft and no auto-send; readonly guards; stale session/account isolation; session project label.');
})().catch(error=>{console.error(error);dom.window.close();process.exitCode=1;});
