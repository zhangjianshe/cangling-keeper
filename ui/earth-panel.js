(()=>{'use strict';
 window.createKeeperEarth=({agent,invoke,showConversation})=>{
 const node=(t,c,text)=>{const e=document.createElement(t);e.className=c||'';if(text!==undefined)e.textContent=text;return e;};
 const panel=node('section','keeper-earth hidden'),bar=node('header','earth-toolbar'),title=node('strong','','三维地球'),status=node('span','hint'),split=node('button','btn','对话分屏'),close=node('button','btn','返回会话');for(const b of [split,close])b.type='button';bar.append(title,status,split,close);panel.append(bar);document.querySelector('#agent-main').after(panel);
 let frame=null,project=null,owner=null,epoch=0,sequence=0,applySequence=0,ready=false,timer=null,polling=false,scene=null,view='',splitMode=false;const pending=new Map();
 const live=t=>t===sequence&&epoch===(agent.identityEpoch||0)&&!panel.classList.contains('hidden');
 function request(method,args={}){return new Promise((resolve,reject)=>{if(!frame||!ready)return reject(Error('地球正在初始化'));const id=crypto.randomUUID(),timeout=setTimeout(()=>{pending.delete(id);reject(Error('地球响应超时'));},10000);pending.set(id,{resolve,reject,timeout});frame.contentWindow.postMessage({earth:true,id,method,args},location.origin);});}
 window.addEventListener('message',e=>{if(e.origin!==location.origin||e.source!==frame?.contentWindow||!e.data?.earth||!live(sequence))return;if(e.data.event==='error'){status.textContent=e.data.value;return;}if(e.data.event==='selection'){if(scene)save({...scene,selection:e.data.value}).catch(error=>status.textContent=String(error));return;}if(e.data.event==='ready'){ready=true;status.textContent='地球已就绪';apply(scene).catch(error=>status.textContent=String(error));return;}const p=pending.get(e.data.id);if(!p)return;clearTimeout(p.timeout);pending.delete(e.data.id);e.data.error?p.reject(Error(e.data.error)):p.resolve(e.data.result);});
 async function apply(value){const t=sequence;if(!ready||!value)return;const a=++applySequence;await request('apply',value);if(!live(t)||a!==applySequence)return;scene=value;view=JSON.stringify(await request('get_view'));if(!live(t)||a!==applySequence)return;await invoke('agent_earth_ack',{accountKey:owner,projectId:project.id,revision:scene.revision});if(live(t))status.textContent='场景已显示';}
 async function save(value){const t=sequence,id=project.id;const next=await invoke('agent_earth_save',{accountKey:owner,projectId:id,expectedRevision:scene.revision,scene:value});if(live(t))await apply(next);}
 async function tick(){if(polling||!ready||document.hidden||!live(sequence))return;polling=true;const t=sequence,id=project.id;try{const remote=await invoke('agent_earth_get',{projectId:id});if(!live(t))return;if(remote.revision!==scene.revision){await apply(remote);return;}const camera=await request('get_view');if(!live(t))return;if(JSON.stringify(camera)!==view){const next=await invoke('agent_earth_save',{accountKey:owner,projectId:id,expectedRevision:scene.revision,scene:{...scene,camera}});if(!live(t))return;scene=next;view=JSON.stringify(camera);await invoke('agent_earth_ack',{accountKey:owner,projectId:id,revision:scene.revision});}}catch(e){if(live(t))status.textContent=String(e);}finally{polling=false;}}
 function remember(){if(agent.detachedEarth)return;try{if(project)localStorage.setItem("keeper-earth-view:"+owner,JSON.stringify({projectId:project.id,split:splitMode,workbench:document.body.classList.contains("earth-workbench")}));}catch{}}
 function hide(forget=true){if(forget&&owner&&!agent.detachedEarth){try{localStorage.removeItem("keeper-earth-view:"+owner);}catch{}}document.body.classList.remove('earth-workbench','earth-chat-collapsed');window.dispatchEvent(new Event('earth-layout'));sequence++;ready=false;clearInterval(timer);for(const p of pending.values()){clearTimeout(p.timeout);p.reject(Error('地球已关闭'));}pending.clear();frame?.remove();frame=null;panel.classList.add('hidden');document.querySelector('#agent-main').classList.remove('earth-chat-split');panel.classList.remove('earth-split');project=null;scene=null;}
 async function open(value,restoreSplit=false,workbench=false){hide(false);const t=sequence;project=value;epoch=agent.identityEpoch||0;owner=agent.ownerKey||'guest';showConversation(true);if(t!==sequence)return;agent.projectBrowser?.hide();agent.profiles?.hide();title.textContent=value.name+' · 三维地球';panel.classList.remove('hidden');document.querySelector('#agent-main').classList.add('hidden');splitMode=false;split.textContent='对话分屏';split.disabled=agent.detail?.session?.projectId!==value.id;split.title=split.disabled?'先选择该项目的会话，再打开地球分屏':'';setSplit(restoreSplit&&!split.disabled);if(workbench)setWorkbench();remember();status.textContent='加载地球…';try{const data=await invoke('agent_earth_get',{projectId:value.id});if(!live(t))return;scene=data;frame=node('iframe','earth-frame');frame.title='项目三维地球';frame.name='keeper-earth-'+crypto.randomUUID();// Use the verified compatibility backend in the embedded viewer; avoid repeated Vulkan allocation failures.
 frame.allow='fullscreen';panel.append(frame);await new Promise(resolve=>(window.requestAnimationFrame||((fn)=>setTimeout(fn,0)))(resolve));if(!live(t))return;frame.src='/earth/index.html?backend=webgl2';setTimeout(()=>recoverBlankFrame(t,frame),1500);timer=setInterval(tick,1500);setTimeout(()=>{if(live(t)&&!ready)status.textContent='地球初始化较慢，请检查 iframe 内错误及 GPU 支持；可返回会话后重试。';},20000);}catch(e){if(live(t))status.textContent=String(e);}}
 close.onclick=()=>{hide();showConversation();};function setSplit(value){splitMode=value;const main=document.querySelector('#agent-main');main.classList.toggle('hidden',!splitMode);main.classList.toggle('earth-chat-split',splitMode);panel.classList.toggle('earth-split',splitMode);split.textContent=splitMode?'仅地球':'对话分屏';remember();}
 split.onclick=()=>setSplit(!splitMode);

 async function recoverBlankFrame(t,candidate){
   if(!live(t)||ready||candidate!==frame)return;
   try{
     if(candidate.contentDocument?.URL!=='about:blank')return;
     // Recover only an un-navigated frame, using the trusted local viewer document.
     const response=await fetch('/earth/index.html',{redirect:'error'});
     if(!response.ok)throw Error('无法加载地球页面');
     const html=await response.text();
     if(!live(t)||ready||candidate!==frame)return;
     candidate.srcdoc=html.replace('<head>','<head><base href="'+location.origin+'/earth/index.html?backend=webgl2">');
   }catch(error){if(live(t))status.textContent=String(error);}
 }
 async function restore(){const restoreEpoch=agent.identityEpoch||0;if(document.readyState!=='complete')await new Promise(resolve=>window.addEventListener('load',resolve,{once:true}));if(restoreEpoch!==(agent.identityEpoch||0))return;try{const saved=JSON.parse(localStorage.getItem('keeper-earth-view:'+(agent.ownerKey||'guest'))||'null');const value=agent.projects?.find(p=>p.id===saved?.projectId);if(value){if(saved.workbench)await agent.earthWorkbench?.restoreProject(value);await open(value,!!saved.split,!!saved.workbench);}}catch{}}
 function setWorkbench(){setSplit(true);document.body.classList.add('earth-workbench');remember();window.dispatchEvent(new Event('earth-layout'));}
 async function openWorkbench(value){if(project?.id===value.id&&!panel.classList.contains('hidden'))setWorkbench();else await open(value,true,true);}
 function showWithConversation(id){if(project?.id!==id||panel.classList.contains('hidden'))return false;setSplit(true);return true;}
 async function flyTo(position){
   if(!project||!live(sequence))throw Error('请先打开项目三维地球');
   const t=sequence,id=project.id,accountKey=owner;
   const current=await invoke('agent_earth_get',{projectId:id});if(!live(t))throw Error('项目已切换');
   const next=await invoke('agent_earth_fly_to',{projectId:id,accountKey,expectedRevision:current.revision,longitude:position.longitude,latitude:position.latitude,...(position.height===undefined?{}:{height:position.height})});
   if(!live(t))throw Error('项目已切换');await apply(next);
   if(!live(t))throw Error('项目已切换');const confirmed=await invoke('agent_earth_get',{projectId:id});
   if(!live(t))throw Error('项目已切换');
   return {projectId:id,revision:next.revision,status:confirmed.renderedRevision===next.revision?'rendered':'saved_not_rendered'};
 }
 window.keeperEarth={flyTo,getView:async()=>{if(!project)throw Error('请先打开项目三维地球');return invoke('agent_earth_get',{projectId:project.id});}};
 document.addEventListener('visibilitychange',()=>{if(ready)request('pause',document.hidden).catch(()=>{});});
 return {flyTo,open,openWorkbench,hide,restore,showWithConversation,reset:()=>hide(false),isSplit:()=>splitMode&&!!frame,projectId:()=>project?.id};
 };
})();
