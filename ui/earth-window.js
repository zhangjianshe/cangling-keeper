(()=>{'use strict';
 const params=new URLSearchParams(location.search),projectId=params.get('project'),expectedOwner=params.get('account');
 const status=document.querySelector('#window-status'),agent={identityEpoch:0,detachedEarth:true,detail:{},projects:[]};let earth=null,stopped=false;
 async function invoke(name,args={}){
   if(stopped)throw Error('地球窗口已停止同步，请刷新页面');
   const response=await fetch('/api/invoke/'+name,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(name.startsWith('agent_earth_')?{...args,accountKey:agent.ownerKey}:args)});
   const data=await response.json();
   if(!response.ok||!data.ok){const message=String(data.error||'地球请求失败');if(message.includes('账户已切换')){stopped=true;agent.identityEpoch++;earth?.reset();status.textContent='登录账户已切换，地球已停止同步。请从对话重新打开地球窗口。';}throw Error(message);}
   return data.result;
 }
 async function start(){try{
   if(!projectId)throw Error('缺少项目，请从对话页面打开地球新标签页');
   const data=await invoke('agent_bootstrap');agent.ownerKey=data.accountKey||'guest';
   if(expectedOwner && expectedOwner!==agent.ownerKey)throw Error('登录账户已切换，请从对话重新打开地球窗口');
   agent.projects=data.projects;let project=data.projects.find(p=>p.id===projectId);
   if(!project)throw Error('项目不在当前项目列表中，请在对话中选择项目后重新打开');
   document.title=project.name+' · 三维地球';earth=window.createKeeperEarth({agent,invoke,showConversation(){}});await earth.open(project);status.textContent='';
 }catch(error){status.textContent=String(error);}}
 window.addEventListener('pagehide',()=>{stopped=true;earth?.reset();});start();
})();
