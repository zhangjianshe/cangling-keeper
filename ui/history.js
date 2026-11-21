/* Account-scoped conversation search and lifecycle controls. Message text stays in SQLite. */
(() => {
  'use strict';
  window.createKeeperHistory=({agent,invoke,element,notice,bootstrap,select,showConversation,renderProjects,renderSessions,renderConversation,clearSelected})=>{
    const $=s=>document.querySelector(s),empty=()=>({items:[],nextCursor:null,hasMore:false});
    let recent=empty(),pinned=empty(),projects=new Map(),opened=new Map(),cache=new Map(),revision=0,request=0,debounce=null,previousFocus=null,menu=null,queryBusy=false,mutationBusy=false;
    let result=empty(),state='active',lastOwner=null,previewDetail=null,previewSelection=null;
    const icons={search:'M21 21l-4.4-4.4M19 11a8 8 0 1 1-16 0 8 8 0 0 1 16 0',more:'M5 12h.01M12 12h.01M19 12h.01',archive:'M3 8h18M5 8v13h14V8M3 3h18v5H3zM10 12h4',restore:'M3 11a9 9 0 1 1 2.5 7M3 4v7h7',trash:'M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7',close:'M6 6l12 12M18 6 6 18'};
    function icon(kind){const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');for(const [k,v] of Object.entries({viewBox:'0 0 24 24',fill:'none',stroke:'currentColor','stroke-width':'1.7','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true'}))svg.setAttribute(k,v);const path=document.createElementNS(svg.namespaceURI,'path');path.setAttribute('d',icons[kind]);svg.append(path);return svg;}
    function button(label,kind,fn,cls=''){const node=element('button',cls);node.type='button';if(kind)node.append(icon(kind));node.append(element('span','',label));node.onclick=fn;return node;}
    const launch=button('搜索与管理会话','search',()=>open(),'history-launch');launch.id='agent-history-open';$('.agent-navigation-sections').before(launch);
    const modal=element('div','history-modal');modal.id='agent-history-modal';modal.hidden=true;
    const panel=element('section','history-panel');panel.setAttribute('role','dialog');panel.setAttribute('aria-modal','true');panel.setAttribute('aria-labelledby','history-title');
    const header=element('header','history-header'),title=element('h2','','会话管理');title.id='history-title';const closeButton=button('关闭','close',()=>close(),'history-close');closeButton.setAttribute('aria-label','关闭会话管理');header.append(title,closeButton);
    const controls=element('div','history-controls'),search=element('input');search.id='history-search';search.type='search';search.maxLength=128;search.placeholder='搜索标题和消息内容';search.setAttribute('aria-label','搜索所有会话的标题和消息内容');
    const projectFilter=element('select');projectFilter.id='history-project';projectFilter.setAttribute('aria-label','按项目筛选');controls.append(search,projectFilter);
    const tabs=element('div','history-tabs');tabs.setAttribute('role','group');tabs.setAttribute('aria-label','会话状态');
    for(const [value,label] of [['active','会话'],['archived','已归档'],['deleted','回收站']]){const tab=button(label,null,()=>{state=value;load();});tab.dataset.state=value;tab.setAttribute('aria-pressed',String(value===state));tabs.append(tab);}
    const status=element('p','history-status');status.id='history-status';status.setAttribute('role','status');const list=element('div','history-results');list.id='history-results';list.setAttribute('aria-label','会话搜索结果');
    const footer=element('footer','history-footer');footer.append(element('span','','仅当前账户 · 归档后可恢复 · 删除先进入回收站'));
    panel.append(header,controls,tabs,status,list,footer);modal.append(panel);document.body.append(modal);
    const readonly=element('div','history-readonly');readonly.hidden=true;readonly.setAttribute('role','status');$('#agent-notice').after(readonly);
    function sync(){const all=new Map(cache);for(const page of [recent,pinned,...projects.values()])for(const item of page.items){all.set(item.id,item);cache.set(item.id,item);}agent.sessions=[...all.values()];}
    function remember(item){if(item?.id){cache.set(item.id,item);sync();}}
    function pageButton(page,fetcher,host){if(!page?.hasMore)return;const b=button(page.loading?'加载中…':'加载更多',null,fetcher,'history-load-more');b.disabled=!!page.loading;host.append(b);}
    function closeMenu(focus=false){if(!menu)return;const previous=menu.launch;menu.node.remove();menu=null;if(focus)previous.focus();}
    function decorateRow(row,item){
      const action=button('会话操作','more',event=>{event.stopPropagation();if(menu?.launch===action){closeMenu();return;}closeMenu();const node=element('div','history-row-menu');node.setAttribute('role','menu');node.setAttribute('aria-label',item.title+'的操作');
        for(const [name,kind,command] of [['归档','archive','archive'],['移到回收站','trash','delete']]){const b=button(name,kind,()=>{closeMenu();mutate(item,command);});b.setAttribute('role','menuitem');node.append(b);}document.body.append(node);const r=action.getBoundingClientRect();node.style.left=Math.max(8,Math.min(innerWidth-190,r.left))+'px';node.style.top=Math.max(8,Math.min(innerHeight-100,r.bottom+4))+'px';menu={node,launch:action};node.querySelector('button').focus();
      },'history-row-more');action.title='会话操作';action.setAttribute('aria-label','会话操作：'+item.title);action.setAttribute('aria-haspopup','menu');row.append(action);
    }
    async function morePage(page,args,redraw){if(!page?.hasMore || page.loading)return;const rev=revision,epoch=agent.identityEpoch || 0;page.loading=true;redraw();try{const data=await invoke('agent_sessions',{state:'active',limit:30,...args,cursor:page.nextCursor});if(rev!==revision || epoch!==(agent.identityEpoch || 0))return;page.items=[...new Map([...page.items,...data.items].map(i=>[i.id,i])).values()];page.nextCursor=data.nextCursor;page.hasMore=data.hasMore;sync();}catch(error){if(rev===revision)notice(String(error));}finally{page.loading=false;if(rev===revision)redraw();}}
    function appendRecent(host){pageButton(recent,()=>morePage(recent,{},()=>{renderSessions();renderProjects();}),host);}
    function appendPinned(host){pageButton(pinned,()=>morePage(pinned,{pinned:true},renderSessions),host);}
    function attachProject(group,id){
      group.addEventListener('toggle',()=>{opened.set(id,group.open);if(group.open && !projects.has(id))loadProject(id);});
      const page=projects.get(id);
      if(!page){if(group.open)queueMicrotask(()=>loadProject(id));return;}
      if(!page.items.length)group.append(element('div','agent-nav-empty',page.loading?'加载中…':page.error?'加载失败':'暂无会话'));
      if(page.error){group.append(button('重试',null,()=>{projects.delete(id);loadProject(id);},'history-load-more'));return;}
      pageButton(page,()=>morePage(page,{projectId:id},renderProjects),group);
    }
    async function loadProject(id){if(projects.has(id))return;const rev=revision,epoch=agent.identityEpoch || 0,page={...empty(),loading:true};projects.set(id,page);renderProjects();try{const data=await invoke('agent_sessions',{state:'active',projectId:id,limit:30});if(rev!==revision || epoch!==(agent.identityEpoch || 0))return;Object.assign(page,data);sync();}catch(error){page.error=String(error);}finally{page.loading=false;if(rev===revision)renderProjects();}}
    function acceptBootstrap(data){
      if(lastOwner!==null && lastOwner!==data.accountKey)reset();lastOwner=data.accountKey;revision++;closeMenu();
      const selected=cache.get(agent.selected);cache=new Map(selected?[[selected.id,selected]]:[]);projects.clear();recent={items:data.sessions || [],...(data.sessionPage || {}),loading:false};pinned={items:data.pinnedSessions || recent.items.filter(s=>s.pinned),...(data.pinnedPage || {}),loading:false};sync();
      const selectedProject=projectFilter.value;projectFilter.replaceChildren(new Option('全部项目',''));for(const p of agent.projects)projectFilter.append(new Option(p.name,p.id));projectFilter.value=selectedProject;
    }
    function reset(){revision++;request++;clearTimeout(debounce);close();closeMenu();recent=empty();pinned=empty();projects.clear();cache.clear();opened.clear();clearResults();search.value='';projectFilter.replaceChildren(new Option('全部项目',''));state='active';lastOwner=null;readonly.hidden=true;$('#agent-main').classList.remove('history-readonly-view');}
    function projectOpen(id){return opened.has(id)?opened.get(id):id===agent.project;}
    function clearResults(){result=empty();list.replaceChildren();}
    async function load(append=false){
      clearTimeout(debounce);if(append && (queryBusy || !result.hasMore))return;
      const token=++request,epoch=agent.identityEpoch || 0;queryBusy=true;status.textContent='正在查找…';list.setAttribute('aria-busy','true');if(!append)clearResults();
      for(const t of tabs.querySelectorAll('button'))t.setAttribute('aria-pressed',String(t.dataset.state===state));
      try{const data=await invoke('agent_sessions',{state,q:search.value.trim(),projectId:projectFilter.value || undefined,limit:30,cursor:append?result.nextCursor:undefined});if(token!==request || epoch!==(agent.identityEpoch || 0))return;
        result={...data,items:append?[...result.items,...data.items]:data.items};renderResults();status.textContent=result.items.length?'已显示 '+result.items.length+' 个会话'+(result.hasMore?'，可继续加载':''):(search.value.trim()?'没有匹配的会话':({active:'暂无会话',archived:'暂无归档会话',deleted:'回收站为空'})[state]);
      }catch(error){if(token===request)status.textContent='加载失败：'+String(error);}finally{if(token===request){queryBusy=false;list.setAttribute('aria-busy','false');}}
    }
    function renderResults(){
      list.replaceChildren();for(const item of result.items){
        const row=element('article','history-result');row.dataset.sessionId=item.id;
        const info=button(item.title,null,()=>openResult(item),'history-result-open');info.disabled=state==='deleted';info.title=state==='deleted'?'恢复后可打开会话':'打开会话';
        const project=agent.projects.find(p=>p.id===item.projectId)?.name || '项目';info.append(element('small','',project+' · '+new Date(item.updatedAt).toLocaleString()));
        if(item.match){const snippet=element('p','history-snippet',item.match.snippet || '');info.append(snippet);if(item.match.messageId)info.append(element('small','history-match-label','消息内容匹配 · 点击定位'));}
        const actions=element('div','history-result-actions');
        const choices=state==='deleted'?[['恢复','restore','restore'],['永久删除','trash','purge']]:state==='archived'?[['取消归档','restore','unarchive'],['删除','trash','delete']]:[['归档','archive','archive'],['删除','trash','delete']];
        for(const [label,kind,action] of choices){const b=button(label,kind,()=>mutate(item,action));b.disabled=mutationBusy;if(action==='purge')b.classList.add('danger');actions.append(b);}row.append(info,actions);list.append(row);
      }
      if(result.hasMore)list.append(button('加载更多',null,()=>load(true),'history-load-more'));
    }
    async function openResult(item){
      if(item.deletedAt)return;const epoch=agent.identityEpoch || 0;close();remember(item);agent.project=item.projectId;opened.set(item.projectId,true);showConversation();await select(item.id);if(epoch!==(agent.identityEpoch || 0) || agent.selected!==item.id || agent.detail.sessionId!==item.id)return;
      renderProjects();const match=item.match;
      if(match?.messageId && !agent.detail.modelPending){const group=selectReadingVersion(match.messageId);renderConversation();window.keeperThreadNav?.jump(group?.rootId || match.rootId || match.messageId);}
    }
    // Resolve the matching prompt and response together, as the persisted reply_groups does.
    function selectReadingVersion(messageId){
      const messages=agent.detail.messages || [],groups=agent.detail.replyGroups || [],entry=messages.find(m=>m.id===messageId);
      const ids=g=>g.allVersionIds || g.versionIds,groupFor=id=>groups.find(g=>ids(g).includes(id)),target=groupFor(messageId);
      if(!entry || !target)return null;
      if(previewDetail!==agent.detail){previewDetail=agent.detail;previewSelection=JSON.stringify(groups.map(g=>[g.rootId,g.selectedId]));}
      target.selectedId=messageId;
      if(entry.role==='assistant'){
        const root=messages.findIndex(m=>m.id===target.rootId),original=messages.slice(0,root).reverse().find(m=>m.role==='user');
        const prompt=entry.modelMetadata?.promptMessageId || original?.id,userGroup=groupFor(prompt);if(userGroup)userGroup.selectedId=prompt;
      }
      for(const group of groups){
        const root=messages.findIndex(m=>m.id===group.rootId && m.role==='assistant');if(root<0)continue;
        const original=messages.slice(0,root).reverse().find(m=>m.role==='user'),user=original && groupFor(original.id);if(!user)continue;
        const all=[...ids(group)],userIds=ids(user),chosen=user.selectedId;
        const eligible=all.filter(id=>{const reply=messages.find(m=>m.id===id),prompt=reply?.modelMetadata?.promptMessageId || original.id;return prompt===chosen || (!userIds.includes(prompt) && chosen===original.id);});
        group.allVersionIds=all;group.versionIds=eligible;if(!eligible.includes(group.selectedId))group.selectedId=eligible.at(-1) || null;
      }
      agent.detail.historyPreview=previewSelection!==JSON.stringify(groups.map(g=>[g.rootId,g.selectedId]));
      return target;
    }

    async function mutate(item,action){
      if(mutationBusy)return;const epoch=agent.identityEpoch || 0;mutationBusy=true;renderResults();
      try{
        if(action==='delete' && !await uiConfirm('将会话“'+item.title+'”移到回收站？之后可以恢复。项目目录中的文件会保留。','删除会话'))return;
        if(action==='purge' && !await uiConfirm('永久删除会话“'+item.title+'”及其消息、版本和工具记录？此操作无法撤销，项目目录中的文件会保留。','永久删除会话'))return;
        if(epoch!==(agent.identityEpoch || 0))return;
        const commands={archive:'agent_archive_session',unarchive:'agent_archive_session',delete:'agent_delete_session',restore:'agent_restore_session',purge:'agent_purge_session'};
        await invoke(commands[action],{sessionId:item.id,archived:action==='archive',confirm:action==='purge'});
        if(epoch!==(agent.identityEpoch || 0))return;
        if(agent.selected===item.id && ['delete','purge'].includes(action))clearSelected();
        await bootstrap();if(epoch!==(agent.identityEpoch || 0))return;
        if(agent.selected===item.id)await select(item.id);if(!modal.hidden)await load();notice();
      }catch(error){if(epoch===(agent.identityEpoch || 0)){if(!modal.hidden)status.textContent=String(error);else notice(String(error));}}
      finally{mutationBusy=false;if(!modal.hidden)renderResults();}
    }
    function updateDetail(){
      if(previewDetail!==agent.detail){previewDetail=null;previewSelection=null;}
      const item=agent.detail.session;remember(item);const inactive=!!(item?.archivedAt || item?.deletedAt),preview=!!agent.detail.historyPreview;$('#agent-main').classList.toggle('history-readonly-view',inactive || preview);readonly.hidden=!(inactive || preview);readonly.replaceChildren();
      if(inactive){readonly.append(element('span','',item.deletedAt?'此会话位于回收站，仅供查看。':'此会话已归档，仅供查看。'),button(item.deletedAt?'恢复会话':'取消归档', 'restore',()=>mutate(item,item.deletedAt?'restore':'unarchive')));}
      if(!inactive && preview)readonly.append(element('span','','正在预览历史版本，返回当前版本后可继续对话。'),button('返回当前版本','restore',()=>select(agent.selected)));
      $('#agent-session-title').disabled=inactive || preview || !agent.selected;$('#agent-input').disabled=inactive || preview;
    }
    function open(){previousFocus=document.activeElement;modal.hidden=false;load();search.focus();}
    function close(){request++;clearTimeout(debounce);modal.hidden=true;queryBusy=false;if(previousFocus?.isConnected)previousFocus.focus();previousFocus=null;}
    search.oninput=()=>{clearTimeout(debounce);request++;clearResults();status.textContent='正在查找…';debounce=setTimeout(()=>load(),250);};projectFilter.onchange=()=>load();
    modal.addEventListener('click',event=>{if(event.target===modal)close();});
    modal.addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();close();}else if(event.key==='Tab'){const nodes=[...panel.querySelectorAll('button:not(:disabled),input,select')].filter(n=>!n.hidden),first=nodes[0],last=nodes.at(-1);if(event.shiftKey && document.activeElement===first){event.preventDefault();last.focus();}else if(!event.shiftKey && document.activeElement===last){event.preventDefault();first.focus();}}});
    document.addEventListener('click',event=>{if(menu && !menu.node.contains(event.target) && event.target!==menu.launch)closeMenu();});document.addEventListener('keydown',event=>{if(!menu)return;const buttons=[...menu.node.querySelectorAll('button')],i=buttons.indexOf(document.activeElement);if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeMenu(true);}else if(['ArrowDown','ArrowUp'].includes(event.key)){event.preventDefault();buttons[(i+(event.key==='ArrowDown'?1:-1)+buttons.length)%buttons.length].focus();}});window.addEventListener('resize',()=>closeMenu());$('.agent-navigation-sections').addEventListener('scroll',()=>closeMenu(),{passive:true});
    return {acceptBootstrap,reset,remember,recent:()=>recent.items,pinned:()=>pinned.items,projectSessions:id=>projects.get(id)?.items || [],projectOpen,attachProject,appendRecent,appendPinned,decorateRow,updateDetail,selectReadingVersion,open};
  };
})();
