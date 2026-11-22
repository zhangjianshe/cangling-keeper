(() => {
 window.createKeeperProjectBrowser=({invoke,agent,showConversation})=>{
  const node=(tag,cls,text)=>{const el=document.createElement(tag);el.className=cls||'';if(text!==undefined)el.textContent=text;return el;};
  const panel=node('section','keeper-project-browser hidden');panel.setAttribute('aria-label','项目目录');
  const header=node('header','keeper-settings-header'),back=node('button','agent-nav-button','← 返回会话'),title=node('h1','','项目目录');back.type='button';back.onclick=showConversation;header.append(back,title);
  const location=node('p','project-browser-location'),bar=node('div','project-browser-bar'),up=node('button','btn','上级目录'),refresh=node('button','btn','刷新'),status=node('p','project-browser-status');status.setAttribute('role','status');bar.append(up,refresh);
  const layout=node('div','project-browser-layout'),list=node('nav','project-browser-list'),preview=node('section','project-browser-preview'),filename=node('h2','','文件预览'),hint=node('p','','选择一个文本文件查看内容。'),code=node('div','project-code-editor');list.setAttribute('aria-label','目录内容');preview.append(filename,hint,code);layout.append(list,preview);panel.append(header,location,bar,status,layout);document.querySelector('#agent-main').after(panel);
  const save=node('button','btn primary','保存文件');bar.append(save);save.disabled=true;
  const editor=window.ace.edit(code);window.ace.config.set('basePath','/vendor/ace');editor.setTheme('ace/theme/github');editor.session.setUseWorker(false);editor.setOptions({fontSize:13,showPrintMargin:false});editor.setReadOnly(true);
  let project=null,path='.',file=null,seq=0,popup=null,trigger=null,original='',hash='',editable=false,saving=false;
  const dirty=()=>!!file&&editor.getValue()!==original;
  function updateSave(){save.disabled=!editable||!dirty()||saving;save.textContent=saving?'正在保存…':dirty()?'保存文件 *':'保存文件';}
  editor.session.on('change',updateSave);
  const discard=async()=>!dirty()||await window.uiConfirm('当前文件有未保存的修改，放弃修改并继续？','未保存的修改');
  window.addEventListener('beforeunload',e=>{if(dirty()){e.preventDefault();e.returnValue='';}});
  save.onclick=async()=>{if(save.disabled)return;const ownerProject=project,ownerFile=file,epoch=agent.identityEpoch,text=editor.getValue();saving=true;updateSave();try{const data=await invoke('agent_project_file_save',{projectId:project.id,path:file,content:text,sha256:hash});if(project!==ownerProject||file!==ownerFile||epoch!==agent.identityEpoch)return;original=text;hash=data.sha256;hint.textContent='已保存';}catch(e){if(project===ownerProject&&file===ownerFile&&epoch===agent.identityEpoch)hint.textContent=String(e);}finally{saving=false;updateSave();}};
  editor.commands.addCommand({name:'saveProjectFile',bindKey:{win:'Ctrl-S',mac:'Command-S'},exec:()=>save.click()});

  function closeMenu(focus=false){popup?.remove();popup=null;if(trigger){trigger.setAttribute('aria-expanded','false');if(focus)trigger.focus();}trigger=null;}
  function hide(){panel.classList.add('hidden');seq++;closeMenu();}
  function reset(){hide();project=null;file=null;list.replaceChildren();original='';editor.setValue('',-1);editor.setReadOnly(true);editable=false;updateSave();location.textContent='';}
  function clearPreview(){file=null;original='';hash='';editable=false;editor.setValue('',-1);editor.setReadOnly(true);updateSave();filename.textContent='文件预览';hint.textContent='支持 UTF-8 文本；128 KiB 内可编辑，1 MiB 内可只读预览；二进制不可预览。';}
  async function loadDirectory(target){
   const before=seq,identity=agent.identityEpoch;if(saving||!await discard()||before!==seq||identity!==agent.identityEpoch)return;
   const version=++seq,epoch=agent.identityEpoch,selected=project;path=target;clearPreview();status.textContent='正在读取目录…';list.replaceChildren();up.disabled=target==='.';location.textContent=selected.directory+(target==='.'?'':' / '+target);
   try{const data=await invoke('agent_project_browse',{projectId:selected.id,path:target,action:'list'});if(version!==seq||epoch!==agent.identityEpoch)return;
    status.textContent=data.truncated?'目录条目较多，仅显示前 200 项。':`${data.entries.length} 项 · 隐藏敏感文件、链接及构建缓存`;
    for(const entry of data.entries){const row=node('button','project-browser-entry');row.type='button';row.append(node('span','',entry.type==='directory'?'▱':'▤'),node('span','project-browser-name',entry.name),node('small','',entry.type==='directory'?'目录':entry.size+' B'));const child=target==='.'?entry.name:target+'/'+entry.name;row.dataset.path=child;row.onclick=()=>{if(entry.type==='directory')loadDirectory(child);else loadFile(child);};list.append(row);}
    if(!data.entries.length)list.append(node('p','','此目录为空。'));
   }catch(e){if(version===seq)status.textContent=String(e);}
  }
  async function loadFile(target){
   const before=seq,identity=agent.identityEpoch;if(saving||!await discard()||before!==seq||identity!==agent.identityEpoch)return;
   const version=++seq,epoch=agent.identityEpoch;clearPreview();filename.textContent=target;hint.textContent='正在读取…';
   try{const data=await invoke('agent_project_browse',{projectId:project.id,path:target,action:'edit'});if(version!==seq||epoch!==agent.identityEpoch)return;
    file=target;list.querySelectorAll('button').forEach(b=>b.classList.toggle('active',b.dataset.path===target));original=data.content;hash=data.sha256;editable=data.editable;editor.setValue(original,-1);editor.setReadOnly(!editable);
    const ext=target.split('.').pop().toLowerCase(),modes={js:'javascript',ts:'typescript',json:'json',html:'html',htm:'html',css:'css',py:'python',java:'java',rs:'rust',c:'c_cpp',cpp:'c_cpp',h:'c_cpp',sh:'sh',yaml:'yaml',yml:'yaml',xml:'xml',md:'markdown',sql:'sql',ini:'ini'};editor.session.setMode('ace/mode/'+(modes[ext]||'text'));editor.resize();
    hint.textContent=editable?'可编辑 · Ctrl/Cmd+S 保存':'文件超过128 KiB，只读预览';updateSave();
   }catch(e){if(version===seq)hint.textContent='不可预览：'+String(e);}
  }
  up.onclick=()=>loadDirectory(path.includes('/')?path.slice(0,path.lastIndexOf('/')):'.');refresh.onclick=()=>loadDirectory(path);
  async function open(value){closeMenu();const before=seq,identity=agent.identityEpoch;const changed=project?.id!==value.id;if(changed&&(saving||!await discard()||before!==seq||identity!==agent.identityEpoch))return;const resume=!changed&&dirty();if(changed)clearPreview();project=value;for(const id of ['agent-main','keeper-account','keeper-settings'])document.getElementById(id).classList.add('hidden');panel.classList.remove('hidden');title.textContent=value.name+' · 项目目录';if(resume)editor.resize();else loadDirectory('.');}
  function menu(button,value,manage){closeMenu();trigger=button;button.setAttribute('aria-expanded','true');popup=node('div','project-context-menu');popup.setAttribute('role','menu');
   for(const [label,action] of [['管理项目',manage],['项目目录',()=>open(value)]]){const item=node('button');const icon=document.createElementNS('http://www.w3.org/2000/svg','svg');for(const [key,val] of Object.entries({viewBox:'0 0 24 24',fill:'none',stroke:'currentColor','stroke-width':'1.7','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true'}))icon.setAttribute(key,val);const shape=document.createElementNS(icon.namespaceURI,'path');shape.setAttribute('d',label==='管理项目'?'m9 3-.6 2-2 .9-1.9-.5-2 3.4L4 10.5v3l-1.5 1.7 2 3.4 1.9-.5 2 .9.6 2h6l.6-2 2-.9 1.9.5 2-3.4-1.5-1.7v-3l1.5-1.7-2-3.4-1.9.5-2-.9-.6-2ZM15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0':'M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v2M3 7v12a2 2 0 0 0 2 2h14l3-11H8l-3 7');icon.append(shape);item.append(icon,node('span','',label));item.type='button';item.setAttribute('role','menuitem');item.onclick=()=>{closeMenu();action();};popup.append(item);}
   document.body.append(popup);const rect=button.getBoundingClientRect();popup.style.left=Math.max(8,Math.min(rect.right-popup.offsetWidth,innerWidth-popup.offsetWidth-8))+'px';popup.style.top=Math.max(8,Math.min(rect.bottom+4,innerHeight-popup.offsetHeight-8))+'px';popup.firstChild.focus();
   popup.onkeydown=e=>{const items=[...popup.children],i=items.indexOf(document.activeElement);if(e.key==='Escape'){e.preventDefault();e.stopPropagation();closeMenu(true);}else if(['ArrowDown','ArrowUp'].includes(e.key)){e.preventDefault();items[(i+(e.key==='ArrowDown'?1:items.length-1))%items.length].focus();}};
  }
  document.addEventListener('click',e=>{if(popup&&!popup.contains(e.target)&&!trigger?.contains(e.target))closeMenu();});window.addEventListener('resize',()=>closeMenu());
  return {menu,open,hide,reset};
 };
})();
