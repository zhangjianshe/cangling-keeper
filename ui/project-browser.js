(() => {
 window.createKeeperProjectBrowser=({invoke,agent,showConversation})=>{
  const node=(tag,cls,text)=>{const el=document.createElement(tag);el.className=cls||'';if(text!==undefined)el.textContent=text;return el;};
  const iconButton=(label,path,primary=false)=>{
   const button=node('button','btn project-browser-icon-button'+(primary?' primary':''));button.type='button';button.title=label;button.setAttribute('aria-label',label);
   const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');for(const [key,value] of Object.entries({viewBox:'0 0 24 24',fill:'none',stroke:'currentColor','stroke-width':'1.7','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true'}))svg.setAttribute(key,value);
   const shape=document.createElementNS(svg.namespaceURI,'path');shape.setAttribute('d',path);svg.append(shape);button.append(svg);return button;
  };
  const panel=node('section','keeper-project-browser hidden');panel.setAttribute('aria-label','项目目录');
  const header=node('header','keeper-settings-header'),back=node('button','agent-nav-button','← 返回会话'),title=node('h1','','项目目录');back.type='button';back.onclick=showConversation;header.append(back,title);
  const location=node('p','project-browser-location'),bar=node('div','project-browser-bar'),up=iconButton('上级目录','m6 10 6-6 6 6M12 4v16'),refresh=iconButton('刷新','M20 7v5h-5M4 17v-5h5M6.1 6.1A8 8 0 0 1 20 12M4 12a8 8 0 0 0 13.9 5.9'),status=node('p','project-browser-status');status.setAttribute('role','status');bar.append(up,location,status,refresh);
  const layout=node('div','project-browser-layout'),list=node('nav','project-browser-list'),preview=node('section','project-browser-preview'),code=node('div','project-code-editor');list.setAttribute('aria-label','目录内容');preview.append(code);layout.append(list,preview);panel.append(header,bar,layout);document.querySelector('#agent-main').after(panel);
  const save=iconButton('保存文件','M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h12l4 4v12a2 2 0 0 1-2 2ZM7 3v6h10V3M7 21v-8h10v8',true);bar.append(save);save.disabled=true;
  const editor=window.ace.edit(code);window.ace.config.set('basePath','/vendor/ace');editor.setTheme('ace/theme/github');editor.session.setUseWorker(false);editor.setOptions({fontSize:15,showPrintMargin:false});editor.setReadOnly(true);
  let guidanceMode=false,guidanceSession=null;
  let project=null,path='.',file=null,seq=0,popup=null,trigger=null,original='',hash='',editable=false,saving=false,creating=false;
  const dirty=()=>!!file&&editor.getValue()!==original;
  function updateSave(){save.disabled=!editable||!dirty()||saving;const label=saving?'正在保存…':dirty()?'保存文件（有未保存修改）':'保存文件';save.title=label;save.setAttribute('aria-label',label);save.setAttribute('aria-busy',String(saving));}
  editor.session.on('change',updateSave);
  const discard=async()=>!dirty()||await window.uiConfirm('当前文件有未保存的修改，放弃修改并继续？','未保存的修改');
  window.addEventListener('beforeunload',e=>{if(dirty()){e.preventDefault();e.returnValue='';}});
  save.onclick=async()=>{if(save.disabled)return;const ownerProject=project,ownerFile=file,epoch=agent.identityEpoch,text=editor.getValue();saving=true;updateSave();try{const data=await invoke('agent_project_file_save',{projectId:project.id,path:file,content:text,sha256:hash});if(project!==ownerProject||file!==ownerFile||epoch!==agent.identityEpoch)return;original=text;hash=data.sha256;status.textContent='已保存';}catch(e){if(project===ownerProject&&file===ownerFile&&epoch===agent.identityEpoch)status.textContent=String(e);}finally{saving=false;updateSave();}};
  const newFile=iconButton('新建文件','M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8ZM14 2v6h6M12 12v6M9 15h6'),newDirectory=iconButton('新建目录','M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2ZM12 10v7M8.5 13.5h7');bar.insertBefore(newFile,refresh);bar.insertBefore(newDirectory,refresh);newFile.type=newDirectory.type='button';
  async function create(kind){
   if(creating||saving||!project)return;const version=seq,epoch=agent.identityEpoch,selected=project,parent=path;creating=true;newFile.disabled=newDirectory.disabled=true;
   try{const name=await window.uiPrompt(kind==='file'?'请输入文件名称':'请输入目录名称','',kind==='file'?'新建文件':'新建目录');if(name===null||name===undefined||version!==seq||epoch!==agent.identityEpoch)return;
    if(!name||name.trim()!==name||/[\\/:\x00-\x1f]/.test(name)||name==='.'||name==='..'){status.textContent='请输入有效名称，不能包含路径分隔符或首尾空格。';return;}
    if(!await discard()||version!==seq||epoch!==agent.identityEpoch)return;
    await invoke('agent_project_entry_create',{projectId:selected.id,parent,name,kind});if(version!==seq||epoch!==agent.identityEpoch)return;
    creating=false;clearPreview();await loadDirectory(parent);if(seq===version+1&&project===selected&&epoch===agent.identityEpoch&&!panel.classList.contains('hidden')){if(kind==='file')await loadFile(parent==='.'?name:parent+'/'+name);status.textContent=kind==='file'?'文件已创建':'目录已创建';}
   }catch(e){if(version===seq&&epoch===agent.identityEpoch)status.textContent=String(e);}finally{creating=false;newFile.disabled=newDirectory.disabled=false;}
  }
  newFile.onclick=()=>create('file');newDirectory.onclick=()=>create('directory');
  editor.commands.addCommand({name:'saveProjectFile',bindKey:{win:'Ctrl-S',mac:'Command-S'},exec:()=>save.click()});

  function closeMenu(focus=false){popup?.remove();popup=null;if(trigger){trigger.setAttribute('aria-expanded','false');if(focus)trigger.focus();}trigger=null;}
  function hide(){panel.classList.add('hidden');seq++;closeMenu();}
  function reset(){hide();project=null;file=null;list.replaceChildren();original='';editor.setValue('',-1);editor.setReadOnly(true);editable=false;updateSave();location.textContent='';location.title='';}
  function clearPreview(){file=null;original='';hash='';editable=false;editor.setValue('',-1);editor.setReadOnly(true);updateSave();code.setAttribute('aria-label','文件编辑器');}
  async function loadDirectory(target){
   const before=seq,identity=agent.identityEpoch;if(saving||creating||!await discard()||before!==seq||identity!==agent.identityEpoch)return;
   const version=++seq,epoch=agent.identityEpoch,selected=project;path=target;clearPreview();status.textContent='正在读取目录…';list.replaceChildren();up.disabled=target==='.';location.textContent=target==='.'?'根目录':'根目录 / '+target;location.title=selected.directory+(target==='.'?'':(selected.directory.endsWith('/')?'':'/')+target);
   try{const data=await invoke('agent_project_browse',{projectId:selected.id,path:target,action:'list'});if(version!==seq||epoch!==agent.identityEpoch)return;
    status.textContent=data.truncated?`目录条目较多，仅显示前 ${data.entries.length} 项。`:`${data.entries.length} 项 · 隐藏敏感文件、链接及构建缓存`;
    for(const entry of data.entries){const row=node('button','project-browser-entry');row.type='button';row.append(node('span','',entry.type==='directory'?'▱':'▤'),node('span','project-browser-name',entry.name),node('small','',entry.type==='directory'?'目录':entry.size+' B'));const child=target==='.'?entry.name:target+'/'+entry.name;row.dataset.path=child;row.onclick=()=>{if(entry.type==='directory')loadDirectory(child);else loadFile(child);};list.append(row);}
    if(!data.entries.length)list.append(node('p','','此目录为空。'));
   }catch(e){if(version===seq)status.textContent=String(e);}
  }
  async function loadFile(target){
   const before=seq,identity=agent.identityEpoch;if(saving||creating||!await discard()||before!==seq||identity!==agent.identityEpoch)return;
   const version=++seq,epoch=agent.identityEpoch;clearPreview();code.setAttribute('aria-label',target);status.textContent='正在读取…';
   try{const data=await invoke('agent_project_browse',{projectId:project.id,path:target,action:'edit'});if(version!==seq||epoch!==agent.identityEpoch)return;
    file=target;list.querySelectorAll('button').forEach(b=>b.classList.toggle('active',b.dataset.path===target));original=data.content;hash=data.sha256;editable=data.editable;editor.setValue(original,-1);editor.setReadOnly(!editable);
    const ext=target.split('.').pop().toLowerCase(),modes={js:'javascript',ts:'typescript',json:'json',html:'html',htm:'html',css:'css',py:'python',java:'java',rs:'rust',c:'c_cpp',cpp:'c_cpp',h:'c_cpp',sh:'sh',yaml:'yaml',yml:'yaml',xml:'xml',md:'markdown',sql:'sql',ini:'ini'};editor.session.setMode('ace/mode/'+(modes[ext]||'text'));editor.resize();
    status.textContent=editable?'':'文件超过128 KiB，只读预览';updateSave();
   }catch(e){if(version===seq)status.textContent='不可预览：'+String(e);}
  }
  up.onclick=()=>loadDirectory(path.includes('/')?path.slice(0,path.lastIndexOf('/')):'.');refresh.onclick=()=>guidanceMode?loadGuidance():loadDirectory(path);
  async function open(value){guidanceMode=false;up.hidden=newFile.hidden=newDirectory.hidden=false;addSkill.hidden=true;closeMenu();const before=seq,identity=agent.identityEpoch;const changed=project?.id!==value.id;if(changed&&(saving||creating||!await discard()||before!==seq||identity!==agent.identityEpoch))return;const resume=!changed&&dirty();if(changed)clearPreview();project=value;for(const id of ['agent-main','keeper-account','keeper-settings'])document.getElementById(id).classList.add('hidden');panel.classList.remove('hidden');title.textContent=value.name+' · 项目目录';if(resume)editor.resize();else await loadDirectory('.');}
  async function openFile(value,target){const epoch=agent.identityEpoch;await open(value);if(epoch!==agent.identityEpoch||project?.id!==value.id||panel.classList.contains('hidden'))return;await loadFile(target);}
  const addSkill=iconButton('添加技能','M12 5v14M5 12h14');addSkill.type='button';addSkill.hidden=true;bar.insertBefore(addSkill,refresh);
  function validGuidance(version,epoch){return guidanceMode&&version===seq&&epoch===agent.identityEpoch&&guidanceSession===agent.selected;}
  async function loadGuidance(){
   if(saving||creating||!await discard())return;
   const version=++seq,epoch=agent.identityEpoch;clearPreview();list.replaceChildren();status.textContent='正在读取项目指南…';
   try{const data=await invoke('agent_project_guidance',{sessionId:guidanceSession});if(!validGuidance(version,epoch))return;
    const row=(label,target,missing=false)=>{const b=node('button','project-browser-entry',label);b.type='button';b.dataset.path=target;b.onclick=async()=>{if(missing){try{await invoke('agent_project_entry_create',{projectId:project.id,parent:'.',name:'AGENTS.md',kind:'file'});missing=false;}catch(e){status.textContent=String(e);return;}}await loadFile(target);};list.append(b);return b;};
    row('项目指南 · AGENTS.md','AGENTS.md',data.rules?.status==='missing');list.append(node('h3','guidance-list-heading','技能列表'));
    for(const skill of data.skills||[]){const b=row(skill.name,skill.path);b.title=skill.description||skill.path;}
    if(!data.skills?.length)list.append(node('p','guidance-list-empty','暂无技能，点击“添加技能”创建。'));
    status.textContent=(data.warnings||[]).join(' · ')||(data.catalogTruncated?'技能目录超出扫描上限':'');
    if(data.rules?.status!=='missing')await loadFile('AGENTS.md');
   }catch(e){if(validGuidance(version,epoch))status.textContent=String(e);}
  }
  async function openGuidance(value){
   if(saving||creating||!await discard())return;
   await open(value);guidanceMode=true;guidanceSession=agent.selected;up.hidden=newFile.hidden=newDirectory.hidden=true;addSkill.hidden=false;
   title.textContent=value.name+' · 项目指南';location.textContent='项目指南与技能';location.title=value.directory;await loadGuidance();
  }
  addSkill.onclick=async()=>{
   if(creating||saving||!await discard())return;
   const version=seq,epoch=agent.identityEpoch,owner=project;
   const name=await window.uiPrompt('技能名称（小写字母、数字和连字符，最多64字符）','','添加技能');
   if(name==null||!validGuidance(version,epoch))return;
   if(!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(name)||name.length>64){status.textContent='请输入有效的技能名称。';return;}
   creating=true;addSkill.disabled=true;
   try{
    for(const [parent,leaf] of [['.','.agents'],['.agents','skills']]){
     try{await invoke('agent_project_browse',{projectId:owner.id,path:parent==='.'?leaf:parent+'/'+leaf,action:'list'});}
     catch{await invoke('agent_project_entry_create',{projectId:owner.id,parent,name:leaf,kind:'directory'});}
     if(!validGuidance(version,epoch))return;
    }
    await invoke('agent_project_entry_create',{projectId:owner.id,parent:'.agents/skills',name,kind:'directory'});
    if(!validGuidance(version,epoch))return;
    const target='.agents/skills/'+name+'/SKILL.md';await invoke('agent_project_entry_create',{projectId:owner.id,parent:'.agents/skills/'+name,name:'SKILL.md',kind:'file'});
    if(!validGuidance(version,epoch))return;
    const data=await invoke('agent_project_browse',{projectId:owner.id,path:target,action:'edit'});
    if(!validGuidance(version,epoch))return;
    await invoke('agent_project_file_save',{projectId:owner.id,path:target,sha256:data.sha256,content:'---\nname: '+name+'\ndescription: 请填写技能的用途和适用场景\n---\n\n# '+name+'\n\n请填写执行步骤。\n'});
    if(!validGuidance(version,epoch))return;
    creating=false;await loadGuidance();if(guidanceMode&&epoch===agent.identityEpoch&&project===owner)await loadFile(target);
   }catch(e){if(validGuidance(version,epoch))status.textContent='创建失败：'+String(e);}
   finally{creating=false;addSkill.disabled=false;}
  };
  function menu(button,value,manage){closeMenu();trigger=button;button.setAttribute('aria-expanded','true');popup=node('div','project-context-menu');popup.setAttribute('role','menu');
   for(const [label,action] of [['管理项目',manage],['项目目录',()=>open(value)]]){const item=node('button');const icon=document.createElementNS('http://www.w3.org/2000/svg','svg');for(const [key,val] of Object.entries({viewBox:'0 0 24 24',fill:'none',stroke:'currentColor','stroke-width':'1.7','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true'}))icon.setAttribute(key,val);const shape=document.createElementNS(icon.namespaceURI,'path');shape.setAttribute('d',label==='管理项目'?'m9 3-.6 2-2 .9-1.9-.5-2 3.4L4 10.5v3l-1.5 1.7 2 3.4 1.9-.5 2 .9.6 2h6l.6-2 2-.9 1.9.5 2-3.4-1.5-1.7v-3l1.5-1.7-2-3.4-1.9.5-2-.9-.6-2ZM15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0':'M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v2M3 7v12a2 2 0 0 0 2 2h14l3-11H8l-3 7');icon.append(shape);item.append(icon,node('span','',label));item.type='button';item.setAttribute('role','menuitem');item.onclick=()=>{closeMenu();action();};popup.append(item);}
   document.body.append(popup);const rect=button.getBoundingClientRect();popup.style.left=Math.max(8,Math.min(rect.right-popup.offsetWidth,innerWidth-popup.offsetWidth-8))+'px';popup.style.top=Math.max(8,Math.min(rect.bottom+4,innerHeight-popup.offsetHeight-8))+'px';popup.firstChild.focus();
   popup.onkeydown=e=>{const items=[...popup.children],i=items.indexOf(document.activeElement);if(e.key==='Escape'){e.preventDefault();e.stopPropagation();closeMenu(true);}else if(['ArrowDown','ArrowUp'].includes(e.key)){e.preventDefault();items[(i+(e.key==='ArrowDown'?1:items.length-1))%items.length].focus();}};
  }
  document.addEventListener('click',e=>{if(popup&&!popup.contains(e.target)&&!trigger?.contains(e.target))closeMenu();});window.addEventListener('resize',()=>closeMenu());
  return {menu,open,openFile,openGuidance,hide,reset};
 };
})();
