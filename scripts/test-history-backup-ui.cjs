const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const {JSDOM}=require('jsdom');
const dom=new JSDOM(fs.readFileSync('ui/index.html','utf8'),{runScripts:'outside-only',url:'http://localhost:4301'}),context=dom.getInternalVMContext(),document=dom.window.document;
const archive={snapshot:{sessions:[{id:'s'}]},sha256:'fixture'},calls=[];let confirmed=true,downloads=0,refreshes=0;
Object.assign(context,{$:s=>document.querySelector(s),agent:{identityEpoch:0},invoke:async n=>{calls.push(n);return archive;},uiConfirm:async()=>confirmed,bootstrap:async()=>{refreshes++;},fetch:async(url,options)=>{const args=JSON.parse(options.body);calls.push({url,args});return {ok:true,status:200,json:async()=>({ok:true,result:{projects:1,sessions:1,messages:2,runs:1,accountKey:'owner',differentAccount:false}})};}});
context.URL.createObjectURL=()=> 'blob:fixture';context.URL.revokeObjectURL=()=>{};dom.window.HTMLAnchorElement.prototype.click=function(){downloads++;assert.match(this.download,/cangling-keeper-history/);};
const source=fs.readFileSync('ui/agent.js','utf8'),start=source.indexOf('  function historyBusy('),end=source.indexOf('  let editingMcp=',start);assert.ok(start>0 && end>start);vm.runInContext(source.slice(start,end),context);
async function choose(text){await document.querySelector('#history-backup-file').onchange({target:{files:[{size:text.length,text:async()=>text}]}});}
(async()=>{
 await document.querySelector('#history-backup').onclick();assert.equal(downloads,1);assert.deepEqual(calls,['agent_history_backup']);
 calls.length=0;await choose(JSON.stringify(archive));assert.equal(calls.length,2);assert.equal(calls[0].url,'/api/history/restore');assert.equal(calls[0].args.mode,'preview');assert.equal(calls[1].args.mode,'restore');assert.equal(calls[1].args.expectedAccount,'owner');assert.equal(calls[1].args.confirmed,true);assert.equal(refreshes,1);
 calls.length=0;confirmed=false;await choose(JSON.stringify(archive));assert.equal(calls.length,1,'cancel must not restore');
 calls.length=0;await choose('invalid json');assert.equal(calls.length,0);assert.match(document.querySelector('#history-backup-status').textContent,/恢复失败/);assert.equal(document.querySelector('#history-restore').disabled,false);
 dom.window.close();console.log('PASS: backup download; preview and explicit restore confirmation; cancellation; bad file handling; session list refresh.');
})().catch(e=>{console.error(e);dom.window.close();process.exitCode=1;});
