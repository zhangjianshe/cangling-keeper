/* Inspect project instructions and lazily preview project-local Agent Skills. */
(() => {
  'use strict';
  window.createKeeperGuidance=({agent,invoke,element,notice})=>{
    const $=s=>document.querySelector(s);let request=0,session=null,previousFocus=null,snapshot=null,selectedSkill=null,page=null,text='',busy=false,tab='rules';
    const trigger=element('button','guidance-trigger','项目指南');trigger.id='agent-guidance-open';trigger.type='button';trigger.disabled=true;trigger.setAttribute('aria-haspopup','dialog');$('.agent-header-tools').prepend(trigger);
    const modal=element('div','history-modal guidance-modal');modal.id='project-guidance-modal';modal.hidden=true;
    const panel=element('section','history-panel');panel.setAttribute('role','dialog');panel.setAttribute('aria-modal','true');panel.setAttribute('aria-labelledby','project-guidance-title');
    const header=element('header','history-header'),title=element('h2','','项目指南');title.id='project-guidance-title';const closeButton=element('button','history-close','×');closeButton.type='button';closeButton.setAttribute('aria-label','关闭项目指南');header.append(title,closeButton);
    const hint=element('p','guidance-hint'),tabs=element('div','history-tabs');tabs.setAttribute('role','group');tabs.setAttribute('aria-label','项目指南内容');
    for(const [id,label] of [['rules','项目规则'],['skills','技能']]){const b=element('button','',label);b.type='button';b.dataset.tab=id;b.onclick=()=>{tab=id;render();};tabs.append(b);}
    const status=element('p','history-status');status.id='guidance-status';status.setAttribute('role','status');
    const warnings=element('div','guidance-warnings');warnings.id='guidance-warnings';
    const content=element('div','guidance-content');content.id='guidance-content';
    const footer=element('footer','history-footer guidance-footer'),refresh=element('button','btn','重新扫描');refresh.id='guidance-refresh';refresh.type='button';footer.append(element('span','','生成时会将项目规则及按需加载的技能提供给当前模型。'),refresh);
    panel.append(header,hint,tabs,status,warnings,content,footer);modal.append(panel);document.body.append(modal);
    function button(label,handler,cls='btn'){const b=element('button',cls,label);b.type='button';b.onclick=handler;return b;}
    async function editDocument(path,missing=false){
      const project=agent.projects.find(p=>p.id===(agent.detail.session?.projectId || agent.project)),epoch=agent.identityEpoch,selected=agent.selected;if(!project||busy)return;
      busy=true;render();
      try{if(missing)await invoke('agent_project_entry_create',{projectId:project.id,parent:'.',name:path,kind:'file'});if(epoch!==agent.identityEpoch||selected!==agent.selected||modal.hidden)return;close();await agent.projectBrowser?.openFile(project,path);}
      catch(error){status.textContent='打开失败：'+String(error);}finally{busy=false;if(!modal.hidden)render();}
    }
    function inactive(){return !!(agent.detail.session?.archivedAt || agent.detail.session?.deletedAt || agent.detail.historyPreview);}
    function render(){
      for(const b of tabs.querySelectorAll('button'))b.setAttribute('aria-pressed',String(b.dataset.tab===tab));
      content.replaceChildren();warnings.replaceChildren();refresh.disabled=busy;
      if(!snapshot)return;
      for(const message of snapshot.warnings || [])warnings.append(element('p','',message));
      if(snapshot.catalogTruncated)warnings.append(element('p','','技能数量或目录大小超出扫描上限，仅显示本次发现的技能。'));
      if(tab==='rules'){
        const rules=snapshot.rules || {};const edit=button(rules.status==='missing'?'创建并编辑 AGENTS.md':'编辑 AGENTS.md',()=>editDocument('AGENTS.md',rules.status==='missing'));edit.disabled=busy;content.append(edit);content.append(element('h3','','AGENTS.md'),element('p','hint','适用于当前项目；本轮仅自动加载根目录规则。'));
        if(rules.status==='missing')content.append(element('p','guidance-empty','项目根目录尚未创建 AGENTS.md。可以在其中填写项目介绍、构建命令和代码约定。'));
        else if(typeof rules.content==='string')content.append(element('pre','guidance-source',rules.content || '（空文件）'));
        else content.append(element('p','guidance-empty',rules.status==='too_large'?'规则文件较长，未自动全文载入。智能体需要通过 read_file 分段读取。':'规则文件无法读取，请检查文件格式或路径。'));
      }else{
        content.append(element('p','hint','技能目录：.agents/skills/<名称>/SKILL.md；先提供名称和用途，正文通过 load_skill 按需读取。'));
        const skills=element('div','guidance-skills');skills.id='guidance-skills';content.append(skills);
        for(const skill of snapshot.skills || []){
          const row=element('section','guidance-skill');row.dataset.skillName=skill.name;
          const info=element('div','guidance-skill-info');info.append(element('strong','',skill.name),element('p','',skill.description));
          const actions=element('div','guidance-skill-actions');const view=button(selectedSkill?.name===skill.name?'已展开':'查看',()=>loadSkill(skill));view.disabled=busy;const use=button('用于任务',()=>useSkill(skill));use.classList.add('guidance-use');use.disabled=inactive() || busy;const edit=button('编辑',()=>editDocument(skill.path));edit.disabled=busy;actions.append(view,edit,use);row.append(info,actions);
          if(selectedSkill?.name===skill.name){const preview=element('div','guidance-skill-preview');preview.append(element('small','',skill.path),element('pre','guidance-source',text || (busy?'正在读取…':'（空技能）')));if(Number.isInteger(page?.nextOffset)){const more=button(busy?'正在读取…':'继续读取',()=>loadSkill(skill,true),'history-load-more');more.disabled=busy;preview.append(more);}row.append(preview);}skills.append(row);
        }
        if(!snapshot.skills?.length)skills.append(element('p','guidance-empty','暂无可用技能。在 .agents/skills 中创建技能目录，并在 SKILL.md 中填写 name、description 和操作说明。'));
      }
    }
    async function scan(){
      if(!agent.selected)return;const token=++request,epoch=agent.identityEpoch || 0,selected=agent.selected;session=selected;busy=true;snapshot=null;page=null;selectedSkill=null;text='';status.textContent='正在读取项目规则和技能目录…';render();
      try{const data=await invoke('agent_project_guidance',{sessionId:selected});if(token!==request || epoch!==(agent.identityEpoch || 0) || selected!==agent.selected)return;snapshot=data;const project=agent.projects.find(p=>p.id===(agent.detail.session?.projectId || agent.project));hint.textContent=project?.name || '当前会话项目';status.textContent='发现 '+(data.skills?.length || 0)+' 个技能';}
      catch(error){if(token===request)status.textContent='读取失败：'+String(error);}
      finally{if(token===request){busy=false;render();}}
    }
    async function loadSkill(skill,append=false){
      if(busy || session!==agent.selected)return;const token=++request,epoch=agent.identityEpoch || 0,selected=session;busy=true;selectedSkill=skill;if(!append){text='';page=null;}status.textContent='正在读取技能 '+skill.name+'…';render();
      try{const data=await invoke('agent_skill_content',{sessionId:selected,name:skill.name,offset:append?page.nextOffset:0,expectedSha256:append?page.sha256:skill.sha256});if(token!==request || epoch!==(agent.identityEpoch || 0) || selected!==agent.selected)return;page=data;text+=data.instructions || '';status.textContent=Number.isInteger(data.nextOffset)?'已加载部分内容，可继续读取':'技能内容已加载';}
      catch(error){if(token===request)status.textContent='读取失败：'+String(error);}
      finally{if(token===request){busy=false;render();}}
    }
    function useSkill(skill){if(inactive())return;const input=$('#agent-input'),value='$'+skill.name;if(!input.value.split(/\s+/).includes(value))input.value=input.value+(input.value && !input.value.endsWith('\n')?'\n':'')+value;input.dispatchEvent(new Event('input',{bubbles:true}));close();input.focus();}
    function open(){if(!agent.selected)return;previousFocus=document.activeElement;modal.hidden=false;scan();refresh.focus();}
    function close(){request++;modal.hidden=true;busy=false;if(previousFocus?.isConnected)previousFocus.focus();previousFocus=null;}
    function reset(){close();session=null;snapshot=null;selectedSkill=null;page=null;text='';status.textContent='';content.replaceChildren();warnings.replaceChildren();hint.textContent='';trigger.disabled=true;}
    function update(){trigger.disabled=!agent.selected;if(session && session!==agent.selected)reset();trigger.disabled=!agent.selected;for(const use of content.querySelectorAll('.guidance-use'))use.disabled=inactive() || busy;}
    trigger.onclick=open;closeButton.onclick=close;refresh.onclick=scan;modal.onclick=event=>{if(event.target===modal)close();};
    modal.onkeydown=event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();close();}else if(event.key==='Tab'){const nodes=[...panel.querySelectorAll('button:not(:disabled)')],first=nodes[0],last=nodes.at(-1);if(event.shiftKey && document.activeElement===first){event.preventDefault();last.focus();}else if(!event.shiftKey && document.activeElement===last){event.preventDefault();first.focus();}}};
    return {update,reset,open};
  };
})();
