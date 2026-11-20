const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),{JSDOM}=require('jsdom');
const dom=new JSDOM(fs.readFileSync('ui/index.html','utf8'),{runScripts:'outside-only'}),c=dom.getInternalVMContext(),d=dom.window.document,source=fs.readFileSync('ui/agent.js','utf8'),calls=[];
Object.assign(c,{$:s=>d.querySelector(s),agent:{selected:'session',sessions:[{id:'session',title:'原始标题'}]},notice(){},bootstrap:async()=>{},invoke:async(name,args)=>{calls.push({name,args});}});
vm.runInContext(source.slice(source.indexOf('  function element('),source.indexOf('  function notice(')),c);
const start=source.indexOf("  $('#agent-session-title').onclick=");vm.runInContext(source.slice(start,source.indexOf('  function renderProjects(',start)),c);
(async()=>{
 const button=d.querySelector('#agent-session-title');button.click();let input=d.querySelector('.agent-title-editor input');assert.equal(input.value,'原始标题');input.value='取消';input.dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:'Escape',bubbles:true}));assert.equal(calls.length,0);assert.equal(button.hidden,false);
 button.click();input=d.querySelector('.agent-title-editor input');input.value='😀'.repeat(129);input.dispatchEvent(new dom.window.Event('input'));assert.equal(Array.from(input.value).length,128);input.dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:'Enter',bubbles:true}));await new Promise(r=>setTimeout(r,0));assert.equal(calls.length,1);assert.equal(calls[0].name,'agent_rename_session');assert.equal(Array.from(calls[0].args.title).length,128);assert.equal(button.hidden,false);
 button.click();input=d.querySelector('.agent-title-editor input');input.value='点击外部保存';input.dispatchEvent(new dom.window.Event('blur'));await new Promise(r=>setTimeout(r,0));assert.equal(calls.length,2);assert.equal(calls[1].args.title,'点击外部保存');
 assert.ok(!source.includes("element('a','','了解更多')"));dom.window.close();console.log('PASS: click title, prefill, Escape cancel, Unicode limit, Enter/blur save once, help link removed');
})().catch(e=>{console.error(e);dom.window.close();process.exitCode=1;});
