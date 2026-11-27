(() => {
 window.keeperProjectDialog=({project,invoke,notice,onChanged,deleteOnly=false})=>{
  const dialog=document.createElement('dialog');dialog.className='keeper-project-dialog';
  const heading=document.createElement('h2');heading.textContent=deleteOnly?'删除项目“'+project.name+'”':'管理项目';
  const label=document.createElement('label');label.textContent='项目名称';
  const name=document.createElement('input');name.type='text';name.value=project.name;name.maxLength=80;name.required=true;label.append(name);
  const path=document.createElement('p');path.className='keeper-project-path';path.textContent=project.directory;
  const hint=document.createElement('p');hint.className='keeper-project-hint';hint.textContent='项目在本机账户间共享。'+(deleteOnly?'':'保存名称时会同步重命名项目目录。')+'删除时将项目、全部会话（包括归档、回收站及其他本机账户的会话）和磁盘文件移入项目回收站，可整项恢复。执行中的会话须先停止。';
  const confirmLabel=document.createElement('label');confirmLabel.className='keeper-dialog-check';const confirmed=document.createElement('input');confirmed.type='checkbox';confirmLabel.append(confirmed,document.createTextNode('我确认将项目、全部会话和文件移入回收站'));
  const status=document.createElement('p');status.className='keeper-dialog-status';status.setAttribute('role','status');
  const actions=document.createElement('div');actions.className='keeper-project-actions';
  const buttons=[];let busy=false;
  function button(title,action){const b=document.createElement('button');b.type='button';b.className='btn';b.textContent=title;b.onclick=async()=>{if(busy)return;busy=true;buttons.forEach(v=>v.disabled=true);try{await action();}catch(e){status.textContent=String(e);}finally{busy=false;buttons.forEach(v=>v.disabled=false);remove.disabled=project.id==='general';}};buttons.push(b);actions.append(b);return b;}
  if(!deleteOnly)button('打开目录',async()=>{await invoke('agent_project_open',{projectId:project.id});status.textContent='已请求在 Keeper 主机的文件管理器中打开目录。';});
  const remove=button('删除项目',async()=>{
   if(!confirmed.checked){status.textContent='请先勾选确认将项目、全部会话和文件移入回收站。';return;}
   await invoke('agent_project_delete',{projectId:project.id,confirmed:true});await onChanged(true);dialog.close();notice('项目、会话和文件已移入回收站，可在项目回收站恢复。');
  });remove.classList.add('danger');remove.disabled=project.id==='general';
  button('取消',async()=>dialog.close());
  const save=!deleteOnly&&button('保存',async()=>{if(!name.value.trim() || !name.reportValidity())return;await invoke('agent_project_update',{projectId:project.id,name:name.value});await onChanged(false);dialog.close();});
  if(save)save.classList.add('primary');
  heading.id='keeper-project-dialog-title';dialog.setAttribute('aria-labelledby',heading.id);
  dialog.addEventListener('cancel',event=>{if(busy)event.preventDefault();});
  dialog.addEventListener('close',()=>dialog.remove());dialog.append(heading);if(!deleteOnly)dialog.append(label);dialog.append(path,hint,confirmLabel,status,actions);document.body.append(dialog);dialog.showModal();(deleteOnly?confirmed:name).focus();
 };
 function confirmPermanent(projects,all){
  return new Promise(resolve=>{
   const dialog=document.createElement('dialog');dialog.className='keeper-project-dialog';
   const title=document.createElement('h2');title.id='keeper-project-purge-title';title.textContent=all?'清空项目回收站':'彻底删除项目';dialog.setAttribute('aria-labelledby',title.id);
   const message=document.createElement('p');message.className='keeper-project-hint';message.textContent=(all?'将彻底删除回收站中的 '+projects.length+' 个项目':'将彻底删除项目“'+projects[0].name+'”')+'，包括全部会话（含其他本机账户、归档和回收站中的会话）、消息、附件、工具记录及磁盘目录中的所有文件。此操作无法撤销，也无法恢复。';
   const names=document.createElement('p');names.className='keeper-project-purge-names';names.textContent=all?projects.map(p=>p.name).join('、'):projects[0].directory;
   const actions=document.createElement('div');actions.className='keeper-project-actions';
   const cancel=document.createElement('button');cancel.className='btn';cancel.textContent='取消';cancel.onclick=()=>dialog.close();
   const confirm=document.createElement('button');confirm.className='btn danger';confirm.textContent=all?'确认清空回收站':'确认彻底删除';let confirmed=false;confirm.onclick=()=>{confirmed=true;dialog.close();};
   actions.append(cancel,confirm);dialog.append(title,message,names,actions);dialog.addEventListener('close',()=>{dialog.remove();resolve(confirmed);},{once:true});document.body.append(dialog);dialog.showModal();cancel.focus();
  });
 }
 window.keeperProjectTrash=async({invoke,notice,onChanged})=>{
  const dialog=document.createElement('dialog');dialog.className='keeper-project-dialog';
  const title=document.createElement('h2');title.id='keeper-project-trash-title';title.textContent='项目回收站';dialog.setAttribute('aria-labelledby',title.id);dialog.append(title);
  const hint=document.createElement('p');hint.className='keeper-project-hint';hint.textContent='恢复项目时，会同时恢复其全部会话与磁盘目录。彻底删除或清空回收站会永久删除项目、会话和磁盘文件，无法恢复。';dialog.append(hint);
  const list=document.createElement('div');dialog.append(list);
  const status=document.createElement('p');status.setAttribute('role','status');dialog.append(status);
  const actions=document.createElement('div');actions.className='keeper-project-actions';dialog.append(actions);
  const empty=document.createElement('button');empty.type='button';empty.className='btn danger';empty.textContent='清空回收站';actions.append(empty);
  const close=document.createElement('button');close.type='button';close.className='btn';close.textContent='关闭';close.onclick=()=>dialog.close();actions.append(close);
  let busy=false,projects=[];
  function controls(){for(const b of dialog.querySelectorAll('button'))b.disabled=busy;empty.disabled=busy||!projects.length;for(const b of dialog.querySelectorAll('[data-purge-pending]'))b.disabled=true;}
  async function run(action){
   if(busy)return;busy=true;controls();
   try{await action();}catch(e){try{await refresh();}catch(_){}status.textContent=String(e);}
   finally{busy=false;controls();}
  }
  empty.onclick=()=>run(async()=>{
   const snapshot=projects.slice();if(!snapshot.length || !await confirmPermanent(snapshot,true))return;
   await invoke('agent_project_trash_empty',{confirmed:true,projects:snapshot.map(({id,deletedAt})=>({id,deletedAt}))});await onChanged();await refresh();notice('回收站已清空，项目、会话和磁盘文件已彻底删除。');
  });
  dialog.addEventListener('cancel',e=>{if(busy)e.preventDefault();});dialog.addEventListener('close',()=>dialog.remove());document.body.append(dialog);dialog.showModal();
  async function refresh(){
   projects=await invoke('agent_project_trash_list');status.textContent='';list.replaceChildren();
   if(!projects.length){const noProjects=document.createElement('p');noProjects.textContent='回收站为空';list.append(noProjects);}
   for(const project of projects){
    const row=document.createElement('div');row.className='keeper-project-trash-row';
    const name=document.createElement('strong');name.textContent=project.name;
    const path=document.createElement('p');path.className='keeper-project-path';path.textContent=project.directory;
    const buttons=document.createElement('div');buttons.className='keeper-project-trash-actions';
    const restore=document.createElement('button');restore.type='button';restore.className='btn';restore.textContent='恢复项目';restore.setAttribute('aria-label','恢复项目 '+project.name);
    if(project.pendingPurge){restore.dataset.purgePending='true';const pending=document.createElement('p');pending.className='keeper-project-hint';pending.textContent='删除未完成：记录已删除，部分磁盘文件仍待清理，请重试彻底删除。';row.append(pending);}
    restore.onclick=()=>run(async()=>{await invoke('agent_project_restore',{projectId:project.id});await onChanged();await refresh();notice('项目、会话和文件已恢复。');});
    const purge=document.createElement('button');purge.type='button';purge.className='btn danger';purge.textContent=project.pendingPurge?'重试彻底删除':'彻底删除';purge.setAttribute('aria-label',purge.textContent+' '+project.name);
    purge.onclick=()=>run(async()=>{if(!await confirmPermanent([project],false))return;await invoke('agent_project_purge',{projectId:project.id,deletedAt:project.deletedAt,confirmed:true});await onChanged();await refresh();notice('项目、全部会话和磁盘文件已彻底删除。');});
    buttons.append(restore,purge);row.append(name,path,buttons);list.append(row);
   }
   controls();
  }
  try{await refresh();}catch(e){status.textContent=String(e);}
 };
})();
