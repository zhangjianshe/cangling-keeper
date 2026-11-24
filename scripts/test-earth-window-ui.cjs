const fs=require('fs'),assert=require('assert/strict'),{JSDOM}=require('jsdom');
(async()=>{
 const dom=new JSDOM(fs.readFileSync('ui/earth-window.html','utf8'),{url:'http://localhost/earth-window.html?project=p&account=guest',runScripts:'outside-only'}),w=dom.window;
 let config,opened,reset=0,changed=false;const calls=[];
 w.fetch=async(url,options)=>{calls.push([url,JSON.parse(options.body)]);return {ok:!changed,json:async()=>changed?{ok:false,error:'账户已切换，请重新打开地球页面'}:{ok:true,result:url.endsWith('agent_bootstrap')?{accountKey:'guest',projects:[{id:'p',name:'测试项目'}]}:{revision:2}}};};
 w.createKeeperEarth=c=>{config=c;return {open:async p=>opened=p,reset:()=>reset++};};
 w.eval(fs.readFileSync('ui/earth-window.js','utf8'));await new Promise(r=>setTimeout(r,10));assert.equal(opened.id,'p');assert(config.agent.detachedEarth);assert.equal(w.document.title,'测试项目 · 三维地球');
 await config.invoke('agent_earth_get',{projectId:'p'});assert.equal(calls.at(-1)[1].accountKey,'guest');
 changed=true;await assert.rejects(config.invoke('agent_earth_get',{projectId:'p'}));assert.equal(reset,1);assert.match(w.document.querySelector('#window-status').textContent,/账户已切换/);
 const count=calls.length;await assert.rejects(config.invoke('agent_earth_get',{projectId:'p'}));assert.equal(calls.length,count,'account change stops further requests');
 dom.window.close();console.log('PASS detached Earth: URL project restore, account pinning, teardown and stopped requests');
})().catch(e=>{console.error(e);process.exitCode=1;});
