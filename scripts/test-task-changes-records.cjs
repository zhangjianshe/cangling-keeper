// Render recorded real-model runs with the production task panel, without executing actions.
const fs=require('fs'),assert=require('assert/strict'),{JSDOM}=require('jsdom');
for(const path of process.argv.slice(2)){
 const detail=JSON.parse(fs.readFileSync(path,'utf8')),dom=new JSDOM('<!doctype html><body></body>',{runScripts:'outside-only'}),w=dom.window,d=w.document;
 w.agent={selected:detail.sessionId,identityEpoch:0,detail};w.element=(tag,cls,text)=>{const e=d.createElement(tag);e.className=cls||'';if(text!==undefined)e.textContent=text;return e;};w.notice=()=>{};w.invoke=()=>{throw Error('No mutations in render validation');};w.select=()=>{};
 const source=fs.readFileSync('ui/agent.js','utf8');w.eval(source.slice(source.indexOf('  function renderTaskChanges('),source.indexOf('  function renderProjects()')));
 w.renderTaskChanges(d.body,detail.runs);
 const files=detail.runs.filter(r=>r.review?.kind==='file');assert.equal(d.querySelectorAll('button').length,files.length);
 for(const r of files){assert(d.body.textContent.includes(r.review.path));assert(d.body.textContent.includes(r.review.diff));}
 for(const r of detail.runs.filter(r=>r.tool==='run_command')){assert(d.body.textContent.includes(r.review.command));const result=JSON.parse(r.output);if(result.exitCode!=null)assert(d.body.textContent.includes('退出码 '+result.exitCode));}
 assert(!d.querySelector('script'));dom.window.close();console.log('PASS real task change panel:',path);
}
