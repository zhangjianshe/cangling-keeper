(() => {
 window.createKeeperProjectBrowser=({invoke,agent,showConversation})=>{
  const node=(tag,cls,text)=>{const el=document.createElement(tag);el.className=cls||'';if(text!==undefined)el.textContent=text;return el;};
  const panel=node('section','keeper-project-browser hidden');panel.setAttribute('aria-label','项目目录');
  const header=node('header','keeper-settings-header'),back=node('button','agent-nav-button','← 返回会话'),title=node('h1','','项目目录');back.type='button';back.onclick=showConversation;header.append(back,title);
  const location=node('p','project-browser-location'),bar=node('div','project-browser-bar'),up=node('button','btn','上级目录'),refresh=node('button','btn','刷新'),status=node('p','project-browser-status');status.setAttribute('role','status');bar.append(up,refresh);
  const layout=node('div','project-browser-layout'),list=node('nav','project-browser-list'),preview=node('section','project-browser-preview'),filename=node('h2','','文件预览'),hint=node('p','','选择一个文本文件查看内容。'),code=node('pre'),pages=node('div','project-browser-pages'),previous=node('button','btn','上一页'),next=node('button','btn','下一页');list.setAttribute('aria-label','目录内容');pages.append(previous,next);preview.append(filename,hint,code,pages);layout.append(list,preview);panel.append(header,location,bar,status,layout);document.querySelector('#agent-main').after(panel);
  let project=null,path='.',file=null,start=1,seq=0,popup=null,trigger=null;
  function closeMenu(focus=false){popup?.remove();popup=null;if(trigger){trigger.setAttribute('aria-expanded','false');if(focus)trigger.focus();}trigger=null;}
  function hide(){panel.classList.add('hidden');seq++;closeMenu();}
  function reset(){hide();project=null;file=null;list.replaceChildren();code.textContent='';location.textContent='';}
  function clearPreview(){file=null;start=1;filename.textContent='文件预览';hint.textContent='支持 UTF-8 文本（最多 1 MiB），不执行 HTML 或脚本。';code.textContent='';pages.hidden=true;}
  async function loadDirectory(target){
   const version=++seq,epoch=agent.identityEpoch,selected=project;path=target;clearPreview();status.textContent='正在读取目录…';list.replaceChildren();up.disabled=target==='.';location.textContent=selected.directory+(target==='.'?'':' / '+target);
   try{const data=await invoke('agent_project_browse',{projectId:selected.id,path:target,action:'list'});if(version!==seq||epoch!==agent.identityEpoch)return;
    status.textContent=data.truncated?'目录条目较多，仅显示前 200 项。':`${data.entries.length} 项 · 隐藏敏感文件、链接及构建缓存`;
    for(const entry of data.entries){const row=node('button','project-browser-entry');row.type='button';row.append(node('span','',entry.type==='directory'?'▱':'▤'),node('span','project-browser-name',entry.name),node('small','',entry.type==='directory'?'目录':entry.size+' B'));const child=target==='.'?entry.name:target+'/'+entry.name;row.onclick=()=>{if(entry.type==='directory')loadDirectory(child);else{list.querySelectorAll('button').forEach(b=>b.classList.toggle('active',b===row));loadFile(child,1);}};list.append(row);}
    if(!data.entries.length)list.append(node('p','','此目录为空。'));
   }catch(e){if(version===seq)status.textContent=String(e);}
  }
  async function loadFile(target,line){
   const version=++seq,epoch=agent.identityEpoch;file=target;start=line;filename.textContent=target;code.textContent='';hint.textContent='正在读取…';pages.hidden=true;
   try{const data=await invoke('agent_project_browse',{projectId:project.id,path:target,action:'read',startLine:line});if(version!==seq||epoch!==agent.identityEpoch)return;
    code.textContent=data.lines.map(l=>String(l.line).padStart(4)+'  '+l.text).join('\n');hint.textContent=`共 ${data.totalLines} 行`+(data.truncated?' · 当前预览有省略，可继续翻页；超长单行仅显示前段。':'');
    const last=data.lines.at(-1)?.line||line;previous.disabled=line<=1;next.disabled=last>=data.totalLines;next.dataset.line=String(last+1);pages.hidden=data.totalLines===0;
   }catch(e){if(version===seq)hint.textContent=String(e);}
  }
  previous.onclick=()=>loadFile(file,Math.max(1,start-200));next.onclick=()=>loadFile(file,Number(next.dataset.line));up.onclick=()=>loadDirectory(path.includes('/')?path.slice(0,path.lastIndexOf('/')):'.');refresh.onclick=()=>loadDirectory(path);
  function open(value){closeMenu();project=value;for(const id of ['agent-main','keeper-account','keeper-settings'])document.getElementById(id).classList.add('hidden');panel.classList.remove('hidden');title.textContent=value.name+' · 项目目录';loadDirectory('.');}
  function menu(button,value,manage){closeMenu();trigger=button;button.setAttribute('aria-expanded','true');popup=node('div','project-context-menu');popup.setAttribute('role','menu');
   for(const [label,action] of [['管理项目',manage],['项目目录',()=>open(value)]]){const item=node('button','',label);item.type='button';item.setAttribute('role','menuitem');item.onclick=()=>{closeMenu();action();};popup.append(item);}
   document.body.append(popup);const rect=button.getBoundingClientRect();popup.style.left=Math.max(8,Math.min(rect.right-popup.offsetWidth,innerWidth-popup.offsetWidth-8))+'px';popup.style.top=Math.max(8,Math.min(rect.bottom+4,innerHeight-popup.offsetHeight-8))+'px';popup.firstChild.focus();
   popup.onkeydown=e=>{const items=[...popup.children],i=items.indexOf(document.activeElement);if(e.key==='Escape'){e.preventDefault();e.stopPropagation();closeMenu(true);}else if(['ArrowDown','ArrowUp'].includes(e.key)){e.preventDefault();items[(i+(e.key==='ArrowDown'?1:items.length-1))%items.length].focus();}};
  }
  document.addEventListener('click',e=>{if(popup&&!popup.contains(e.target)&&!trigger?.contains(e.target))closeMenu();});window.addEventListener('resize',()=>closeMenu());
  return {menu,open,hide,reset};
 };
})();
