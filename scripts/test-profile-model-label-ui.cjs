const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');const {JSDOM}=require('jsdom');
const dom=new JSDOM(fs.readFileSync('ui/index.html','utf8'),{runScripts:'outside-only'}),c=dom.getInternalVMContext(),d=dom.window.document;
const src=fs.readFileSync('ui/agent.js','utf8');Object.assign(c,{$:s=>d.querySelector(s),agent:{detail:{}},config:{model:'deepseek-flash',name:'DeepSeek'},enabled:true,available:[{}],picker:d.querySelector('#agent-model-picker')});
const start=src.indexOf('    const followsProfile='),end=src.indexOf('    picker.title=',start);const code=src.slice(start,end);
function render(profile){c.agent.detail.agentProfile=profile;vm.runInContext('{'+code+'}',c);}
render({id:'java'});assert.equal(c.picker.hidden,true);assert.equal(c.picker.disabled,true);assert.equal(d.querySelector('#agent-profile-model-label').textContent,'deepseek-flash · 跟随智能体');assert.equal(d.querySelector('#agent-profile-model-label svg'),null);assert.equal(d.querySelector('#agent-profile-model-label').tagName,'SPAN');
render(null);assert.equal(c.picker.hidden,false);assert.equal(c.picker.disabled,false);assert.equal(d.querySelector('#agent-profile-model-label').hidden,true);
c.agent.detail.modelPending=true;render(null);assert.equal(c.picker.disabled,true);assert.equal(c.picker.hidden,false);
c.config.model='<img src=x>';render({id:'java'});assert.equal(d.querySelector('#agent-profile-model-label img'),null);
dom.window.close();console.log('PASS profile model label: readonly semantics, generic picker restore, pending lock and safe text');
