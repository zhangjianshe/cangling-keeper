/* Account-owned, cross-project agent presets. All labels use textContent, never HTML from configuration. */
(() => {
 'use strict';
 window.createKeeperProfiles=({agent,invoke,element,notice,select,ensureSession})=>{
  const header=document.querySelector('.agent-header-tools');
  const picker=element('select','btn');picker.id='agent-profile-picker';picker.setAttribute('aria-label','会话智能体');
  const manage=element('button','btn','管理智能体');manage.type='button';header.prepend(picker,manage);
  const modal=element('div','modal hidden');modal.id='agent-profiles-modal';const box=element('div','modal-box');box.setAttribute('role','dialog');box.setAttribute('aria-modal','true');box.setAttribute('aria-labelledby','agent-profiles-title');
  const head=element('header','llm-dialog-head'),title=element('h2','','智能体配置');title.id='agent-profiles-title';const close=element('button','modal-close','×');close.type='button';close.setAttribute('aria-label','关闭');head.append(title,close);
  const form=element('form'),fields=element('div','llm-dialog-fields'),list=element('div','llm-server-list'),add=element('button','btn','＋ 新建智能体');add.type='button';
  const info=element('p','hint','智能体在当前账户跨项目使用。共享和专属技能保存在本机技能库；项目技能仍从当前项目目录读取，其他项目的关联不生效。技能按需加载，不改变工具审批。');
  const name=element('input'),prompt=element('textarea'),model=element('select');name.required=true;name.maxLength=80;prompt.rows=7;prompt.maxLength=16384;model.required=true;
  name.id='profile-name';prompt.id='profile-prompt';model.id='profile-model';
  function field(text,input){const label=element('label','',text);label.append(input);return label;}
  const skills=element('fieldset','profile-choices'),mcp=element('fieldset','profile-choices'),status=element('p','hint');status.setAttribute('role','status');
  const actions=element('div','modal-actions'),cancel=element('button','btn','取消'),save=element('button','btn primary','保存');cancel.type='button';save.type='submit';actions.append(cancel,save);
  fields.append(info,list,add,field('名称',name),field('系统提示词',prompt),field('模型',model),skills,mcp,status);form.append(fields,actions);box.append(head,form);modal.append(box);document.body.append(modal);
  const skillActions=element('div','profile-skill-actions');
  const shared=element('button','btn','＋ 共享技能'),privateSkill=element('button','btn','＋ 专属技能');shared.type=privateSkill.type='button';skillActions.append(shared,privateSkill);skills.after(skillActions);
  let current='',options=null,session='',epoch=0,sequence=0;
  const library=window.createKeeperSkillLibrary({invoke,element,valid:()=>valid(),saved:async entry=>{
   const selected=[...skills.querySelectorAll('input:checked')].map(x=>x.value);if(!selected.includes(entry.ref))selected.push(entry.ref);
   const fresh=await invoke('agent_profile_options',{sessionId:session});if(!valid())return;options=fresh;skillChoices(selected);status.textContent='技能已保存。点击下方保存，将技能关联到此智能体。';
  }});
  shared.onclick=()=>{if(options)library.open({sessionId:session,scope:'account'});};
  privateSkill.onclick=()=>{if(current)library.open({sessionId:session,scope:'agent',profileId:current});};
  function skillChoices(selected){
   const labels={account:'账户共享',agent:'智能体专属',project:'当前项目'};
   const items=options.skills.filter(x=>x.scope!=='agent'||x.profileId===current);
   choices(skills,'关联 Skills（勾选已有技能）',items.map(x=>({id:x.ref||x.name,name:(labels[x.scope]?'['+labels[x.scope]+'] ':'')+x.name,description:x.description})),selected);
   for(const row of skills.querySelectorAll('.profile-choice')){const ref=row.querySelector('input').value,entry=items.find(x=>(x.ref||x.name)===ref);if(entry&&entry.scope!=='project'&&entry.id){const edit=element('button','btn','编辑');edit.type='button';edit.onclick=e=>{e.preventDefault();library.open({sessionId:session,id:entry.id});};row.append(edit);}}
   privateSkill.disabled=!current;privateSkill.title=current?'为此智能体创建专属技能':'先保存智能体，再创建专属技能';
  }
  const valid=()=>session===agent.selected&&epoch===(agent.identityEpoch||0)&&!modal.classList.contains('hidden');
  function hide(){library.hide();sequence++;modal.classList.add('hidden');options=null;current='';name.value='';prompt.value='';model.replaceChildren();list.replaceChildren();skills.replaceChildren();mcp.replaceChildren();manage.focus();}
  close.onclick=cancel.onclick=hide;modal.addEventListener('keydown',e=>{if(e.key==='Escape'){e.preventDefault();hide();}if(e.key==='Tab'){const all=[...box.querySelectorAll('button,input,textarea,select')].filter(x=>!x.disabled);const first=all[0],last=all.at(-1);if(e.shiftKey&&document.activeElement===first){e.preventDefault();last.focus();}else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first.focus();}}});
  function choices(target,label,items,chosen){target.replaceChildren(element('legend','',label));for(const item of items){const input=element('input');input.type='checkbox';input.value=item.id;input.checked=chosen.includes(item.id);const line=field(item.name,input);line.className='profile-choice';line.title=item.description||item.name;target.append(line);}for(const id of chosen.filter(id=>!items.some(x=>x.id===id))){const input=element('input');input.type='checkbox';input.value=id;input.checked=true;const line=field((id.startsWith('project:')?'其他项目（当前不适用）：':'不可用：')+id,input);line.className='profile-choice';target.append(line);}if(!items.length&&!chosen.length)target.append(element('p','hint','暂无可用项'));}
  function edit(p={}){current=p.id||'';name.value=p.name||'';prompt.value=p.systemPrompt||'';model.replaceChildren(element('option','','请选择模型'));model.firstChild.value='';for(const m of options.models.filter(x=>x.enabled&&x.hasApiKey)){const o=element('option','',m.name+' · '+m.model);o.value=m.id;model.append(o);}model.value=p.modelId||'';
   skillChoices(p.skills||[]);
   choices(mcp,'关联 MCP 工具',options.mcpTools.map(x=>({id:x.function.name,name:x.function.description.slice(0,150)})),p.mcpTools||[]);status.textContent='';name.focus();}
  function rows(){list.replaceChildren();for(const p of options.profiles){const row=element('div','llm-server-row'),label=element('strong','',p.name),buttons=element('div','llm-server-actions'),editButton=element('button','btn','编辑'),remove=element('button','btn','删除');editButton.type=remove.type='button';editButton.onclick=()=>edit(p);remove.onclick=async()=>{if(!await window.uiConfirm('删除智能体“'+p.name+'”？历史回复快照保留，使用中的智能体不能删除。','删除智能体'))return;if(!valid())return;try{await invoke('agent_profile_delete',{sessionId:session,id:p.id});if(valid()){await open();await select(session);}}catch(e){if(valid())status.textContent=String(e);}};buttons.append(editButton,remove);row.append(label,buttons);list.append(row);}}
  async function open(){await ensureSession();session=agent.selected;epoch=agent.identityEpoch||0;const ticket=++sequence;modal.classList.remove('hidden');status.textContent='正在读取配置…';save.disabled=true;try{const data=await invoke('agent_profile_options',{sessionId:session});if(ticket!==sequence||!valid())return;options=data;rows();edit(data.selected||{});save.disabled=false;}catch(e){if(ticket===sequence&&valid())status.textContent=String(e);}}
  manage.onclick=()=>open().catch(e=>notice(String(e)));add.onclick=()=>{if(options)edit();};
  form.onsubmit=async e=>{e.preventDefault();if(!valid()||!options)return;save.disabled=true;const target=session;try{await invoke('agent_profile_save',{sessionId:target,id:current,name:name.value,systemPrompt:prompt.value,modelId:model.value,skills:[...skills.querySelectorAll('input:checked')].map(x=>x.value),mcpTools:[...mcp.querySelectorAll('input:checked')].map(x=>x.value)});if(valid()){hide();await select(target);notice('智能体已保存，可在会话顶部选择；正在执行的任务仍使用原配置。');}}catch(e){if(valid())status.textContent=String(e);}finally{save.disabled=false;}};
  picker.onchange=async()=>{const target=agent.selected,identity=agent.identityEpoch;picker.disabled=true;try{await invoke('agent_profile_select',{sessionId:target,id:picker.value});if(target===agent.selected&&identity===agent.identityEpoch)await select(target);}catch(e){if(target===agent.selected&&identity===agent.identityEpoch){update();notice(String(e));}}};
  function update(){if(!modal.classList.contains('hidden')&&!valid())hide();picker.replaceChildren(element('option','','通用助手'));picker.firstChild.value='';for(const p of agent.detail.agentProfiles||[]){const o=element('option','',p.name);o.value=p.id;picker.append(o);}picker.value=agent.detail.agentProfile?.id||'';picker.disabled=!agent.selected||!!agent.detail.modelPending;}
  return {open,update,reset(){hide();picker.replaceChildren();picker.disabled=true;}};
 };
})();
