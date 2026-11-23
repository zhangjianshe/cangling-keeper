const fs=require('fs'),assert=require('assert/strict'),{JSDOM}=require('jsdom');
const dom=new JSDOM('<!doctype html><main id="agent-main"></main>',{url:'http://localhost',runScripts:'outside-only'}),w=dom.window,d=w.document;
w.eval(fs.readFileSync('ui/earth-panel.js','utf8'));
const scene={revision:0,camera:{longitude:108,latitude:34,height:1000000},layers:{source:0,boundaries:true,graticule:false},overlays:[]};
const agent={projects:[{id:'p',name:'Project'}],identityEpoch:0,ownerKey:'guest',detail:{session:{projectId:'p'}}},calls=[];let deferred=null;
const earth=w.createKeeperEarth({agent,showConversation(){},invoke:async(name,args)=>{calls.push([name,args]);if(deferred&&name==='agent_earth_get')return new Promise(resolve=>deferred=resolve);return JSON.parse(JSON.stringify(scene));}});
const flush=()=>new Promise(r=>setTimeout(r,0));
function send(frame,data,origin='http://localhost',source=frame.contentWindow){w.dispatchEvent(new w.MessageEvent('message',{origin,source,data:{earth:true,...data}}));}
(async()=>{
 await earth.open({id:'p',name:'Project'});const f=d.querySelector('iframe');assert(f);assert.equal(new URL(f.src).searchParams.get('backend'),'webgl2');assert.equal(d.querySelectorAll('.keeper-earth input,.keeper-earth select,.earth-controls,.earth-capture').length,0);assert.deepEqual([...d.querySelectorAll('.keeper-earth button')].map(b=>b.textContent),['对话分屏','返回会话']);assert(d.querySelector('#agent-main').classList.contains('hidden'));
 const commands=[];f.contentWindow.postMessage=msg=>{commands.push(msg);setTimeout(()=>send(f,{id:msg.id,result:msg.method==='get_view'?scene.camera:true}),0);};
 send(f,{event:'ready'},'https://attacker.invalid');await flush();assert.equal(commands.length,0);
 send(f,{event:'ready'},'http://localhost',w);await flush();assert.equal(commands.length,0);
 send(f,{event:'ready'});for(let i=0;i<8;i++)await flush();assert.equal(calls.filter(c=>c[0]==='agent_earth_ack').length,1);assert.equal(calls.at(-1)[1].accountKey,'guest');
 [...d.querySelectorAll('button')].find(b=>b.textContent==='对话分屏').click();assert(earth.isSplit());assert(d.querySelector('#agent-main').classList.contains('earth-chat-split'));
 assert(earth.showWithConversation('p'));assert.equal(d.querySelector('iframe'),f);assert(!earth.showWithConversation('other'));
 earth.reset();assert.equal(d.querySelector('iframe'),null);await earth.restore();assert(d.querySelector('iframe'));assert(earth.isSplit());earth.hide();await earth.restore();assert.equal(d.querySelector('iframe'),null);
 // Late responses from a former project/account must never recreate the canvas.
 deferred=true;const opening=earth.open({id:'q',name:'Other'});await flush();agent.identityEpoch++;earth.reset();deferred(scene);await opening;assert.equal(d.querySelector('iframe'),null);assert(d.querySelector('.keeper-earth').classList.contains('hidden'));
 dom.window.close();console.log('PASS Earth UI: origin/source checks, rendered acknowledgment, split, stale account response and iframe teardown');
})().catch(e=>{console.error(e);dom.window.close();process.exitCode=1});
