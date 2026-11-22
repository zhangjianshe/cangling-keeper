(() => {
  'use strict';
  const $ = selector => document.querySelector(selector);
  const invoke = (name, args = {}) => window.__KEEPER__.core.invoke(name, args);
  const agent = {projects:[], project:localStorage.getItem('keeper-agent-project') || 'general', sessions:[], tools:[], hosts:[], selected:null, detail:{messages:[],runs:[]}, generation:0, timer:null, sending:false};
  agent.projectBrowser=window.createKeeperProjectBrowser?.({invoke,agent,showConversation});
  const projectDirectoryButton=document.createElement('button');projectDirectoryButton.type='button';projectDirectoryButton.className='guidance-trigger';projectDirectoryButton.textContent='项目目录';projectDirectoryButton.onclick=()=>{const project=agent.projects.find(p=>p.id===(agent.detail.session?.projectId || agent.project));if(project)agent.projectBrowser?.open(project);};document.querySelector('.agent-header-tools').prepend(projectDirectoryButton);
  window.keeperThreadNav=window.createKeeperThreadNav?.();
  const statusNames = {running:'正在执行',pending_approval:'等待确认',succeeded:'已完成',failed:'执行失败',rejected:'已取消',interrupted:'执行中断',stopped:'已停止',approved:'已确认'};
  function element(tag, cls, text) { const node=document.createElement(tag); if(cls)node.className=cls;if(text!==undefined)node.textContent=text;return node; }
  function relativeMessageTime(timestamp,now=Date.now()){
    const seconds=Math.max(0,Math.floor((now-timestamp)/1000));
    if(seconds<60)return '刚刚';
    for(const [unit,label] of [[31536000,'年'],[2592000,'个月'],[86400,'天'],[3600,'小时'],[60,'分钟']])if(seconds>=unit)return Math.floor(seconds/unit)+label+'前';
    return '刚刚';
  }
  function updateMessageTimes(){for(const node of document.querySelectorAll('.agent-message-time[data-time]')){const time=Number(node.dataset.time);if(Number.isFinite(time) && time>0)node.textContent=relativeMessageTime(time);}}
  function notice(message='') {$('#agent-notice').textContent=message;$('#agent-notice').hidden=!message;}
  function sessionStorageKey(){return 'keeper-agent-session:'+(agent.ownerKey || 'guest');}
  function titleFor(tool) {return ({list_directory:'列出项目目录',read_file:'读取项目文件',load_skill:'加载项目技能',search_files:'搜索项目文件',write_file:'写入项目文件',apply_patch:'应用文件补丁',run_command:'执行本机命令'})[tool] || agent.tools.find(item=>item.name===tool)?.title || tool;}
  function sessionRow(session){
    const row=element('div','agent-session-row');
    const button=element('button','agent-session'+(session.id===agent.selected?' active':''),session.title);button.type='button';
    button.title=(agent.projects.find(p=>p.id===session.projectId)?.name || '')+' · '+session.title;
    button.setAttribute('aria-current',String(session.id===agent.selected));
    button.onclick=()=>{agent.project=session.projectId;renderProjects();showConversation();select(session.id);};
    row.append(button);agent.history?.decorateRow(row,session);return row;
  }
  function renderSessions(){
    const list=$('#agent-sessions'),pinned=$('#agent-pinned');list.replaceChildren();pinned.replaceChildren();
    const recent=agent.history?.recent() || agent.sessions,pins=agent.history?.pinned() || agent.sessions.filter(s=>s.pinned);
    for(const session of recent)list.append(sessionRow(session));for(const session of pins)pinned.append(sessionRow(session));
    if(!recent.length)list.append(element('div','agent-nav-empty','暂无会话'));
    if(!pins.length)pinned.append(element('div','agent-nav-empty','暂无置顶'));
    agent.history?.appendRecent(list);agent.history?.appendPinned(pinned);
    if(agent.titleEditor && agent.titleEditor.session!==agent.selected)agent.titleEditor.cancel(false);
    $('#agent-session-title').disabled=!agent.selected || !!(agent.detail.session?.archivedAt || agent.detail.session?.deletedAt || agent.detail.historyPreview);
    $('#agent-session-title').textContent=agent.sessions.find(s=>s.id===agent.selected)?.title || '新会话';
  }
  $('#agent-session-title').onclick=()=>{
    if(!agent.selected || agent.titleEditor || agent.detail.session?.archivedAt || agent.detail.session?.deletedAt || agent.detail.historyPreview)return;
    const session=agent.selected,button=$('#agent-session-title'),form=element('form','agent-title-editor'),input=element('input');
    input.type='text';input.maxLength=256;input.value=agent.sessions.find(s=>s.id===session)?.title || '新会话';input.setAttribute('aria-label','会话标题，最多128个字符');
    const count=element('span','agent-title-count');const update=()=>{const chars=Array.from(input.value);if(chars.length>128)input.value=chars.slice(0,128).join('');count.textContent=Array.from(input.value).length+'/128';};input.oninput=event=>{if(!event.isComposing)update();};input.oncompositionend=update;update();
    let cancelled=false,saving=false;
    const cancel=(focus=true)=>{cancelled=true;form.remove();button.hidden=false;agent.titleEditor=null;if(focus)button.focus();};agent.titleEditor={session,cancel};
    const save=async()=>{if(cancelled || saving)return;const title=input.value.trim();if(!title){notice('标题不能为空。');input.focus();return;}saving=true;input.disabled=true;
      try{await invoke('agent_rename_session',{sessionId:session,title});cancel(agent.selected===session);await bootstrap();notice();}
      catch(error){saving=false;input.disabled=false;notice(String(error));if(!cancelled)input.focus();}
    };
    form.onsubmit=event=>{event.preventDefault();save();};input.onkeydown=event=>{if(event.isComposing)return;if(event.key==='Escape'){event.preventDefault();event.stopPropagation();cancel();}else if(event.key==='Enter'){event.preventDefault();save();}};
    input.onblur=()=>{if(!cancelled && agent.selected===session)save();};form.append(input,count);button.hidden=true;button.after(form);input.focus();input.select();
  };
  function renderProjects(){
    if(!agent.projects.some(p=>p.id===agent.project))agent.project='general';
    localStorage.setItem('keeper-agent-project',agent.project);
    const list=$('#agent-projects');list.replaceChildren();
    for(const project of agent.projects){
      const group=element('details','agent-project-group');group.open=agent.history?agent.history.projectOpen(project.id):project.id===agent.project;
      const summary=element('summary');summary.title=project.directory;
      const icon=document.createElementNS('http://www.w3.org/2000/svg','svg');icon.setAttribute('viewBox','0 0 24 24');icon.setAttribute('class','agent-project-icon');icon.setAttribute('aria-hidden','true');
      // Lucide folder-closed / folder-open (ISC), see ui/vendor/lucide-LICENSE.
      for(const [state,paths] of Object.entries({"closed": ["M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z", "M2 10h20"], "open": ["m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"]})){
        const shape=document.createElementNS('http://www.w3.org/2000/svg','g');shape.setAttribute('class','folder-'+state);
        for(const d of paths){const path=document.createElementNS('http://www.w3.org/2000/svg','path');path.setAttribute('d',d);shape.append(path);}icon.append(shape);
      }
      summary.append(icon,element('span','agent-project-name',project.name));
      const create=element('button','agent-project-chat','＋');create.type='button';create.title='在'+project.name+'中开始会话';create.setAttribute('aria-label',create.title);
      create.onclick=event=>{event.preventDefault();event.stopPropagation();agent.project=project.id;newSession();};
      const manage=element('button','agent-project-manage','⋯');manage.type='button';manage.title='管理项目';manage.setAttribute('aria-label','管理项目 '+project.name);
      manage.setAttribute('aria-haspopup','menu');manage.title='项目操作';
      const projectMenu=event=>{event.preventDefault();event.stopPropagation();agent.projectBrowser?.menu(manage,project,()=>window.keeperProjectDialog({project,invoke,notice,onChanged:async deleted=>{await bootstrap();if(deleted && agent.project===project.id)await switchProject('general');}}));};
      manage.onclick=projectMenu;manage.oncontextmenu=projectMenu;
      summary.append(manage,create);group.append(summary);
      const sessions=agent.history?.projectSessions(project.id) || agent.sessions.filter(s=>s.projectId===project.id);
      for(const session of sessions)group.append(sessionRow(session));
      if(agent.history)agent.history.attachProject(group,project.id);else if(!sessions.length)group.append(element('div','agent-nav-empty','暂无会话'));
      list.append(group);
    }
  }
  async function switchProject(project){
    if(agent.drafts && !await agent.drafts.load(null))return;
    clearTimeout(agent.timer);++agent.generation;agent.project=project;agent.selected=null;agent.detail={messages:[],runs:[]};
    renderProjects();renderSessions();renderConversation();showConversation();
    const session=agent.sessions.find(s=>s.projectId===project);if(session)await select(session.id);
  }
  $('#agent-new-project').onclick=async event=>{
    event.preventDefault();event.stopPropagation();
    const name=await uiPrompt('请输入项目名称','','新建项目');if(!name?.trim())return;
    try{const project=await invoke('agent_new_project',{name:name.trim()});await bootstrap();await switchProject(project.id);}catch(error){notice(String(error));}
  };
  async function bootstrap() {
    const epoch=agent.identityEpoch || 0,data=await invoke('agent_bootstrap');if(epoch!==(agent.identityEpoch || 0))return;agent.ownerKey=data.accountKey || 'guest';agent.projects=data.projects;agent.sessions=data.sessions;agent.history?.acceptBootstrap(data);
    renderProjects();renderModel(data.model);agent.tools=data.tools;agent.hosts=data.hosts;
    renderSessions();renderTools();
  }
  async function ensureSession(){
    if(agent.detail.historyPreview)throw new Error('正在预览历史版本，请先返回当前版本。');
    if(agent.detail.session?.archivedAt || agent.detail.session?.deletedAt)throw new Error('请先恢复会话，再发送消息或执行工具。');
    if(!agent.selected){
      const identity=agent.identityEpoch || 0,generation=agent.generation,project=agent.project;
      let creation=agent.creatingSession;
      if(!creation || creation.identity!==identity || creation.generation!==generation || creation.project!==project){creation={identity,generation,project,promise:invoke('agent_new_session',{projectId:project})};agent.creatingSession=creation;}
      try{const session=await creation.promise;if(identity!==(agent.identityEpoch || 0) || generation!==agent.generation || project!==agent.project || (agent.selected && agent.selected!==session.id))throw new Error('会话或账号已切换，请在当前会话重新添加附件或发送消息。');agent.selected=session.id;localStorage.setItem(sessionStorageKey(),agent.selected);}
      finally{if(agent.creatingSession===creation)agent.creatingSession=null;}
    }
    return agent.selected;
  }
  async function select(session) {
    try{if(agent.drafts && !await agent.drafts.load(session))return false;}catch(error){notice(String(error));return false;}
    clearTimeout(agent.timer);agent.selected=session;const generation=++agent.generation;
    try{
      const data=await invoke('agent_session',{sessionId:session});
      if(generation!==agent.generation)return;
      // A WebSocket checkpoint can arrive while this snapshot request is in flight.
      for(const item of data.messages){const current=agent.detail.messages.find(m=>m.id===item.id);if(current && current.content.length>item.content.length)item.content=current.content;}
      if(agent.detail.sessionId===session){
        for(const key of ['runs','tasks']){const id=key==='runs'?'id':'responseId';for(const current of agent.detail[key] || []){const index=(data[key] || []).findIndex(v=>v[id]===current[id]);if(index<0)(data[key] ||= []).push(current);else if((current.updatedAt || 0)>(data[key][index].updatedAt || 0))data[key][index]=current;}}
      }
      agent.detail=data;agent.history?.remember(data.session);renderModel(data.model);localStorage.setItem(sessionStorageKey(),session);renderProjects();renderSessions();renderConversation();poll();notice();return true;
    }catch(error){if(generation===agent.generation){if(String(error).includes('不属于当前账号'))clearSelected();notice(String(error));}return false;}
  }
  function poll(){
    clearTimeout(agent.timer);
    if(agent.detail.modelPending || agent.detail.runs.some(run=>run.status==='running'))agent.timer=setTimeout(()=>select(agent.selected),1100);
  }
  function securityOptions(){return [
    {id:'ask',name:'请求批准',icon:'ask',description:'读取项目无需确认；文件修改和命令逐次确认'},
    {id:'auto',name:'自动审批',icon:'auto',description:'自动批准项目内文件修改；未隔离的 Shell 命令仍需确认'},
    {id:'full',name:'完全访问',icon:'full',description:'跳过本机工具审批；命令可访问当前用户的文件与网络'}
  ];}
  function securityIcon(kind){
    const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');
    for(const [name,value] of Object.entries({viewBox:'0 0 24 24',fill:'none',stroke:'currentColor','stroke-width':'1.7','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true'}))svg.setAttribute(name,value);
    const paths={ask:['M12 3 3.5 7v5c0 4 3.5 7 8.5 9 5-2 8.5-5 8.5-9V7Z','M10.2 9.5a1.9 1.9 0 0 1 3.8.4c0 1.3-2 1.4-2 2.8','M12 15.6h.01'],auto:['M12 3 3.5 7v5c0 4 3.5 7 8.5 9 5-2 8.5-5 8.5-9V7Z','m8.5 12 2.3 2.3 4.8-4.8'],full:['M12 3 3.5 7v5c0 4 3.5 7 8.5 9 5-2 8.5-5 8.5-9V7Z','M12 8v5','M12 16h.01']};
    for(const d of paths[kind] || paths.ask){const path=document.createElementNS('http://www.w3.org/2000/svg','path');path.setAttribute('d',d);svg.append(path);}return svg;
  }
  function closeSecurityMenu(focus=false){$('#agent-security-menu').hidden=true;$('#agent-security-picker').setAttribute('aria-expanded','false');if(focus)$('#agent-security-picker').focus();}
  function renderSecurity(){
    const mode=agent.detail.securityMode || 'ask',options=securityOptions(),picker=$('#agent-security-picker'),menu=$('#agent-security-menu');
    const chosen=options.find(o=>o.id===mode) || options[0];$('#agent-security-label').textContent=chosen.name;picker.dataset.mode=mode;picker.disabled=!!agent.detail.modelPending;
    picker.title=chosen.description;menu.replaceChildren();
    menu.append(element('div','agent-security-heading','如何批准智能体操作？'));
    for(const option of options){const item=element('button','agent-security-option'+(option.id===mode?' selected':'')+(option.id==='full'?' full':''));item.type='button';item.setAttribute('role','menuitemradio');item.setAttribute('aria-checked',String(option.id===mode));
      const content=element('span','agent-security-copy');content.append(element('strong','',option.name),element('small','',option.description));const icon=element('span','agent-security-icon');icon.append(securityIcon(option.icon));item.append(icon,content,element('span','agent-security-check',option.id===mode?'✓':''));item.onclick=()=>chooseSecurity(option.id);menu.append(item);
    }
    menu.append(element('div','agent-security-footnote','仅作用于当前会话。文件工具仍限制在项目内；手动远程维护操作单独确认。'));
    if(picker.disabled)closeSecurityMenu();
  }
  let fullAccessFinish=null;
  function confirmFullAccess(){
    const modal=$('#full-access-modal'),cancel=$('#full-access-cancel');
    return new Promise(resolve=>{
      fullAccessFinish=approved=>{if(!fullAccessFinish)return;fullAccessFinish=null;modal.classList.add('hidden');$('#agent-security-picker').focus();resolve(approved);};
      modal.classList.remove('hidden');cancel.focus();
    });
  }
  $('#full-access-close').onclick=()=>fullAccessFinish?.(false);
  $('#full-access-cancel').onclick=()=>fullAccessFinish?.(false);
  $('#full-access-confirm').onclick=()=>fullAccessFinish?.(true);
  $('#full-access-modal').onclick=event=>{if(event.target===$('#full-access-modal'))fullAccessFinish?.(false);};
  $('#full-access-modal').onkeydown=event=>{
    if(event.key==='Escape'){event.preventDefault();event.stopPropagation();fullAccessFinish?.(false);}
    else if(event.key==='Tab'){const buttons=Array.from($('#full-access-modal').querySelectorAll('button:not(:disabled)')),first=buttons[0],last=buttons.at(-1);if(event.shiftKey && document.activeElement===first){event.preventDefault();last.focus();}else if(!event.shiftKey && document.activeElement===last){event.preventDefault();first.focus();}}
  };
  async function chooseSecurity(mode){
    if(agent.detail.modelPending || agent.changingSecurity)return;
    closeSecurityMenu();agent.changingSecurity=true;
    const selected=agent.selected,epoch=agent.identityEpoch || 0;
    try{
      if(mode==='full' && agent.detail.securityMode!=='full' && !await confirmFullAccess())return;
      if(selected!==agent.selected || epoch!==(agent.identityEpoch || 0))return;
      $('#agent-security-picker').disabled=true;
      const session=await ensureSession();await invoke('agent_security_mode',{sessionId:session,mode,confirmFullAccess:mode==='full'});if(agent.selected===session)await select(session);
    }catch(error){notice(String(error));}finally{agent.changingSecurity=false;$('#agent-security-picker').disabled=!!agent.detail.modelPending;}
  }
  $('#agent-security-picker').onclick=()=>{if(!$('#agent-security-menu').hidden){closeSecurityMenu(true);return;}if(agent.detail.modelPending)return;renderSecurity();closeModelMenu();$('#agent-tool-panel').hidden=true;$('#agent-security-menu').hidden=false;$('#agent-security-picker').setAttribute('aria-expanded','true');$('#agent-security-menu').querySelector('[aria-checked="true"]')?.focus();};
  $('#agent-security-menu').onkeydown=event=>{const items=Array.from($('#agent-security-menu').querySelectorAll('button')),index=items.indexOf(document.activeElement);if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeSecurityMenu(true);}else if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){event.preventDefault();const next=event.key==='Home'?0:event.key==='End'?items.length-1:(index+(event.key==='ArrowDown'?1:-1)+items.length)%items.length;items[next]?.focus();}};
  document.addEventListener('click',event=>{if(!event.target.closest('.agent-security-control'))closeSecurityMenu();});
  let llmConfig={servers:[],activeServerId:''},llmReturnFocus=null,llmRemote=false,llmIdentityEpoch=0;
  function renderModel(config){
    if(!config)return;
    renderSecurity();
    const defaultId=config.defaultServerId ?? config.activeServerId;
    const chosen=agent.selected && agent.detail.sessionId===agent.selected?agent.detail.model?.activeServerId:null;
    if(chosen!==null && chosen!==undefined){
      const selected=(config.servers || []).find(server=>server.id===chosen);
      config={...config,...selected,activeServerId:chosen,enabled:!!selected?.enabled,hasApiKey:!!selected?.hasApiKey,defaultServerId:defaultId};
    }
    llmConfig=config;
    const enabled=config.enabled && config.hasApiKey;
    $('.agent-model-badge').textContent=enabled?(config.name || 'LLM')+' · '+config.model:'未连接模型';
    const picker=$('#agent-model-picker'),menu=$('#agent-model-menu');
    const available=(config.servers || []).filter(server=>server.enabled && server.hasApiKey);
    $('#agent-model-label').textContent=enabled?config.name+' · '+config.model:available.length?'选择模型':'本地工具模式';
    picker.dataset.available=String(available.length);picker.disabled=!available.length || !!agent.detail.modelPending;
    picker.title=available.length?'选择当前对话使用的 LLM Server':'到账户 → 模型服务，启用并配置模型';
    menu.replaceChildren(element('div','agent-model-menu-title','选择模型'));
    for(const server of available){
      const current=enabled && server.id===config.activeServerId;
      const item=element('button','agent-model-option'+(current?' selected':''));item.type='button';item.setAttribute('role','menuitemradio');item.setAttribute('aria-checked',String(current));
      const info=element('span','agent-model-option-info');info.append(element('strong','',server.name),element('small','',server.model+' · '+(server.source==='cis'?'CIS 代理':'本地配置')));
      item.append(info,element('span','agent-model-option-check',current?'✓':''));
      item.onclick=()=>chooseModel(server.id);menu.append(item);
    }
    if(picker.disabled)closeModelMenu();
    $('#agent-model-note').textContent=enabled?(config.name || 'LLM')+' · '+(securityOptions().find(o=>o.id===(agent.detail.securityMode || 'ask'))?.description || '文件修改和命令需确认'):'尚未启用模型 · 会话保存在本机 · 远程命令执行前需要确认';
    const list=$('#llm-server-list');list.replaceChildren();
    if(!config.servers?.length)list.append(element('p','llm-empty','尚未添加 LLM Server，点击右侧按钮添加。'));
    for(const server of config.servers || []){
      const row=element('div','llm-server-row');const info=element('div','llm-server-info');
      const heading=element('div','llm-server-heading');heading.append(element('strong','',server.name));
      if(server.id===defaultId)heading.append(element('span','llm-current',server.enabled?'新会话默认':'默认服务 · 已停用'));
      info.append(heading,element('div','llm-server-meta',server.model+' · '+(server.source==='cis'?'CIS · '+(server.upstreamProvider==='deepseek'?'DeepSeek':'OpenAI 兼容'):(server.provider==='deepseek'?'DeepSeek':'OpenAI 兼容'))),element('div','llm-server-url',server.source==='cis'?'由 CIS 代理调用':server.baseUrl),element('small','',server.source==='cis'?'密钥由 CIS 管理':server.hasApiKey?'已配置 API Key':'未配置 API Key'));
      const actions=element('div','llm-server-actions');const edit=element('button','btn','编辑');edit.type='button';edit.onclick=()=>openLlm(server,edit);
      const test=element('button','btn','测试连接');test.type='button';test.disabled=!server.hasApiKey;test.onclick=async()=>{
        test.disabled=true;$('#llm-list-status').textContent='正在测试 '+server.name+'…';
        try{const result=await invoke('agent_model_test',{id:server.id});$('#llm-list-status').textContent=server.name+' 连接成功'+(result.models.length?' · '+result.models.join('、'):'');}catch(error){$('#llm-list-status').textContent=String(error);}finally{test.disabled=!server.hasApiKey;}
      };
      const use=element('button','btn',server.id===defaultId && server.enabled?'默认服务':'设为新会话默认');use.type='button';use.disabled=!server.hasApiKey || (server.id===defaultId && server.enabled);
      use.onclick=async()=>{try{renderModel(await invoke('agent_model_select',{id:server.id}));$('#llm-list-status').textContent='新会话默认使用 '+server.name+'，已有会话不变';}catch(error){$('#llm-list-status').textContent=String(error);}};
      const remove=element('button','btn','删除');remove.type='button';remove.onclick=async()=>{
        const epoch=agent.identityEpoch || 0;
        if(!await uiConfirm('删除本地模型“'+server.name+'”？本地保存的 API Key 也会被删除，使用该模型的会话需重新选择模型，历史消息会保留。','删除本地模型'))return;
        if(epoch!==(agent.identityEpoch || 0)){notice('登录账号已改变，请重新操作');return;}
        remove.disabled=true;
        try{const result=await invoke('agent_model_delete',{id:server.id});if(epoch!==(agent.identityEpoch || 0))return;renderModel(result);$('#llm-list-status').textContent='本地模型已删除';if(agent.selected)await select(agent.selected);}
        catch(error){$('#llm-list-status').textContent=String(error);}finally{remove.disabled=false;}
      };
      if(server.source==='cis')actions.append(use);else actions.append(use,edit,test,remove);row.append(info,actions);list.append(row);
    }
  }
  function closeModelMenu(focus=false){$('#agent-model-menu').hidden=true;$('#agent-model-picker').setAttribute('aria-expanded','false');if(focus)$('#agent-model-picker').focus();}
  function openModelMenu(last=false){
    if($('#agent-model-picker').disabled)return;
    $('#agent-tool-panel').hidden=true;$('#agent-model-menu').hidden=false;$('#agent-model-picker').setAttribute('aria-expanded','true');
    const items=Array.from($('#agent-model-menu').querySelectorAll('button'));const selected=items.find(item=>item.getAttribute('aria-checked')==='true');(last?items.at(-1):selected || items[0])?.focus();
  }
  async function chooseModel(id){
    if(agent.detail.modelPending)return;closeModelMenu(true);
    const picker=$('#agent-model-picker');picker.disabled=true;
    try{
      const session=await ensureSession();
      await invoke('agent_session_model',{sessionId:session,id});
      await bootstrap();if(agent.selected===session)await select(session);
    }catch(error){notice(String(error));}finally{picker.disabled=picker.dataset.available==='0' || !!agent.detail.modelPending;}
  }
  $('#agent-model-picker').onclick=()=>$('#agent-model-menu').hidden?openModelMenu():closeModelMenu(true);
  $('#agent-model-picker').onkeydown=event=>{if(event.key==='ArrowDown' || event.key==='ArrowUp'){event.preventDefault();openModelMenu(event.key==='ArrowUp');}};
  $('#agent-model-menu').onkeydown=event=>{
    if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeModelMenu(true);}
    else if(event.key==='Tab')closeModelMenu();
    else if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){
      event.preventDefault();const items=Array.from($('#agent-model-menu').querySelectorAll('button')),index=items.indexOf(document.activeElement);
      const next=event.key==='Home'?0:event.key==='End'?items.length-1:(index+(event.key==='ArrowDown'?1:-1)+items.length)%items.length;items[next]?.focus();
    }
  };
  document.addEventListener('click',event=>{if(!event.target.closest('.agent-model-control'))closeModelMenu();});
  function openLlm(server=null,trigger=null,remote=false){
    llmRemote=remote;llmIdentityEpoch=agent.identityEpoch || 0;
    llmReturnFocus=trigger || $('#llm-add');$('#llm-dialog-title').textContent=remote?(server?'编辑 CIS 模型':'添加 CIS 模型'):(server?'编辑 LLM Server':'添加 LLM Server');$('#llm-clear-key').closest('label').hidden=remote;$('#deepseek-test').hidden=remote;
    $('#llm-server-id').value=server?.id || '';$('#llm-server-name').value=server?.name || '';
    $('#llm-provider').value=server?.provider || 'deepseek';$('#deepseek-enabled').checked=server?.enabled || false;
    $('#deepseek-url').value=server?.baseUrl || 'https://api.deepseek.com';$('#deepseek-model').value=server?.model || 'deepseek-flash';
    $('#llm-context-limit').value=server?.contextLimit || 32768;$('#llm-images-option').hidden=remote;$('#llm-supports-images').checked=server?.supportsImages===true;
    $('#deepseek-key').value='';$('#deepseek-key').placeholder=server?.hasApiKey?'已保存密钥，留空保留':'请输入 API Key';$('#llm-clear-key').checked=false;$('#deepseek-status').textContent='';
    $('#llm-server-modal').classList.remove('hidden');$('#llm-server-name').focus();
  }
  function closeLlm(){const dialog=$('#llm-server-modal');dialog.classList.add('hidden');$('#deepseek-key').value='';(llmReturnFocus?.isConnected?llmReturnFocus:$('#llm-add')).focus();}
  async function saveLlm(){
    if(!$('#deepseek-form').reportValidity())return null;
    const clear=$('#llm-clear-key').checked;
    const payload={id:$('#llm-server-id').value,name:$('#llm-server-name').value.trim(),provider:$('#llm-provider').value,enabled:clear?false:$('#deepseek-enabled').checked,baseUrl:$('#deepseek-url').value.trim(),model:$('#deepseek-model').value.trim(),apiKey:$('#deepseek-key').value,contextLimit:Number($('#llm-context-limit').value)};
    if(llmRemote){if(llmIdentityEpoch!==(agent.identityEpoch || 0))throw new Error('登录账号已改变，请重新打开配置');const result=await invoke('agent_cis_model_save',payload);$('#deepseek-key').value='';if(llmIdentityEpoch!==(agent.identityEpoch || 0))throw new Error('模型已保存到原 CIS 账号，当前账号已切换');let refreshError=result.refreshError;try{await loadCisAdmin();await bootstrap();}catch(error){refreshError=refreshError || String(error);}if(refreshError)$('#llm-list-status').textContent='CIS 模型已保存，列表刷新失败：'+refreshError;return result.id;}
    const result=await invoke('agent_model_save',{...payload,clearApiKey:clear,supportsImages:$('#llm-supports-images').checked});
    const saved=result.servers.find(s=>s.id===result.savedServerId);$('#llm-server-id').value=result.savedServerId;$('#deepseek-key').value='';$('#deepseek-key').placeholder=saved.hasApiKey?'已保存密钥，留空保留':'请输入 API Key';$('#llm-clear-key').checked=false;$('#deepseek-enabled').checked=saved.enabled;renderModel(result);return result.savedServerId;
  }
  async function loadCisAdmin(){
    const adminEpoch=agent.identityEpoch || 0;const result=await invoke('agent_cis_models_admin');if(adminEpoch!==(agent.identityEpoch || 0))throw new Error('登录账号已改变，请重新打开模型管理');const list=$('#cis-model-admin-list');list.replaceChildren();
    for(const server of result.models || []){const row=element('div','llm-server-row'),info=element('div','llm-server-info');info.append(element('strong','',server.name),element('div','llm-server-meta',server.model+' · '+(server.enabled?'已启用':'已停用')),element('div','llm-server-url',server.baseUrl));const actions=element('div','llm-server-actions'),edit=element('button','btn','编辑'),remove=element('button','btn','删除');edit.type=remove.type='button';edit.onclick=()=>openLlm(server,edit,true);remove.onclick=async()=>{if(!await uiConfirm('删除 CIS 模型“'+server.name+'”？该模型将对所有用户不可用。','删除 CIS 模型'))return;try{if(adminEpoch!==(agent.identityEpoch || 0))throw new Error('登录账号已改变，请重新打开模型管理');const result=await invoke('agent_cis_model_delete',{id:server.id});if(adminEpoch!==(agent.identityEpoch || 0))throw new Error('模型已从原 CIS 账号删除，当前账号已切换');let refreshError=result.refreshError;try{await loadCisAdmin();await bootstrap();}catch(error){refreshError=refreshError || String(error);}$('#llm-list-status').textContent=refreshError?'模型已删除，列表刷新失败：'+refreshError:'CIS 模型已删除';}catch(error){$('#llm-list-status').textContent=String(error);}};actions.append(edit,remove);row.append(info,actions);list.append(row);}
    if(!result.models?.length)list.append(element('p','llm-empty','尚未配置 CIS 模型。'));$('#cis-model-admin').hidden=false;
  }
  async function loadCisUsage(all){
    const epoch=agent.identityEpoch || 0,result=await invoke('agent_cis_usage',{all});
    if(epoch!==(agent.identityEpoch || 0))return;
    const content=$('#cis-model-usage-content');content.replaceChildren();$('#cis-model-usage-title').textContent=all?'全部账号调用统计':'我的调用统计';
    if(!result.rows?.length)content.append(element('p','llm-empty','暂无调用记录。'));
    else{
      const table=element('table','cis-usage-table'),head=document.createElement('thead'),header=document.createElement('tr');
      for(const name of ['账号','模型','调用','完成','失败','中断','进行中','用量未知','输入 Token','输出 Token','总 Token'])header.append(element('th','',name));head.append(header);table.append(head);
      const body=document.createElement('tbody');for(const row of result.rows){const tr=document.createElement('tr');for(const value of [row.userName || row.userId,row.model,row.calls,row.completed,row.failed,row.disconnected,row.inProgress,row.missingUsage,row.promptTokens,row.completionTokens,row.totalTokens])tr.append(element('td','',String(value ?? '')));body.append(tr);}table.append(body);content.append(table);
      if(result.truncated)content.append(element('p','hint','结果超过 500 组，仅显示最近的 500 组。'));
    }
    $('#cis-model-usage').hidden=false;
  }
  for(const [selector,all] of [['#cis-model-my-usage',false],['#cis-model-all-usage',true]])$(selector).onclick=async()=>{try{await loadCisUsage(all);$('#llm-list-status').textContent='';}catch(error){$('#cis-model-usage').hidden=true;$('#llm-list-status').textContent=String(error);}};
  $('#cis-model-manage').onclick=async()=>{try{await loadCisAdmin();$('#llm-list-status').textContent='';}catch(error){$('#cis-model-admin').hidden=true;$('#llm-list-status').textContent=String(error);}};
  $('#cis-model-add').onclick=()=>openLlm(null,$('#cis-model-add'),true);
  $('#cis-model-refresh').onclick=async()=>{try{renderModel(await invoke('agent_cis_models_refresh'));if(agent.selected)await select(agent.selected);$('#llm-list-status').textContent='CIS 模型列表已刷新';}catch(error){$('#llm-list-status').textContent=String(error);}};
  $('#llm-add').onclick=()=>openLlm();$('#llm-cancel').onclick=closeLlm;$('#llm-dialog-close').onclick=closeLlm;
  $('#llm-server-modal').onkeydown=event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeLlm();}else if(event.key==='Tab'){
    const nodes=Array.from($('#llm-server-modal').querySelectorAll('button:not(:disabled),input:not([type=hidden]),select'));const first=nodes[0],last=nodes[nodes.length-1];
    if(event.shiftKey && document.activeElement===first){event.preventDefault();last.focus();}else if(!event.shiftKey && document.activeElement===last){event.preventDefault();first.focus();}
  }};
  $('#deepseek-form').onsubmit=async event=>{event.preventDefault();const buttons=Array.from($('#deepseek-form').querySelectorAll('button'));buttons.forEach(b=>b.disabled=true);try{if(await saveLlm()){const remote=llmRemote;closeLlm();if(!$('#llm-list-status').textContent.includes('刷新失败'))$('#llm-list-status').textContent=remote?'CIS 模型已保存':'LLM Server 已保存';}}catch(error){$('#deepseek-status').textContent=String(error);}finally{buttons.forEach(b=>b.disabled=false);}};
  $('#deepseek-test').onclick=async()=>{
    const button=$('#deepseek-test');button.disabled=true;$('#deepseek-status').textContent='正在保存并测试连接…';
    try{const id=await saveLlm();if(!id)return;const result=await invoke('agent_model_test',{id});$('#deepseek-status').textContent='连接成功'+(result.models.length?' · '+result.models.join('、'):'');}catch(error){$('#deepseek-status').textContent=String(error);}finally{button.disabled=false;}
  };
  function historyBusy(busy){$('#history-backup').disabled=busy;$('#history-restore').disabled=busy;}
  $('#history-backup').onclick=async()=>{const epoch=agent.identityEpoch || 0;historyBusy(true);$('#history-backup-status').textContent='正在生成备份…';try{const backup=await invoke('agent_history_backup');if(epoch!==(agent.identityEpoch || 0))throw new Error('账号已切换，请重新备份');const blob=new Blob([JSON.stringify(backup)],{type:'application/json'}),url=URL.createObjectURL(blob),link=document.createElement('a');link.href=url;link.download='cangling-keeper-history-'+new Date().toISOString().replace(/[:.]/g,'-')+'.json';document.body.append(link);link.click();link.remove();setTimeout(()=>URL.revokeObjectURL(url),1000);$('#history-backup-status').textContent='备份已下载，共 '+backup.snapshot.sessions.length+' 个会话。';}catch(error){$('#history-backup-status').textContent='备份失败：'+String(error);}finally{historyBusy(false);}};
  $('#history-restore').onclick=()=>{$('#history-backup-file').value='';$('#history-backup-file').click();};
  async function restoreHistoryRequest(args){const response=await fetch('/api/history/restore',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(args)});if(response.status===413)throw new Error('备份文件过大');const data=await response.json();if(!response.ok || !data.ok)throw new Error(data.error || '无法恢复备份');return data.result;}
  $('#history-backup-file').onchange=async event=>{const file=event.target.files?.[0];if(!file)return;historyBusy(true);const epoch=agent.identityEpoch || 0;$('#history-backup-status').textContent='正在检查备份…';try{
    if(file.size>24*1024*1024)throw new Error('备份文件不能超过 24 MiB');const backup=JSON.parse(await file.text());const preview=await restoreHistoryRequest({mode:'preview',backup});if(epoch!==(agent.identityEpoch || 0))throw new Error('账号已切换，请重新选择备份');
    const message='备份包含 '+preview.projects+' 个关联项目、'+preview.sessions+' 个会话、'+preview.messages+' 条消息、'+preview.runs+' 条工具记录、'+(preview.attachments || 0)+' 个附件、'+(preview.mcpImages || 0)+' 张 MCP 图片。'+(preview.differentAccount?' 此备份来自其他账户，将导入当前账户。':'')+'\n恢复将新增会话并保留现有数据。工具不会重新执行，项目文件不会恢复。是否继续？';
    if(!await uiConfirm(message,'恢复对话备份')){$('#history-backup-status').textContent='已取消恢复';return;}
    const result=await restoreHistoryRequest({mode:'restore',backup,confirmed:true,expectedAccount:preview.accountKey});if(epoch!==(agent.identityEpoch || 0))throw new Error('备份已恢复到确认时的账户，当前账号已切换');await bootstrap();$('#history-backup-status').textContent='已恢复 '+result.sessions+' 个会话，原有数据已保留。';
  }catch(error){$('#history-backup-status').textContent='恢复失败：'+String(error);}finally{historyBusy(false);}};
  let editingMcp=null,mcpEditEpoch=0;
  const mcpSettings=window.createMcpSettingsForm?.({element});
  function closeMcp(){$('#mcp-modal').classList.add('hidden');editingMcp=null;mcpSettings?.reset();}
  function updateMcpTransport(){const local=$('#mcp-transport').value==='stdio';$('#mcp-command-label').textContent=local?'启动命令':'服务地址';$('#mcp-args-label').textContent=local?'启动参数（每行一个）':'请求头（每行 名称: 值）';$('#mcp-command').placeholder=local?'例如 python3':'例如 http://127.0.0.1:64342/sse';$('#mcp-args').placeholder=local?'例如 /path/to/server.py':'例如 IJ_MCP_SERVER_PROJECT_PATH: /path/to/project';$('#mcp-hint').textContent=local?'连接时以当前用户权限运行命令，请只连接信任的服务。':'远程地址需要 HTTPS；本机允许 HTTP。请求头保存在本机，请求时会发送到此服务；切换为远程连接会清除本机进程环境变量。';mcpSettings?.update();}
  function openMcp(server=null){mcpEditEpoch=agent.identityEpoch || 0;editingMcp=server;$('#mcp-title').textContent=server?'编辑 MCP Server':'添加 MCP Server';$('#mcp-name').value=server?.name || '';$('#mcp-transport').value=server?.transport || 'stdio';$('#mcp-command').value=server?.command || '';$('#mcp-args').value=(server?.args || []).join('\n');mcpSettings?.fill(server,agent.projects);updateMcpTransport();$('#mcp-form-status').textContent='';$('#mcp-modal').classList.remove('hidden');$('#mcp-name').focus();}
  async function useMcpCapability(server,tool){
    if(agent.detail.session?.archivedAt || agent.detail.session?.deletedAt || agent.detail.historyPreview){notice('请先恢复会话或返回当前版本，再使用 MCP 能力。');return;}
    const epoch=agent.identityEpoch || 0,project=agent.detail.session?.projectId || agent.project;
    if(server.projectIds!=null && (!Array.isArray(server.projectIds) || !server.projectIds.includes(project))){notice('当前项目未启用此 MCP 服务，请先调整服务的项目范围。');return;}
    if(!agent.selected){await newSession();if(!agent.selected || epoch!==(agent.identityEpoch || 0))return;}
    const tasks={'resources/list':'请列出 MCP 服务「'+server.name+'」提供的资料。','resources/read':'请读取 MCP 服务「'+server.name+'」中的资料，URI：','resources/templates/list':'请列出 MCP 服务「'+server.name+'」提供的资料 URI 模板。','prompts/list':'请列出 MCP 服务「'+server.name+'」提供的提示模板和所需参数。','prompts/get':'请使用 MCP 服务「'+server.name+'」的提示模板，名称及参数：'};
    const input=$('#agent-input');input.value=input.value+(input.value && !input.value.endsWith('\n')?'\n':'')+(tasks[tool.kind] || '');showConversation();input.dispatchEvent(new Event('input',{bubbles:true}));input.focus();
  }
  function renderMcp(servers){
    const list=$('#mcp-list');list.replaceChildren();
    for(const server of servers){
      const epoch=agent.identityEpoch || 0,current=()=>epoch===(agent.identityEpoch || 0),tools=server.tools || [],checked=(tool,index)=>tool.enabled ?? (!tool.kind && index<8);
      const native=tools.filter(t=>t.kind),ordinary=tools.filter(t=>!t.kind),chosen=tools.filter(checked),nativeSelected=native.filter(t=>t.enabled===true).length;
      const row=element('div','llm-server-row'),info=element('div','llm-server-info');
      info.append(element('strong','',server.name),element('div','llm-server-meta',(server.enabled?'已启用':'未启用')+' · 工具 '+chosen.filter(t=>!t.kind).length+'/'+ordinary.length+(native.length?' · 资料与模板 '+nativeSelected+'/'+native.length:'')+' · '+(server.connectedSessions?'连接会话 '+server.connectedSessions:'按需连接')),element('div','llm-server-url',server.command));
      const scope=Array.isArray(server.projectIds)?'指定项目：'+server.projectIds.map(id=>agent.projects.find(p=>p.id===id)?.name || id).join('、'):'所有项目';
      info.append(element('div','mcp-server-scope',scope+(server.environmentKeys?.length?' · 环境变量 '+server.environmentKeys.length+' 项':'')));
      if(server.plugin?.manifest){const p=server.plugin.manifest;info.append(element('div','mcp-server-scope','插件 '+p.id+' · '+p.version+' · 来源声明（未验证）：'+p.source),element('div','mcp-server-scope','原始清单 SHA-256：'+server.plugin.sha256+'；以当前连接配置为准'));}
      const actions=element('div','llm-server-actions');
      const operations=[['编辑',()=>{if(current())openMcp(server);}],[server.plugin?.manifest?'启用并发现':'连接并发现',async()=>{
        if(!await uiConfirm((server.transport==='stdio'?'将以当前用户权限运行 MCP Server：':'将连接 MCP 服务并发送配置的请求头：')+server.command+'。请确认你信任该服务。','连接 MCP Server') || !current())return;
        try{$('#mcp-status').textContent='正在连接…';const result=await invoke('agent_mcp_refresh',{id:server.id});if(!current())return;renderMcp(result);const native=result.find(s=>s.id===server.id)?.tools?.some(t=>t.kind);$('#mcp-status').textContent=native?'已发现服务能力；资料与提示模板需要勾选后保存，才可用于对话。':'已发现工具，可在选择能力中调整。';}catch(error){if(current())$('#mcp-status').textContent=String(error);}
      }],...(server.enabled?[['停用',async()=>{try{const result=await invoke('agent_mcp_save',{...server,enabled:false});if(current())renderMcp(result.servers);}catch(error){if(current())$('#mcp-status').textContent=String(error);}}]]:[]),[server.plugin?.manifest?'卸载':'删除',async()=>{
        if(!await uiConfirm('删除 MCP 服务配置“'+server.name+'”？项目文件和历史记录保留。','删除 MCP Server') || !current())return;
        try{const result=await invoke('agent_mcp_delete',{id:server.id});if(current())renderMcp(result);}catch(error){if(current())$('#mcp-status').textContent=String(error);}
      }]];
      for(const [label,handler] of operations){const b=element('button','btn',label);b.type='button';b.onclick=handler;actions.append(b);}row.append(info,actions);const group=element('div','mcp-server-group');group.dataset.serverId=server.id;group.append(row);
      if(tools.length){
        const chooser=element('details','mcp-tools');chooser.append(element('summary','','选择能力（已启用 '+chosen.length+' / '+tools.length+'）'));const grid=element('div','mcp-tools-list');
        for(const [heading,subset] of [['工具',ordinary],['资料与提示模板',native]]){
          if(!subset.length)continue;const section=element('section','mcp-capability-section');section.append(element('h4','',heading));
          if(heading==='资料与提示模板')section.append(element('p','hint','默认关闭。勾选后可在对话中请求读取；“用于对话”只加入草稿。模板内容按外部资料处理。'));
          const options=element('div','mcp-capability-options');
          for(const tool of subset){
            const index=tools.indexOf(tool),option=element('div','mcp-capability-option'),label=element('label','mcp-tool-option'),check=element('input');check.type='checkbox';check.checked=checked(tool,index);check.value=tool.name;
            const caption=element('span','mcp-capability-caption');caption.append(element('span','',tool.title || tool.name));if(tool.description)caption.append(element('small','',tool.description));label.append(check,caption);option.append(label);
            if(tool.kind){const use=element('button','mcp-capability-use','用于对话');use.type='button';const project=agent.detail.session?.projectId || agent.project,allowed=server.projectIds==null || (Array.isArray(server.projectIds) && server.projectIds.includes(project));use.disabled=!server.enabled || !checked(tool,index) || !allowed || !!(agent.detail.session?.archivedAt || agent.detail.session?.deletedAt || agent.detail.historyPreview);if(!allowed)use.title='当前项目未启用此 MCP 服务';use.onclick=()=>{if(current())useMcpCapability(server,tool);};option.append(use);}options.append(option);
          }
          section.append(options);grid.append(section);
        }
        const save=element('button','btn','保存能力选择');save.type='button';save.onclick=async()=>{save.disabled=true;try{const result=await invoke('agent_mcp_tools',{id:server.id,tools:[...grid.querySelectorAll('input:checked')].map(input=>input.value)});if(!current())return;renderMcp(result);$('#mcp-status').textContent='能力选择已保存；可在对话中使用已选能力。';}catch(error){if(current()){$('#mcp-status').textContent=String(error);save.disabled=false;}}};chooser.append(grid,save);group.append(chooser);
      }
      list.append(group);
    }
    if(!servers.length)list.append(element('p','llm-empty','尚未配置 MCP 服务。'));
  }
  let mcpLoadSequence=0;
  async function loadMcp(){const sequence=++mcpLoadSequence;try{const servers=await invoke('agent_mcp_list');if(sequence===mcpLoadSequence)renderMcp(servers);}catch(error){if(sequence===mcpLoadSequence)$('#mcp-status').textContent=String(error);}}
  $('#mcp-disconnect').onclick=async()=>{const epoch=agent.identityEpoch || 0;if(!await uiConfirm('断开所有 MCP 连接？正在调用的 MCP 工具将停止，已发生的外部操作需要核实。','断开 MCP 连接') || epoch!==(agent.identityEpoch || 0))return;try{const result=await invoke('agent_mcp_disconnect');if(epoch!==(agent.identityEpoch || 0))return;renderMcp(result);$('#mcp-status').textContent='连接已断开；下次调用会重新建立会话';}catch(error){if(epoch===(agent.identityEpoch || 0))$('#mcp-status').textContent=String(error);}};
  $('#mcp-transport').onchange=updateMcpTransport;$('#mcp-add').onclick=()=>openMcp();$('#mcp-close').onclick=closeMcp;$('#mcp-cancel').onclick=closeMcp;
  $('#mcp-form').onsubmit=async event=>{event.preventDefault();const epoch=mcpEditEpoch;if(epoch!==(agent.identityEpoch || 0))return;try{const extra=mcpSettings?.args($('#mcp-transport').value) || {};const result=await invoke('agent_mcp_save',{id:editingMcp?.id || '',name:$('#mcp-name').value,transport:$('#mcp-transport').value,command:$('#mcp-command').value,args:$('#mcp-args').value.split('\n').filter(x=>x.trim()),enabled:editingMcp?.enabled || false,...extra});if(epoch!==(agent.identityEpoch || 0))return;renderMcp(result.servers);closeMcp();$('#mcp-status').textContent='已保存；连接并发现服务能力后可供对话使用';}catch(error){if(epoch===(agent.identityEpoch || 0))$('#mcp-form-status').textContent=String(error);}};
  window.addEventListener('keeper-login-state',()=>loadMcp());loadMcp();
  invoke('agent_model_config').then(renderModel).catch(error=>{$('#llm-list-status').textContent=String(error);});
  function renderTools(){
    const list=$('#agent-tools');list.replaceChildren();
    for(const tool of agent.tools){
      const button=element('button','agent-tool-option');button.type='button';button.append(element('span','',tool.title+(tool.approval?' · 执行前确认':'')),element('small','',tool.description));
      button.onclick=()=>chooseTool(tool);list.append(button);
    }
    const suggestions=$('#agent-suggestions');suggestions.replaceChildren();
    for(const name of ['list_hosts','proxy_status','root_certificate_status']){
      const tool=agent.tools.find(t=>t.name===name);if(!tool)continue;
      const button=element('button','agent-suggestion',tool.title);button.type='button';button.append(element('small','',tool.description));button.onclick=()=>chooseTool(tool);suggestions.append(button);
    }
  }
  function renderPending(container){
    const readonly=!!(agent.detail.session?.archivedAt || agent.detail.session?.deletedAt || agent.detail.historyPreview);
    for(const button of container.querySelectorAll('.agent-regenerate'))button.disabled=readonly || !!agent.detail.modelPending || !!agent.regenerating || button.dataset.available!=='true';
    $('#agent-send').disabled=readonly || agent.sending || !!agent.detail.modelPending;
    $('#agent-send').hidden=!!agent.detail.modelPending;$('#agent-stop').hidden=!agent.detail.modelPending;
    $('#agent-stop').disabled=!!agent.stopping || !!agent.detail.stopRequested;
    $('#agent-stop').title=agent.detail.stopRequested?'正在停止…':'停止生成';
    $('#agent-model-picker').disabled=$('#agent-model-picker').dataset.available==='0' || !!agent.detail.modelPending;
    $('#agent-security-picker').disabled=!!agent.detail.modelPending || !!agent.changingSecurity;
    if(agent.detail.modelPending){closeModelMenu();$('#agent-security-menu').hidden=true;$('#agent-security-picker').setAttribute('aria-expanded','false');}
    for(const node of container.querySelectorAll('.agent-message'))node.classList.toggle('streaming',agent.detail.modelPending && node.dataset.messageId===agent.detail.streamingMessageId);
    for(const actions of container.querySelectorAll('[data-review-run]'))for(const button of actions.querySelectorAll('button'))button.disabled=readonly || !!agent.reviewing?.has(actions.dataset.reviewRun) || !!agent.stopping || !!agent.detail.stopRequested;
    for(const button of container.querySelectorAll('.agent-edit-message'))button.disabled=readonly || !!agent.detail.modelPending || !!agent.savingEdit;
    for(const button of container.querySelectorAll('.agent-version-button'))button.disabled=button.dataset.boundary==='true' || !!agent.detail.modelPending;
    let status=container.querySelector('.agent-stream-status');
    if(agent.detail.modelPending){if(!status){status=element('div','agent-message-label agent-stream-status');status.setAttribute('role','status');container.append(status);}const task=agent.detail.tasks?.find(t=>t.responseId===agent.detail.streamingMessageId);status.textContent=agent.detail.stopRequested?'正在停止生成…':agent.detail.messages.find(m=>m.id===agent.detail.streamingMessageId)?.modelMetadata?.context?.status==='compressing'?'正在整理上下文…':task?.status==='approval'?`等待你确认操作 · 第 ${task.step} 轮`:task?.status==='tool'?`正在执行工具 · 第 ${task.step} 轮`:`模型正在生成…${task?.step?' · 第 '+task.step+' 轮':''}`;}
    else status?.remove();
    agent.history?.updateDetail();agent.guidance?.update();agent.attachments?.render();agent.runExperience?.tick();
  }
  async function saveEditedMessage(entry){
    if(agent.savingEdit || agent.detail.modelPending || !agent.editing)return;
    const content=agent.editing.content.trim();if(!content && !entry.attachments?.length){notice('消息内容不能为空。');return;}if(content===entry.content){agent.editing=null;renderConversation();return;}
    const session=agent.selected,anchor=replyAnchor(entry.replyGroup?.rootId || entry.id);agent.savingEdit=true;
    for(const button of $('#agent-messages').querySelectorAll('.agent-message-edit button'))button.disabled=true;
    try{await invoke('agent_edit_message',{sessionId:session,messageId:entry.id,content});agent.editing=null;await bootstrap();if(agent.selected===session){await select(session);restoreReplyAnchor(anchor);}notice();}
    catch(error){notice(String(error));}finally{agent.savingEdit=false;for(const button of $('#agent-messages').querySelectorAll('.agent-message-edit button'))button.disabled=false;}
  }
  function visibleMessages(){
    const groups=agent.detail.replyGroups || [],messages=agent.detail.messages;
    const alternatives=new Set(groups.flatMap(g=>(g.allVersionIds || g.versionIds).slice(1)));
    return messages.filter(m=>!alternatives.has(m.id)).map(m=>{
      const group=groups.find(g=>g.rootId===m.id);if(!group)return m;
      const chosen=messages.find(v=>v.id===group.selectedId);if(!chosen)return null;
      return {...chosen,displayCreatedAt:chosen.createdAt,createdAt:m.createdAt,replyGroup:group};
    }).filter(Boolean);
  }
  function replyAnchor(rootId){
    const timeline=$('#agent-timeline'),node=[...$('#agent-messages').querySelectorAll('[data-root-message-id]')].find(n=>n.dataset.rootMessageId===rootId);
    return {timeline,top:node?.getBoundingClientRect().top,scroll:timeline.scrollTop,rootId};
  }
  function restoreReplyAnchor(anchor){
    const node=[...$('#agent-messages').querySelectorAll('[data-root-message-id]')].find(n=>n.dataset.rootMessageId===anchor.rootId);
    anchor.timeline.scrollTop=anchor.scroll;
    if(node && anchor.top!==undefined)anchor.timeline.scrollTop+=node.getBoundingClientRect().top-anchor.top;
  }
  async function chooseReplyVersion(group,index){
    if(agent.detail.modelPending || agent.switchingVersion)return;
    const session=agent.selected,anchor=replyAnchor(group.rootId);
    if(agent.detail.session?.archivedAt || agent.detail.session?.deletedAt || agent.detail.historyPreview){if(agent.history)agent.history.selectReadingVersion(group.versionIds[index]);else group.selectedId=group.versionIds[index];renderConversation();restoreReplyAnchor(anchor);return;}
    agent.switchingVersion=true;
    try{await invoke('agent_reply_version',{sessionId:session,rootId:group.rootId,messageId:group.versionIds[index]});if(agent.selected===session){await select(session);restoreReplyAnchor(anchor);}}
    catch(error){notice(String(error));}finally{agent.switchingVersion=false;}
  }
  function renderConversation(){
    const container=$('#agent-messages'),timeline=$('#agent-timeline');
    window.keeperThreadNav?.begin({account:agent.ownerKey || 'guest',session:agent.selected});
    const follow=timeline.scrollHeight-timeline.scrollTop-timeline.clientHeight<90;
    const draft=container.querySelector('.agent-run-form');
    const toolOpen=new Map([...container.querySelectorAll('.agent-compact-run')].map(node=>[node.dataset.runId,node.open]));
    const signature=JSON.stringify([agent.selected,agent.detail.messages.map(m=>[m.id,m.role,m.modelMetadata,m.attachments]),agent.detail.replyGroups,agent.detail.runs,agent.detail.tasks,agent.editing?.session===agent.selected?agent.editing.id:null]);
    if(!draft && container.dataset.signature===signature){
      for(const entry of agent.detail.messages){const body=container.querySelector('[data-message-id="'+entry.id+'"] .agent-message-body');if(body)window.keeperMarkdown.render(body,entry.content,entry.role==='assistant');}
      renderPending(container);if(follow)timeline.scrollTop=timeline.scrollHeight;window.keeperThreadNav?.update({messages:visibleMessages(),detail:agent.detail});return;
    }
    container.dataset.signature=signature;container.replaceChildren();
    $('#agent-welcome').hidden=agent.detail.messages.length>0 || agent.detail.runs.length>0;
    const entries=[...visibleMessages().map(m=>({...m,kind:'message'})),...agent.detail.runs.filter(run=>!run.responseId).map(run=>({...run,kind:'run'}))].sort((a,b)=>a.createdAt-b.createdAt);
    let replyNumber=0;
    for(const entry of entries){
      if(entry.kind==='message'){
        const message=element('article','agent-message '+entry.role);message.dataset.messageId=entry.id;message.dataset.rootMessageId=entry.replyGroup?.rootId || entry.id;
        if(entry.role!=='user'){
          if(entry.role==='assistant'){const number=element('span','agent-reply-number',String(++replyNumber));number.setAttribute('aria-label','回复 '+replyNumber);message.append(number);}
          const metadata=entry.modelMetadata || {};
          const label=entry.role==='error'?'模型调用失败':metadata.source==='local'?'苍灵智能 · 本地工具':metadata.model?(metadata.serverName?metadata.serverName+' · ':'')+metadata.model:'苍灵智能 · 模型未记录';
          const heading=element('span','agent-message-label agent-reply-model',label);
          if(metadata.requestedModel && metadata.requestedModel!==metadata.model)heading.title='请求模型：'+metadata.requestedModel+'；服务端返回模型：'+metadata.model;
          const replyHeader=element('div','agent-reply-header');replyHeader.append(heading);
          if(metadata.context || metadata.usage){
            const c=metadata.context || {},u=metadata.usage || {},labels={compressing:'正在整理上下文…',compacted:'上下文已压缩',fallback:'摘要不可用，已裁剪旧上下文',stopped:'上下文压缩已停止',interrupted:'请求已中断',ready:'上下文就绪'};
            const parts=[labels[c.status] || ''];
            if(c.estimatedTokens)parts.push('输入保守估算 '+c.estimatedTokens.toLocaleString()+' / '+c.limit.toLocaleString()+' token');
            if(u.promptTokens!==undefined)parts.push('累计实际输入 '+u.promptTokens.toLocaleString()+' · 输出 '+u.completionTokens.toLocaleString()+' token（含压缩）');
            const info=element('div','agent-context-info',parts.filter(Boolean).join(' · '));info.title='预算使用保守估算；实际用量来自服务端 usage 累计，包含摘要调用。中断且未返回 usage 的调用可能尚未计入。';info.setAttribute('role','status');replyHeader.append(info);
          }
          message.append(replyHeader);
          if(['stopped','interrupted'].includes(metadata.generationStatus))message.append(element('span','agent-generation-status',metadata.generationStatus==='stopped'?'已停止生成':'生成已中断'));
        }
        if(entry.role==='assistant'){
          const runs=agent.detail.runs.filter(run=>run.responseId===entry.id);
          const task=agent.detail.tasks?.find(t=>t.responseId===entry.id),metadata=entry.modelMetadata || {};
          if(task){
            const labels={model:'模型生成中',tool:'执行工具中',approval:'等待确认',complete:'已完成',failed:'执行失败',stopped:'已停止',interrupted:'已中断'};
            const summary=element('details','agent-task-summary'),caption=element('summary','',`${labels[task.status] || '任务记录'} · ${task.step || 0} 轮 · 工具调用 ${runs.length} 次`);
            const terminal=['complete','failed','stopped','interrupted'].includes(task.status);
            const reason=metadata.taskReason || (task.status==='interrupted'?'服务中断，执行记录已保留；不会自动重放操作。':terminal?'历史任务未记录具体结束原因。':'任务进行中，可随时停止生成。');
            summary.append(caption,element('p','',reason));
            const counts={succeeded:0,failed:0,rejected:0,stopped:0,interrupted:0};for(const run of runs)if(run.status in counts)counts[run.status]++;
            summary.append(element('p','',`工具结果：成功 ${counts.succeeded} · 失败 ${counts.failed} · 拒绝 ${counts.rejected} · 停止/中断 ${counts.stopped+counts.interrupted}`));
            summary.open=!!agent.taskSummaryOpen?.has(entry.id);summary.ontoggle=()=>{agent.taskSummaryOpen ||= new Set();if(summary.open)agent.taskSummaryOpen.add(entry.id);else agent.taskSummaryOpen.delete(entry.id);};message.append(summary);
          }
          if(runs.length){const steps=element('div','agent-tool-steps');steps.setAttribute('aria-label','工具执行过程');for(const run of runs)steps.append(runCard(run,toolOpen));message.append(steps);}
        }
        const body=element('div','agent-message-body');window.keeperMarkdown.render(body,entry.content,entry.role==='assistant');body.hidden=entry.role==='user' && !entry.content?.trim();message.append(body);agent.attachments?.decorate(message,entry);
        if(entry.role==='user' && agent.editing?.session===agent.selected && agent.editing.id===entry.id){
          body.hidden=true;const form=element('form','agent-message-edit'),input=element('textarea');input.value=agent.editing.content;input.setAttribute('aria-label','编辑消息');input.rows=Math.max(3,Math.min(10,input.value.split('\n').length));input.oninput=()=>{agent.editing.content=input.value;};
          const actions=element('div','agent-edit-actions'),cancel=element('button','btn','取消'),save=element('button','btn primary','保存并重新生成');cancel.type='button';save.type='submit';
          cancel.onclick=()=>{agent.editing=null;renderConversation();};form.onsubmit=event=>{event.preventDefault();saveEditedMessage(entry);};actions.append(cancel,save);form.append(input,actions);message.append(form);
        }
        if(entry.role==='assistant' || entry.role==='user'){
          const metadata=entry.modelMetadata || {},available=metadata.source==='llm' && !!metadata.serverId && !!metadata.requestedModel;
          const regenerate=element('button','agent-regenerate','↻ 重新生成');regenerate.type='button';regenerate.dataset.available=String(available);regenerate.title=available?'使用本条回复原来的模型和上下文重新生成':'未记录原模型，无法重新生成';
          regenerate.onclick=()=>regenerateReply(entry.id);
          const footer=element('div','agent-reply-actions');
          if(entry.role==='assistant')footer.append(regenerate);
          else{const timestamp=Number(entry.displayCreatedAt ?? entry.createdAt);if(Number.isFinite(timestamp) && timestamp>0){const time=element('time','agent-message-time',relativeMessageTime(timestamp));time.dataset.time=String(timestamp);time.dateTime=new Date(timestamp).toISOString();time.title=new Date(timestamp).toLocaleString();footer.append(time);}const edit=element('button','agent-edit-message','✎ 编辑');edit.type='button';edit.disabled=!!agent.detail.modelPending;edit.onclick=()=>{agent.editing={session:agent.selected,id:entry.id,content:entry.content};renderConversation();$('#agent-messages .agent-message-edit textarea')?.focus();};footer.append(edit);}
          if(entry.role==='user' && agent.editing?.session===agent.selected && agent.editing.id===entry.id)footer.hidden=true;
          const group=entry.replyGroup;
          if(group && group.versionIds.length>1){
            const navigation=element('div','agent-reply-versions');navigation.setAttribute('aria-label',entry.role==='user'?'消息版本':'回复版本');
            const index=group.versionIds.indexOf(group.selectedId);
            for(const [direction,label,offset] of [['previous','上一版本',-1],['next','下一版本',1]]){
              const button=element('button','agent-version-button',direction==='previous'?'‹':'›');button.type='button';button.title=label;button.setAttribute('aria-label',label);
              button.dataset.boundary=String(index+offset<0 || index+offset>=group.versionIds.length);button.disabled=button.dataset.boundary==='true' || !!agent.detail.modelPending;
              button.onclick=()=>chooseReplyVersion(group,index+offset);
              if(offset===1)navigation.append(element('span','agent-version-count',`${index+1} / ${group.versionIds.length}`));navigation.append(button);
            }
            footer.append(navigation);
          }
          message.append(footer);
        }
        container.append(message);
      }else container.append(runCard(entry,toolOpen));
    }
    renderPending(container);
    if(draft && draft.dataset.sessionId===(agent.selected || '')){container.append(draft);$('#agent-welcome').hidden=true;}
    if(follow)timeline.scrollTop=timeline.scrollHeight;
    window.keeperThreadNav?.update({messages:visibleMessages(),detail:agent.detail});
  }
  window.__KEEPER__.event.listen('agent-context-update',event=>{
    const update=event.payload;if(update.sessionId!==agent.selected || !agent.detail)return;
    const entry=agent.detail.messages.find(m=>m.id===update.messageId);if(!entry)return;
    entry.modelMetadata=entry.modelMetadata || {};
    if(update.context)entry.modelMetadata.context=update.context;
    if(update.usage)entry.modelMetadata.usage=update.usage;
    renderConversation();
  });
  window.__KEEPER__.event.listen('agent-model-stream' ,event=>{
    const update=event.payload;if(update.sessionId!==agent.selected || !agent.detail.modelPending)return;
    const entry=agent.detail.messages.find(m=>m.id===update.messageId);
    if(entry && update.content.length>=entry.content.length){entry.content=update.content;renderConversation();}
  });
  window.__KEEPER__.event.listen('agent-model-finished',event=>{if(event.payload.sessionId===agent.selected)select(agent.selected);});
  function updateAgentRun(event){
    const run=event.payload;if(run.sessionId!==agent.selected || agent.detail.sessionId!==agent.selected)return;
    const index=agent.detail.runs.findIndex(r=>r.id===run.id);
    if(index<0)agent.detail.runs.push(run);else if((run.updatedAt || 0)>=(agent.detail.runs[index].updatedAt || 0))agent.detail.runs[index]=run;
    renderConversation();
  }
  window.__KEEPER__.event.listen('agent-tool-update',updateAgentRun);
  window.__KEEPER__.event.listen('agent-task-update',event=>{
    const task=event.payload;if(task.sessionId!==agent.selected || agent.detail.sessionId!==agent.selected)return;
    agent.detail.tasks ||= [];const index=agent.detail.tasks.findIndex(t=>t.responseId===task.responseId);
    if(index<0)agent.detail.tasks.push(task);else agent.detail.tasks[index]=task;
    renderConversation();
  });
  async function reviewAgentTool(run,approved){
    if(agent.detail.stopRequested || agent.stopping || agent.reviewing?.has(run.id))return;
    const session=agent.selected;agent.reviewing ||= new Set();agent.reviewing.add(run.id);
    for(const button of $('#agent-messages').querySelectorAll('[data-review-run="'+run.id+'"] button'))button.disabled=true;
    try{await invoke('agent_review_tool',{sessionId:session,runId:run.id,reviewHash:run.review.hash,approved});if(agent.selected===session)await select(session);}
    catch(error){notice(String(error));}finally{agent.reviewing.delete(run.id);if(agent.selected===session)renderConversation();}
  }
  setInterval(updateMessageTimes,30000);
  function runCard(run,openStates){
    const card=element('details','agent-run agent-compact-run'+(run.responseId?' agent-model-tool':''));card.dataset.runId=run.id;card.open=openStates?.get(run.id) || false;
    const header=element('summary','agent-run-summary');
    const icon=document.createElementNS('http://www.w3.org/2000/svg','svg');icon.setAttribute('viewBox','0 0 24 24');icon.setAttribute('fill','none');icon.setAttribute('stroke','currentColor');icon.setAttribute('stroke-width','1.7');icon.setAttribute('stroke-linecap','round');icon.setAttribute('stroke-linejoin','round');icon.setAttribute('aria-hidden','true');icon.classList.add('agent-run-icon');
    const path=document.createElementNS('http://www.w3.org/2000/svg','path');path.setAttribute('d',['run_command','ssh_execute'].includes(run.tool)?'m5 7 5 5-5 5m8 0h6':'M9 3h6l1 4 4 1v8l-4 1-1 4H9l-1-4-4-1V8l4-1Z');icon.append(path);
    const title=run.tool==='load_skill'?'加载项目技能 · '+(run.arguments?.name || ''):run.review?.kind==='mcp'?'MCP · '+run.review.server+' · '+run.review.tool:titleFor(run.tool);
    const toggle=element('span','agent-run-details-toggle',card.open?'收起':'详情');
    header.append(icon,element('strong','agent-run-title',title),element('span','run-state',statusNames[run.status] || run.status),toggle);card.append(header);
    card.ontoggle=()=>{toggle.textContent=card.open?'收起':'详情';};
    if(run.responseId){
      if(run.review?.approvalReason)card.append(element('div','agent-approval-audit',(run.review.approvalSource==='policy'?'自动批准 · ':'需要确认 · ')+run.review.approvalReason));
      if(run.review?.kind==='file'){
        card.append(element('div','agent-review-path',(run.review.newFile?'新建文件：':'修改文件：')+run.review.path));
        const diff=element('pre','agent-file-diff');
        for(const line of (run.review.diff || '新建空文件').split('\n'))diff.append(element('span',line.startsWith('@@')?'diff-hunk':line.startsWith('+')?'diff-add':line.startsWith('-')?'diff-delete':'',line+'\n'));
        card.append(diff);
      }else if(run.review?.kind==='command'){
        card.append(element('div','agent-review-path','执行目录：'+run.review.directory));
        card.append(element('div','agent-command-meta',run.review.shell+' · 超时 '+run.review.timeoutSeconds+' 秒'));
        card.append(element('pre','agent-command-preview',run.review.command));
        card.append(element('p','agent-command-scope',run.review.scope));
      }else if(run.review?.kind==='mcp'){card.append(element('div','agent-review-path','MCP 服务：'+run.review.server+' · '+(run.review.method?'操作：':'工具：')+run.review.tool),element('pre','agent-command-preview',JSON.stringify([run.review.command,...(run.review.launchArgs || [])],null,2)),element('pre','agent-tool-arguments',JSON.stringify(run.review.args,null,2)));}
      else card.append(element('pre','agent-tool-arguments',JSON.stringify(run.arguments,null,2)));
      if(run.status==='pending_approval' && run.review?.hash){
        const actions=element('div','agent-review-actions');actions.dataset.reviewRun=run.id;
        for(const [approved,label] of [[false,'拒绝'],[true,['command','mcp'].includes(run.review.kind)?'确认执行':'确认写入']]){
          const button=element('button',approved?'approve':'',label);button.type='button';button.disabled=!!agent.reviewing?.has(run.id) || !!agent.detail.stopRequested;
          button.onclick=()=>reviewAgentTool(run,approved);actions.append(button);
        }
        card.append(actions);
      }
      if(agent.runExperience)agent.runExperience.decorate(card,run);else if(run.output)card.append(element('pre','agent-tool-output',run.output));
      window.keeperMcpImagePreview?.(card,run,agent.selected,invoke);
      return card;
    }
    if(run.tool==='ssh_execute'){
      const target=run.arguments.target || {};
      card.append(element('pre','',`${target.username || ''}@${target.hostname || ''}:${target.port || 22}\n\n${run.arguments.command || ''}`));
    }
    if(agent.runExperience)agent.runExperience.decorate(card,run);else if(run.output)card.append(element('pre','',run.output));
    if(run.status==='pending_approval'){
      const actions=element('div','agent-run-actions');
      for(const [approved,label] of [[false,'取消'],[true,'确认执行此命令']]){
        const button=element('button',approved?'approve':'',label);button.type='button';
        button.onclick=async()=>{
          actions.querySelectorAll('button').forEach(b=>b.disabled=true);
          try{await invoke('agent_decide_tool',{runId:run.id,approved});await select(agent.selected);}
          catch(error){notice(String(error));actions.querySelectorAll('button').forEach(b=>b.disabled=false);}
        };actions.append(button);
      }
      card.append(actions);
    }
    return card;
  }
  async function runTool(tool,args={}){
    try{
      notice();const session=await ensureSession();await invoke('agent_run_tool',{sessionId:session,tool:tool.name,arguments:args});
      await bootstrap();await select(session);$('#agent-tool-panel').hidden=true;$('#agent-timeline').scrollTop=$('#agent-timeline').scrollHeight;
    }catch(error){notice(String(error));}
  }
  function chooseTool(tool){
    $('#agent-tool-panel').hidden=true;
    if(!tool.approval){runTool(tool);return;}
    const form=element('form','agent-run agent-run-form');form.dataset.sessionId=agent.selected || '';
    form.append(element('strong','','准备远程命令'));
    const label=element('label','','目标主机');const hosts=element('select');hosts.required=true;hosts.name='hostId';hosts.setAttribute('aria-label','目标主机');
    for(const host of agent.hosts){const option=element('option','',`${host.name} · ${host.hostname}`);option.value=host.id;hosts.append(option);}label.append(hosts);form.append(label);
    const commandLabel=element('label','','完整命令');const command=element('textarea');command.required=true;command.maxLength=8192;command.name='command';command.placeholder='例如：uname -a';command.setAttribute('aria-label','完整命令');commandLabel.append(command);form.append(commandLabel);
    form.append(element('small','','创建请求后会展示目标和命令，确认执行前不会连接主机。'));
    const submit=element('button','','创建待确认请求');submit.type='submit';submit.disabled=!agent.hosts.length;form.append(submit);
    if(!agent.hosts.length)form.append(element('p','','尚未配置主机，请到「设置 > 主机」添加。'));
    form.onsubmit=async event=>{event.preventDefault();submit.disabled=true;await runTool(tool,{hostId:hosts.value,command:command.value});submit.disabled=false;};
    $('#agent-welcome').hidden=true;$('#agent-messages').querySelector('.agent-run-form')?.remove();$('#agent-messages').append(form);command.focus();$('#agent-timeline').scrollTop=$('#agent-timeline').scrollHeight;
  }
  $('#agent-compose').onsubmit=async event=>{
    event.preventDefault();if(agent.sending || agent.detail.modelPending || agent.attachments?.busy())return;const content=$('#agent-input').value.trim(),attachmentIds=agent.attachments?.draftIds() || [];if(!content && !attachmentIds.length)return;
    const identity=agent.identityEpoch || 0;agent.sending=true;renderPending($('#agent-messages'));
    try{const session=await ensureSession();const submit=draftRevision=>invoke('agent_send_message',{sessionId:session,content,attachmentIds,draftRevision});if(agent.drafts)await agent.drafts.send(submit);else{await submit();if(agent.selected===session)$('#agent-input').value='';}if(identity!==(agent.identityEpoch || 0))return;await bootstrap();if(agent.selected===session){await select(session);$('#agent-timeline').scrollTop=$('#agent-timeline').scrollHeight;}notice();}
    catch(error){if(identity===(agent.identityEpoch || 0))notice(String(error));}finally{agent.sending=false;renderPending($('#agent-messages'));if(identity===(agent.identityEpoch || 0))$('#agent-input').focus();}
  };
  async function regenerateReply(messageId){
    if(!agent.selected || agent.detail.modelPending || agent.regenerating)return;
    const root=agent.detail.replyGroups?.find(g=>g.versionIds.includes(messageId))?.rootId || messageId,anchor=replyAnchor(root);
    const session=agent.selected;agent.regenerating=true;renderPending($('#agent-messages'));
    try{await invoke('agent_regenerate',{sessionId:session,messageId});await bootstrap();if(agent.selected===session){await select(session);restoreReplyAnchor(anchor);}}
    catch(error){notice(String(error));}finally{agent.regenerating=false;renderPending($('#agent-messages'));}
  }
  $('#agent-stop').onclick=async()=>{
    if(!agent.selected || !agent.detail.modelPending || agent.stopping)return;
    const session=agent.selected,messageId=agent.detail.streamingMessageId;agent.stopping=true;$('#agent-stop').disabled=true;
    try{await invoke('agent_stop_generation',{sessionId:session,messageId});if(agent.selected===session)await select(session);}catch(error){notice(String(error));}finally{agent.stopping=false;if(agent.selected===session)renderPending($('#agent-messages'));}
  };
  $('#agent-input').onkeydown=event=>{if(event.key==='Enter'&&!event.shiftKey&&!event.isComposing){event.preventDefault();$('#agent-compose').requestSubmit();}};
  async function newSession(){
    showConversation();
    try{clearTimeout(agent.timer);const session=await invoke('agent_new_session',{projectId:agent.project});await bootstrap();await select(session.id);$('#agent-input').focus();}catch(error){notice(String(error));}
  }
  $('#agent-tool-toggle').onclick=()=>{closeModelMenu();$('#agent-tool-panel').hidden=!$('#agent-tool-panel').hidden;};
  document.addEventListener('click',event=>{if(!event.target.closest('.agent-compose-area'))$('#agent-tool-panel').hidden=true;});
  function closeAccountMenu(restoreFocus=false){
    $('#agent-settings-menu').hidden=true;$('#agent-settings-toggle').setAttribute('aria-expanded','false');
    if(restoreFocus)$('#agent-settings-toggle').focus();
  }
  function openAccountMenu(){
    $('#agent-settings-menu').hidden=false;$('#agent-settings-toggle').setAttribute('aria-expanded','true');
    $('#agent-menu-account').focus();
  }
  function showConversation(){agent.projectBrowser?.hide();$('#keeper-account').classList.add('hidden');$('#keeper-settings').classList.add('hidden');$('#agent-main').classList.remove('hidden');}
  function openSettings(page){
    agent.projectBrowser?.hide();closeAccountMenu();$('#agent-main').classList.add('hidden');
    $('#keeper-account').classList.toggle('hidden',page!=='profile');
    $('#keeper-settings').classList.toggle('hidden',page!=='security');
    $(page==='profile'?'#account-page-title':'#settings-page-title').focus();if(page==='profile')loadMcp();
  }
  function updateAccount(login={}){
    const name=login.loggedIn?(login.username || login.nickname || '已登录'):'未登录';
    $('#agent-username').textContent=name;$('#agent-username').title=name;
    $('#agent-avatar').textContent=login.loggedIn?Array.from(name).slice(0,2).join('').toUpperCase():'灵';
    $('#agent-menu-logout').disabled=!login.loggedIn;$('#cis-model-refresh').disabled=!login.loggedIn;$('#cis-model-manage').disabled=!login.loggedIn;$('#cis-model-my-usage').disabled=!login.loggedIn;$('#cis-model-all-usage').disabled=!login.loggedIn;
    $('#agent-menu-logout').title=login.loggedIn?'退出当前账号':'当前未登录';
  }
  async function refreshAccountSessions(){
    agent.taskSummaryOpen?.clear();agent.projectBrowser?.reset();fullAccessFinish?.(false);if(llmRemote && !$('#llm-server-modal').classList.contains('hidden'))closeLlm();$('#cis-model-admin').hidden=true;$('#cis-model-admin-list').replaceChildren();$('#cis-model-usage').hidden=true;$('#cis-model-usage-content').replaceChildren();
    const epoch=agent.identityEpoch=(agent.identityEpoch || 0)+1;clearTimeout(agent.timer);++agent.generation;
    ++mcpLoadSequence;$('#mcp-list').replaceChildren();$('#mcp-status').textContent='';closeMcp();loadMcp();agent.guidance?.reset();agent.history?.reset();agent.attachments?.reset();agent.drafts?.reset();agent.runExperience?.reset();agent.titleEditor?.cancel(false);agent.editing=null;agent.selected=null;agent.sessions=[];agent.detail={messages:[],runs:[],tasks:[],modelPending:false};$('#agent-input').value='';renderSessions();renderProjects();renderConversation();closeSecurityMenu();
    try{await bootstrap();if(epoch!==agent.identityEpoch)return;const saved=localStorage.getItem(sessionStorageKey());if(saved && await select(saved))return;const session=agent.sessions[0]?.id;if(session)await select(session);}
    catch(error){if(epoch===agent.identityEpoch)notice(String(error));}
  }
  // Re-read persisted state after missed events; never replay a model/tool request.
  window.__KEEPER__.event.listen('keeper-transport-restored',async()=>{
    const epoch=agent.identityEpoch || 0,owner=agent.ownerKey,selected=agent.selected;
    try{
      const data=await invoke('agent_bootstrap');
      if(epoch!==(agent.identityEpoch || 0))return;
      if((data.accountKey || 'guest')!==owner){await refreshAccountSessions();}
      else{await bootstrap();if(epoch===(agent.identityEpoch || 0) && selected===agent.selected && selected)await select(selected);}
      const login=await invoke('get_login_status');
      window.dispatchEvent(new CustomEvent('keeper-login-state',{detail:login}));
    }catch(error){notice('连接已恢复，读取会话失败：'+String(error));}
  });
  let loginIdentity=null;
  window.addEventListener('keeper-login-state',event=>{updateAccount(event.detail);const login=event.detail || {},identity=JSON.stringify([login.loggedIn,login.serverUrl,login.userId,login.username]);if(loginIdentity!==null && loginIdentity!==identity)refreshAccountSessions();loginIdentity=identity;});
  window.__KEEPER__.event.listen('keeper-account-changed',()=>refreshAccountSessions());
  window.__KEEPER__.event.listen('keeper-model-catalog',()=>bootstrap().then(()=>agent.selected && select(agent.selected)).catch(error=>notice(String(error))));
  invoke('get_login_status').then(login=>{updateAccount(login);if(login.loggedIn)invoke('agent_cis_models_refresh').then(()=>bootstrap().then(()=>agent.selected && select(agent.selected))).catch(()=>{});if(loginIdentity===null)loginIdentity=JSON.stringify([login.loggedIn,login.serverUrl,login.userId,login.username]);}).catch(()=>updateAccount());
  $('#agent-account').onclick=()=>openSettings('profile');
  $('#agent-settings-toggle').onclick=()=>{if($('#agent-settings-menu').hidden)openAccountMenu();else closeAccountMenu(true);};
  $('#agent-settings-toggle').oncontextmenu=event=>{event.preventDefault();openAccountMenu();};
  $('#agent-menu-account').onclick=()=>openSettings('profile');
  $('#agent-menu-settings').onclick=()=>openSettings('security');
  $('#service-close-btn').addEventListener('click',()=>closeAccountMenu());
  $('#agent-menu-logout').onclick=async()=>{closeAccountMenu();await onLogoutClick();$('#agent-settings-toggle').focus();};
  $('#agent-settings-menu').onkeydown=event=>{
    const items=Array.from($('#agent-settings-menu').querySelectorAll('button:not(:disabled)'));const index=items.indexOf(document.activeElement);
    if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeAccountMenu(true);}
    else if(event.key==='Tab')closeAccountMenu();
    else if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){
      event.preventDefault();const next=event.key==='Home'?0:event.key==='End'?items.length-1:(index+(event.key==='ArrowDown'?1:-1)+items.length)%items.length;items[next]?.focus();
    }
  };
  document.addEventListener('click',event=>{if(!event.target.closest('.agent-sidebar-foot'))closeAccountMenu();});
  $('#agent-settings-toggle').onkeydown=event=>{if(event.key==='ArrowDown'){event.preventDefault();openAccountMenu();}};
  $('#settings-back').onclick=()=>{showConversation();$('#agent-account').focus();bootstrap().catch(error=>notice(String(error)));};
  $('#account-back').onclick=$('#settings-back').onclick;
  document.addEventListener('keydown',event=>{if(!event.defaultPrevented && event.key==='Escape'){$('#agent-tool-panel').hidden=true;if(!document.querySelector('.modal:not(.hidden),.dialog:not(.hidden)')){showConversation();}}});
  // Move existing controls; their original handlers and state remain intact.
  const account=$('.account-actions');if(account)$('#settings-account-actions').append(account);
  $('#account-update-actions').append($('#app-update-btn'));
  // Keep the existing proxy updater bound, without showing its status row.
  $('#proxy-indicator').hidden=true;
  const rootNav=element('button','navlink');rootNav.id='nav-root-certificate';rootNav.type='button';
  const rootIcon=document.createElementNS('http://www.w3.org/2000/svg','svg');rootIcon.setAttribute('viewBox','0 0 24 24');rootIcon.setAttribute('class','agent-menu-icon');rootIcon.setAttribute('fill','none');rootIcon.setAttribute('stroke','currentColor');rootIcon.setAttribute('stroke-width','1.7');rootIcon.setAttribute('aria-hidden','true');
  const shield=document.createElementNS('http://www.w3.org/2000/svg','path');shield.setAttribute('d','M12 3 3 7v5c0 5 9 9 9 9s9-4 9-9V7ZM8 12l3 3 5-6');rootIcon.append(shield);rootNav.append(rootIcon,document.createTextNode('根证书'));$('.topnav').append(rootNav);
  const rootPanel=element('section','settings-root-panel hidden');rootPanel.setAttribute('aria-label','根证书');
  rootPanel.append(element('h2','','根证书'),element('p','','将苍灵根证书安装到当前电脑的系统信任库。安装时需要系统管理员授权。'));
  const rootStatus=element('p');rootStatus.id='settings-root-status';rootStatus.setAttribute('role','status');rootStatus.textContent='正在读取证书状态…';
  rootPanel.append(rootStatus,$('#install-root-cert-btn'));$('#maintenance-app').append(rootPanel);
  rootNav.onclick=()=>{
    $('#maintenance-app').classList.add('root-certificate-view');rootPanel.classList.remove('hidden');
    for(const button of document.querySelectorAll('.topnav .navlink'))button.classList.toggle('active',button===rootNav);
    loadRootCertificateStatus();
  };
  for(const button of document.querySelectorAll('.topnav .navlink'))if(button!==rootNav)button.addEventListener('click',()=>{$('#maintenance-app').classList.remove('root-certificate-view');rootPanel.classList.add('hidden');rootNav.classList.remove('active');});
  function clearSelected(){
    agent.drafts?.reset();$('#agent-input').value='';
    clearTimeout(agent.timer);++agent.generation;agent.titleEditor?.cancel(false);agent.editing=null;agent.selected=null;agent.detail={messages:[],runs:[],tasks:[],modelPending:false};localStorage.removeItem(sessionStorageKey());renderSessions();renderConversation();
  }
  agent.plugins=window.createKeeperPlugins?.({agent,invoke,renderMcp,status:message=>{$('#mcp-status').textContent=message;}});
  agent.runExperience=window.createKeeperRunExperience?.({agent,notice});
  agent.drafts=window.createKeeperDrafts?.({agent,invoke,notice,ensureSession});
  agent.history=window.createKeeperHistory?.({agent,invoke,element,notice,bootstrap,select,showConversation,renderProjects,renderSessions,renderConversation,clearSelected});
  agent.guidance=window.createKeeperGuidance?.({agent,invoke,element,notice});
  agent.attachments=window.createKeeperAttachments?.({agent,invoke,element,notice,ensureSession,select,renderPending});
  bootstrap().then(async()=>{
    const saved=(localStorage.getItem(sessionStorageKey()) || localStorage.getItem('keeper-agent-session'));
    if(saved && await select(saved))return;
    const selected=agent.sessions.find(s=>s.projectId===agent.project)?.id;
    if(selected)await select(selected);
  }).catch(error=>notice(String(error)));
})();
