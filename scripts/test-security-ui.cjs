const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),{JSDOM}=require('jsdom');
const dom=new JSDOM(fs.readFileSync('ui/index.html','utf8'),{runScripts:'outside-only'}),c=dom.getInternalVMContext(),d=dom.window.document,source=fs.readFileSync('ui/agent.js','utf8'),calls=[];
Object.assign(c,{$:s=>d.querySelector(s),agent:{selected:'session',detail:{securityMode:'ask',modelPending:false}},notice(){},closeModelMenu(){},ensureSession:async()=> 'session',invoke:async(name,args)=>{calls.push({name,args});c.agent.detail.securityMode=args.mode;},select:async()=>c.renderSecurity()});
vm.runInContext(source.slice(source.indexOf('  function element('),source.indexOf('  function notice(')),c);
vm.runInContext(source.slice(source.indexOf('  function securityOptions('),source.indexOf('  let llmConfig=')),c);
(async()=>{
 c.renderSecurity();assert.equal(d.querySelector('#agent-security-label').textContent,'请求批准');
 d.querySelector('#agent-security-picker').click();assert.equal(d.querySelector('#agent-security-menu').hidden,false);assert.equal(d.querySelectorAll('#agent-security-menu [role=menuitemradio]').length,3);
 assert.match(d.querySelector('#agent-security-menu').textContent,/未隔离的 Shell 命令仍需确认/);
 let pending=c.chooseSecurity('full');assert.equal(d.querySelector('#full-access-modal').classList.contains('hidden'),false);assert.equal(d.activeElement.id,'full-access-cancel');assert.match(d.querySelector('#full-access-description').textContent,/不提供操作系统沙箱/);d.querySelector('#full-access-cancel').click();await pending;assert.equal(calls.length,0,'cancel never changes access');
 pending=c.chooseSecurity('full');d.querySelector('#full-access-modal').dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:'Escape',bubbles:true}));await pending;assert.equal(calls.length,0,'Escape never changes access');
 await c.chooseSecurity('auto');assert.equal(calls[0].args.mode,'auto');assert.equal(d.querySelector('#agent-security-label').textContent,'自动审批');
 pending=c.chooseSecurity('full');d.querySelector('#full-access-confirm').click();await pending;assert.equal(calls[1].args.confirmFullAccess,true);assert.equal(d.querySelector('#agent-security-picker').dataset.mode,'full');
 c.agent.detail.modelPending=true;await c.chooseSecurity('ask');assert.equal(calls.length,2,'busy task cannot switch policy');c.renderSecurity();assert.equal(d.querySelector('#agent-security-picker').disabled,true);
 dom.window.close();console.log('PASS: three accessible modes, truthful scope descriptions, full-access confirmation/cancel, busy guard and selected state');
})().catch(e=>{console.error(e);dom.window.close();process.exitCode=1;});
