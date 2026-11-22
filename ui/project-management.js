(() => {
 window.keeperProjectDialog=({project,invoke,notice,onChanged})=>{
  const dialog=document.createElement('dialog');dialog.className='keeper-project-dialog';
  const heading=document.createElement('h2');heading.textContent='管理项目';
  const label=document.createElement('label');label.textContent='项目名称';
  const name=document.createElement('input');name.type='text';name.value=project.name;name.maxLength=80;name.required=true;label.append(name);
  const path=document.createElement('p');path.className='keeper-project-path';path.textContent=project.directory;
  const hint=document.createElement('p');hint.className='keeper-project-hint';hint.textContent='项目在本机账户间共享。改名不会移动目录。仅可删除无会话的项目（包括归档、回收站及其他账户会话）；磁盘目录和文件始终保留。';
  const confirmLabel=document.createElement('label');confirmLabel.className='keeper-dialog-check';const confirmed=document.createElement('input');confirmed.type='checkbox';confirmLabel.append(confirmed,document.createTextNode('删除时仅移除项目记录，保留磁盘文件'));
  const status=document.createElement('p');status.className='keeper-dialog-status';status.setAttribute('role','status');
  const actions=document.createElement('div');actions.className='keeper-project-actions';
  const buttons=[];let busy=false;
  function button(title,action){const b=document.createElement('button');b.type='button';b.className='btn';b.textContent=title;b.onclick=async()=>{if(busy)return;busy=true;buttons.forEach(v=>v.disabled=true);try{await action();}catch(e){status.textContent=String(e);}finally{busy=false;buttons.forEach(v=>v.disabled=false);remove.disabled=project.id==='general';}};buttons.push(b);actions.append(b);return b;}
  button('打开目录',async()=>{await invoke('agent_project_open',{projectId:project.id});status.textContent='已请求在 Keeper 主机的文件管理器中打开目录。';});
  const remove=button('删除项目',async()=>{
   if(!confirmed.checked){status.textContent='请先勾选删除说明，确认保留磁盘文件。';return;}
   await invoke('agent_project_delete',{projectId:project.id,confirmed:true});await onChanged(true);dialog.close();notice('项目记录已删除，磁盘文件已保留。');
  });remove.classList.add('danger');remove.disabled=project.id==='general';
  button('取消',async()=>dialog.close());
  const save=button('保存',async()=>{if(!name.value.trim() || !name.reportValidity())return;await invoke('agent_project_update',{projectId:project.id,name:name.value});await onChanged(false);dialog.close();});
  save.classList.add('primary');
  heading.id='keeper-project-dialog-title';dialog.setAttribute('aria-labelledby',heading.id);
  dialog.addEventListener('cancel',event=>{if(busy)event.preventDefault();});
  dialog.addEventListener('close',()=>dialog.remove());dialog.append(heading,label,path,hint,confirmLabel,status,actions);document.body.append(dialog);dialog.showModal();name.focus();
 };
})();
