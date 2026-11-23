const fs=require('fs'),assert=require('assert/strict'),{JSDOM}=require('jsdom');
const dom=new JSDOM('<div class="agent-header-tools"></div>',{runScripts:'outside-only'}),w=dom.window,d=w.document;
w.eval(fs.readFileSync('ui/agent-profiles.js','utf8'));
const p={id:'p',name:'<b>Agent</b>',systemPrompt:'Be precise',modelId:'m',skills:['skill'],mcpTools:['mcp_one']};
const agent={selected:'s',identityEpoch:0,detail:{agentProfiles:[p],agentProfile:p}},calls=[];
let deferred=null,selected='';
const invoke=async(n,a)=>{calls.push([n,a]);if(deferred)return new Promise(r=>deferred=r);if(n==='agent_profile_options')return {profiles:[p],selected:p,models:[{id:'m',name:'Model',model:'fixture',enabled:true,hasApiKey:true}],skills:[{name:'skill',description:'guide'}],mcpTools:[{function:{name:'mcp_one',description:'MCP fixture'}}]};return {};};
const element=(t,c,text)=>{const e=d.createElement(t);e.className=c||'';if(text!==undefined)e.textContent=text;return e;};
const ui=w.createKeeperProfiles({agent,invoke,element,notice:()=>{},select:async s=>selected=s,ensureSession:async()=>{}});
(async()=>{
 ui.update();const picker=d.querySelector('#agent-profile-picker');assert.equal(picker.value,'p');assert.equal(d.querySelector('b'),null);
 agent.detail.modelPending=true;ui.update();assert(picker.disabled);agent.detail.modelPending=false;ui.update();
 await ui.open();assert.equal(d.querySelector('#profile-name').value,p.name);assert.equal(d.querySelector('#profile-model').value,'m');assert.equal(d.querySelectorAll('input:checked').length,2);
 d.querySelector('#profile-name').value='Updated';await d.querySelector('#agent-profiles-modal form').onsubmit({preventDefault(){}});const saved=calls.find(([n])=>n==='agent_profile_save')[1];assert.equal(saved.name,'Updated');assert.equal(JSON.stringify(saved.skills),JSON.stringify(['skill']));assert.equal(selected,'s');
 deferred=true;const pending=ui.open();await new Promise(setImmediate);agent.identityEpoch++;ui.reset();deferred({profiles:[p],selected:p,models:[],skills:[],mcpTools:[]});await pending;assert(d.querySelector('#agent-profiles-modal').classList.contains('hidden'));assert.equal(d.querySelector('#profile-prompt').value,'');
 dom.window.close();console.log('PASS presets UI: safe labels, select/pending, populated edit form, save payload and stale account response');
})().catch(e=>{console.error(e);process.exitCode=1;dom.window.close();});
