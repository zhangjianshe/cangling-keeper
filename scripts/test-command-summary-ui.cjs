const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const {JSDOM}=require('jsdom'),dom=new JSDOM('<main></main>',{runScripts:'outside-only'}),context=dom.getInternalVMContext(),document=dom.window.document;
context.agent={detail:{modelPending:false}};
context.element=(tag,cls='',text='')=>{const n=document.createElement(tag);n.className=cls;n.textContent=text;return n;};
const source=fs.readFileSync('ui/agent.js','utf8');vm.runInContext(source.slice(source.indexOf('  function renderTaskChanges('),source.indexOf('  function renderProjects()')),context);
const run=(status,output)=>({tool:'run_command',status,arguments:{command:'<img src=x> test'},output:typeof output==='string'?output:JSON.stringify(output)});
const runs=[run('succeeded',{exitCode:0}),run('succeeded',{exitCode:2}),run('failed',{exitCode:0}),run('stopped',{exitCode:0}),run('interrupted',{exitCode:0}),run('rejected',{exitCode:0}),run('running',{exitCode:0}),run('pending_approval',{}),run('approved',{}),run('succeeded',{exitCode:null}),run('succeeded',{exitCode:'0'}),run('succeeded','null'),run('succeeded','broken'),run('succeeded',[]),run('failed',{timedOut:true}),run('succeeded',{stopped:true,exitCode:0})];
function render(records){const root=document.querySelector('main');root.replaceChildren();context.renderTaskChanges(root,records);return root;}
let root=render(runs);assert.match(root.querySelector('.agent-command-summary').textContent,/正常退出 1 · 失败\/超时 3 · 停止\/中断 3 · 未执行 1 · 待处理 3 · 待核实 5/);
assert.equal(root.querySelector('img'),null);assert.match(root.textContent,/不代表测试全部通过/);assert.equal(root.querySelectorAll('.agent-command-record').length,runs.length);assert.ok([...root.querySelectorAll('.agent-command-record')].every(n=>!n.open));
const before=root.textContent;root=render(JSON.parse(JSON.stringify(runs)));assert.equal(root.textContent,before);
root=render([]);assert.equal(root.children.length,0);
dom.window.close();console.log('PASS command evidence: lifecycle precedence, unknown legacy results, counts, safe text and snapshot restore');
