const assert=require('node:assert/strict'),{JSDOM}=require('jsdom');
const dom=new JSDOM('<textarea id="agent-input"></textarea><main></main>');global.window=dom.window;global.document=dom.window.document;const timers=[];global.setInterval=f=>timers.push(f);require('../ui/run-experience.js');
(async()=>{
 let saves=0,notice='';const agent={selected:'s',identityEpoch:0,detail:{session:{}},drafts:{flush:async()=>saves++}},api=window.createKeeperRunExperience({agent,notice:s=>notice=s});
 const run={id:'r',tool:'run_command',arguments:{command:'echo "<script>"'},status:'running',createdAt:1000,updatedAt:61000,output:'A'.repeat(4095)+'😀'+'B'.repeat(4200)};
 const main=document.querySelector('main'),input=document.querySelector('textarea');
 function render(value=run){main.replaceChildren();const card=document.createElement('details');card.append(document.createElement('summary'));main.append(card);api.decorate(card,value);api.tick(121000);return card;}
 let card=render();assert.equal(card.querySelector('.agent-run-elapsed').textContent,'已用时 2分0秒');assert(!card.querySelector('.agent-prepare-retry'));assert(card.querySelector('.agent-output-toolbar').textContent.includes('3/3'));
 card.querySelector('.agent-output-toolbar button').click();assert.equal(card.querySelector('pre').textContent,'B'.repeat(4096));
 card.querySelector('.agent-output-toolbar button').click();assert(card.querySelector('pre').textContent.endsWith('😀'));assert.equal(Array.from(card.querySelector('pre').textContent).length,4096);
 card=render({...run,output:run.output+'new'});assert(card.querySelector('.agent-output-toolbar').textContent.includes('1/3'));assert(card.querySelector('pre').textContent.endsWith('😀'));
 card=render({...run,status:'failed'});assert.equal(card.querySelector('.agent-run-elapsed').textContent,'历时 1分0秒');input.value='existing draft';card.querySelector('.agent-prepare-retry').click();await new Promise(r=>setImmediate(r));assert(input.value.startsWith('existing draft\n\n'));assert(input.value.includes(JSON.stringify(run.arguments,null,2)));assert.equal(saves,1);assert(!card.querySelector('script'));assert(notice.includes('尚未执行'));
 card.querySelector('.agent-prepare-retry').click();await new Promise(r=>setImmediate(r));assert.equal(saves,1);
 input.value='';agent.detail.modelPending=true;card=render({...run,status:'failed'});assert(card.querySelector('.agent-prepare-retry').disabled);agent.detail.modelPending=false;api.tick();assert(!card.querySelector('.agent-prepare-retry').disabled);
 agent.detail.session.archivedAt=1;api.tick();assert(card.querySelector('.agent-prepare-retry').disabled);agent.detail.session={};agent.selected='other';card.querySelector('.agent-prepare-retry').onclick();assert.equal(input.value,'');
 agent.selected='s';input.value='x'.repeat(32000);card=render({...run,status:'stopped'});card.querySelector('.agent-prepare-retry').click();assert.equal(input.value.length,32000);assert(notice.includes('超过'));
 api.reset();card=render();assert(card.querySelector('.agent-output-toolbar').textContent.includes('3/3'));console.log('PASS: elapsed state, Unicode-safe output pages, live paging retention, draft-only retry, no replay, stale/archived/busy guards and size limit');
})().catch(e=>{console.error(e);process.exitCode=1});
