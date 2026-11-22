const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
(async()=>{
 const sockets=[],timers=[];let calls=0,restored=0;
 class Socket{constructor(){sockets.push(this)}close(){this.onclose()}}
 const window={};vm.runInNewContext(fs.readFileSync('ui/web-adapter.js','utf8'),{window,location:{protocol:'http:',host:'localhost'},WebSocket:Socket,clearTimeout(){},setTimeout(fn){timers.push(fn)},fetch(){calls++;throw Error('unexpected request')}});
 await window.__KEEPER__.event.listen('keeper-transport-restored',()=>restored++);
 sockets[0].onopen();assert.equal(restored,0);sockets[0].onclose();timers.shift()();sockets[1].onopen();assert.equal(restored,1);assert.equal(calls,0);
 window.__KEEPER__.shutdownTransport();assert.equal(timers.length,0);
 const source=fs.readFileSync('ui/agent.js','utf8'),start=source.indexOf("  window.__KEEPER__.event.listen('keeper-transport-restored'"),end=source.indexOf('  let loginIdentity=null;',start);
 for(const changed of [false,true]){
  let handler;const reads=[],selected=[],agent={identityEpoch:1,ownerKey:'old',selected:'session'};let reset=0;
  const context={agent,window:{__KEEPER__:{event:{listen(name,fn){handler=fn}}},dispatchEvent(){}},CustomEvent:class{},invoke:async command=>{reads.push(command);return command==='agent_bootstrap'?{accountKey:changed?'new':'old'}:{}},bootstrap:async()=>{},select:async id=>selected.push(id),refreshAccountSessions:async()=>reset++,notice:message=>{throw Error(message)}};
  vm.runInNewContext(source.slice(start,end),context);await handler();assert.equal(reset,changed?1:0);assert.deepEqual(selected,changed?[]:['session']);assert.deepEqual(reads,['agent_bootstrap','get_login_status']);
 }
 console.log('PASS: reconnect notification, shutdown, snapshot recovery, account isolation; no generation/tool replay');
})().catch(error=>{console.error(error);process.exit(1)});
