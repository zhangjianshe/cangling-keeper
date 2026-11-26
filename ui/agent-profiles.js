/* Account-owned, cross-project agent presets. All labels use textContent, never HTML from configuration. */
(() => {
 'use strict';
 window.createKeeperProfileIcon=(source='',size='')=>{
  const holder=document.createElement('span');holder.className='profile-avatar'+(size?' '+size:'');holder.setAttribute('aria-hidden','true');
  const fallback=()=>{holder.replaceChildren();const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 24 24');const p=document.createElementNS(svg.namespaceURI,'path');p.setAttribute('d','M12 3v3M9 3h3M5 6h14v14H5ZM2 10v6M22 10v6M8 11h1M15 11h1M9 16h6');svg.append(p);holder.append(svg);};
  if(typeof source==='string'&&source.length<=90000&&/^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/.test(source)){const img=document.createElement('img');img.alt='';img.src=source;img.onerror=fallback;holder.append(img);}else fallback();return holder;
 };
 window.prepareKeeperProfileIcon=async file=>{
  if(!['image/png','image/jpeg','image/webp'].includes(file.type)||file.size>4*1024*1024)throw Error('请选择不超过4 MiB的 PNG、JPEG 或 WebP 图片');
  const url=URL.createObjectURL(file);
  try{const image=new Image();await new Promise((resolve,reject)=>{image.onload=resolve;image.onerror=()=>reject(Error('无法读取图片'));image.src=url;});
   if(!image.naturalWidth||!image.naturalHeight||image.naturalWidth>8192||image.naturalHeight>8192||image.naturalWidth*image.naturalHeight>24000000)throw Error('图片尺寸过大，请先缩小图片');
   const canvas=document.createElement('canvas');canvas.width=canvas.height=128;const ctx=canvas.getContext('2d');const scale=Math.min(128/image.naturalWidth,128/image.naturalHeight),width=image.naturalWidth*scale,height=image.naturalHeight*scale;ctx.drawImage(image,(128-width)/2,(128-height)/2,width,height);const data=canvas.toDataURL('image/png');if(data.length>87406)throw Error('图标过大，请使用更简单的图片');return data;
  }finally{URL.revokeObjectURL(url);}
 };
 window.createKeeperProfiles=({agent,invoke,element,notice,select,ensureSession,showConversation,closeAccountMenu=()=>{}})=>{
  const modelControl=document.querySelector('.agent-compose-toolbar .agent-model-control');
  const picker=element('button','btn');picker.type='button';picker.id='agent-profile-picker';picker.setAttribute('aria-label','会话智能体');
  const pickerArrow=document.createElementNS('http://www.w3.org/2000/svg','svg');pickerArrow.setAttribute('viewBox','0 0 16 16');pickerArrow.setAttribute('class','profile-picker-chevron');pickerArrow.setAttribute('aria-hidden','true');const arrowPath=document.createElementNS(pickerArrow.namespaceURI,'path');arrowPath.setAttribute('d','m4 6 4 4 4-4');pickerArrow.append(arrowPath);
  const manage=element('button');manage.id='agent-menu-profiles';manage.type='button';manage.setAttribute('role','menuitem');
  const icon=document.createElementNS('http://www.w3.org/2000/svg','svg');for(const [key,value] of Object.entries({class:'agent-menu-icon',viewBox:'0 0 24 24',fill:'none',stroke:'currentColor','stroke-width':'1.7','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true'}))icon.setAttribute(key,value);
  const shape=document.createElementNS(icon.namespaceURI,'path');shape.setAttribute('d','M12 3v3M9 3h3M5 6h14v14H5ZM2 10v6M22 10v6M8 11h1M15 11h1M9 16h6');icon.append(shape);manage.append(icon,element('span','','管理智能体'));document.querySelector('#agent-menu-settings')?.after(manage);const pickerWrap=element('span','profile-picker-wrap');pickerWrap.append(picker);modelControl.before(pickerWrap);
  picker.setAttribute('aria-haspopup','dialog');picker.setAttribute('aria-expanded','false');picker.setAttribute('aria-controls','agent-profile-popup');
  const popup=element('div','agent-profile-popup');popup.id='agent-profile-popup';popup.hidden=true;popup.setAttribute('role','dialog');popup.setAttribute('aria-label','选择智能体');
  const popupHead=element('div','agent-profile-popup-head'),popupTitle=element('strong','','选择智能体'),configure=element('button','btn','配置智能体');configure.type='button';popupHead.append(popupTitle,configure);
  const filter=element('input');filter.type='search';filter.placeholder='搜索智能体';filter.setAttribute('aria-label','搜索智能体');const choicesList=element('div','agent-profile-options');popup.append(popupHead,filter,choicesList);pickerWrap.append(popup);
  let popupSession=null,popupEpoch=null;
  function closePopup(focus=false){popup.hidden=true;picker.setAttribute('aria-expanded','false');if(focus)picker.focus();}
  function renderChoices(){
    choicesList.replaceChildren();const query=filter.value.trim().toLocaleLowerCase();
    for(const p of [{id:'',name:'通用助手'},...(agent.detail.agentProfiles||[])].filter(p=>p.name.toLocaleLowerCase().includes(query))){
      const item=element('button','agent-profile-option');item.type='button';item.setAttribute('aria-pressed',String(p.id===(agent.detail.agentProfile?.id||'')));item.append(element('span','',p.name),element('span','',p.id===(agent.detail.agentProfile?.id||'')?'✓':''));
      item.onclick=()=>{if(popupSession!==agent.selected||popupEpoch!==agent.identityEpoch||agent.detail.modelPending)return;picker.value=p.id;closePopup(true);picker.onchange();};choicesList.append(item);
    }
    if(!choicesList.children.length)choicesList.append(element('p','hint','没有匹配的智能体'));
  }
  picker.onclick=()=>{if(picker.disabled)return;if(!popup.hidden){closePopup(true);return;}popupSession=agent.selected;popupEpoch=agent.identityEpoch;filter.value='';renderChoices();popup.hidden=false;picker.setAttribute('aria-expanded','true');filter.focus();};
  filter.oninput=renderChoices;
  configure.onclick=()=>{closePopup();manage.click();};
  document.addEventListener('click',e=>{if(!pickerWrap.contains(e.target))closePopup();});
  popup.onkeydown=e=>{if(e.key==='Escape'){e.preventDefault();e.stopPropagation();closePopup(true);}else if(e.key==='ArrowDown'||e.key==='ArrowUp'){const items=[filter,...choicesList.querySelectorAll('button')],i=items.indexOf(document.activeElement);e.preventDefault();items[(i+(e.key==='ArrowDown'?1:-1)+items.length)%items.length]?.focus();}};

  const modal=element('section','profiles-page hidden');modal.id='agent-profiles-page';modal.setAttribute('aria-label','智能体管理');
  const head=element('header','keeper-settings-header'),title=element('h1','','智能体管理'),close=element('button','agent-nav-button','← 返回会话');close.type='button';head.append(close,title);
  const layout=element('div','profiles-layout'),nav=element('nav','profiles-navigation'),search=element('input'),list=element('div','profiles-list'),add=element('button','btn','＋ 新建智能体');nav.setAttribute('aria-label','智能体列表');search.type='search';search.placeholder='搜索智能体';search.setAttribute('aria-label','搜索智能体');add.type='button';nav.append(add,search,list);
  const box=element('div','profiles-content'),form=element('form','profiles-form'),fields=element('div','profiles-fields');
  const info=element('p','hint','智能体属于当前账户，可跨项目使用。按需关联技能与 MCP 工具，原有审批规则仍然有效。');
  const name=element('input'),prompt=element('textarea'),model=element('select');name.required=true;name.maxLength=80;prompt.rows=7;prompt.maxLength=16384;model.required=true;
  name.id='profile-name';prompt.id='profile-prompt';model.id='profile-model';
  function field(text,input){const label=element('label','',text);label.append(input);return label;}
  const skills=element('fieldset','profile-choices'),mcp=element('fieldset','profile-choices'),status=element('p','hint');status.setAttribute('role','status');
  const actions=element('div','profiles-actions'),cancel=element('button','btn','重置'),save=element('button','btn primary','保存');cancel.type='button';save.type='submit';actions.append(cancel,save);
  const editorTitle=element('h2','','新建智能体'),remove=element('button','btn danger','删除智能体');remove.type='button';
  const basic=element('section','profiles-section'),skillSection=element('section','profiles-section'),mcpSection=element('section','profiles-section');
  const iconRow=element('div','profile-icon-setting'),iconPreview=element('span'),iconUpload=element('button','btn','上传图标'),iconClear=element('button','btn','恢复默认'),iconFile=element('input');iconUpload.type=iconClear.type='button';iconFile.type='file';iconFile.accept='image/png,image/jpeg,image/webp';iconFile.hidden=true;iconFile.setAttribute('aria-label','上传智能体图标');iconRow.append(iconPreview,iconUpload,iconClear,iconFile);let draftIcon='',iconTicket=0,iconBusy=false;
  function showIcon(){iconPreview.replaceChildren(window.createKeeperProfileIcon(draftIcon,'profile-avatar-large'));iconClear.disabled=!draftIcon;}
  iconUpload.onclick=()=>iconFile.click();iconClear.onclick=()=>{iconTicket++;iconBusy=false;iconUpload.disabled=false;draftIcon='';iconFile.value='';showIcon();};
  iconFile.onchange=async()=>{const file=iconFile.files[0];if(!file)return;const token=++iconTicket;iconBusy=true;iconUpload.disabled=true;try{const data=await window.prepareKeeperProfileIcon(file);if(token!==iconTicket||!valid())return;draftIcon=data;showIcon();status.textContent='图标已准备，保存后生效。';}catch(e){if(token===iconTicket&&valid())status.textContent=String(e);}finally{if(token===iconTicket){iconBusy=false;iconUpload.disabled=false;iconFile.value='';}}};
  basic.append(editorTitle,info,iconRow,field('名称',name),field('模型',model),field('系统提示词',prompt));
  skillSection.append(element('h2','','技能'),element('p','hint','共享技能可重复使用；专属技能跟随智能体；项目技能只在所属项目生效。'),skills);
  mcpSection.append(element('h2','','MCP 工具'),element('p','hint','仅提供勾选的工具，跨项目后仍受 MCP 自身作用域限制。'),mcp);
  const tabs=element('div','profiles-tabs');tabs.setAttribute('role','tablist');tabs.setAttribute('aria-label','智能体配置分类');
  const panels=[basic,skillSection,mcpSection],tabButtons=[];
  function activateTab(index,focus=false){panels.forEach((panel,i)=>{panel.hidden=i!==index;tabButtons[i].setAttribute('aria-selected',String(i===index));tabButtons[i].tabIndex=i===index?0:-1;});fields.scrollTop=0;if(focus)tabButtons[index].focus();}
  ['基本配置','技能','MCP 工具'].forEach((label,index)=>{const button=element('button','profiles-tab',label);button.type='button';button.id='profile-tab-'+index;button.setAttribute('role','tab');button.setAttribute('aria-controls','profile-panel-'+index);panels[index].id='profile-panel-'+index;panels[index].setAttribute('role','tabpanel');panels[index].setAttribute('aria-labelledby',button.id);button.onclick=()=>activateTab(index);button.onkeydown=e=>{let next;if(e.key==='ArrowRight')next=(index+1)%3;else if(e.key==='ArrowLeft')next=(index+2)%3;else if(e.key==='Home')next=0;else if(e.key==='End')next=2;else return;e.preventDefault();activateTab(next,true);};tabButtons.push(button);tabs.append(button);});
  activateTab(0);
  // Reveal required basic fields before the browser focuses invalid controls.
  form.addEventListener('invalid',e=>{const index=panels.findIndex(panel=>panel.contains(e.target));if(index>=0)activateTab(index);},true);
  fields.append(basic,skillSection,mcpSection);actions.prepend(remove,status);form.append(tabs,fields,actions);box.append(form);layout.append(nav,box);modal.append(head,layout);document.querySelector('#agent-main').after(modal);
  const skillActions=element('div','profile-skill-actions');
  const shared=element('button','btn','＋ 共享技能'),privateSkill=element('button','btn','＋ 专属技能');shared.type=privateSkill.type='button';skillActions.append(shared,privateSkill);const skillHeading=element('div','profile-section-heading');skillHeading.append(skillSection.querySelector('h2'),skillActions);skillSection.prepend(skillHeading);
  let current='',options=null,session='',epoch=0,sequence=0;
  const library=window.createKeeperSkillLibrary({invoke,element,host:box,visibility:visible=>{form.inert=visible;nav.inert=visible;},valid:()=>valid(),saved:async entry=>{
   const selected=[...skills.querySelectorAll('input:checked')].map(x=>x.value).filter(value=>!value.startsWith('system:'));if(!selected.includes(entry.ref))selected.push(entry.ref);
   const fresh=await invoke('agent_profile_options',{sessionId:session});if(!valid())return;options=fresh;skillChoices(selected);status.textContent='技能已保存。点击下方保存，将技能关联到此智能体。';
  }});
  shared.onclick=()=>{if(options)library.open({sessionId:session,scope:'account'});};
  privateSkill.onclick=()=>{if(current)library.open({sessionId:session,scope:'agent',profileId:current});};
  function skillChoices(selected){
   const labels={system:'系统内置',account:'账户共享',agent:'智能体专属',project:'当前项目'};
   const items=options.skills.filter(x=>x.scope!=='agent'||x.profileId===current);
   choices(skills,'关联 Skills（勾选已有技能）',items.map(x=>({id:x.ref||x.name,name:(labels[x.scope]?'['+labels[x.scope]+'] ':'')+x.name,description:x.description})),selected);
   for(const row of skills.querySelectorAll('.profile-choice')){const ref=row.querySelector('input').value,entry=items.find(x=>(x.ref||x.name)===ref);if(entry?.scope==='system'){const input=row.querySelector('input');input.checked=true;input.disabled=true;row.title='系统技能默认可用，按任务需要加载';}if(entry&&entry.scope!=='project'&&entry.scope!=='system'&&entry.id){const edit=element('button','btn','编辑');edit.type='button';edit.onclick=e=>{e.preventDefault();library.open({sessionId:session,id:entry.id});};row.append(edit);}}
   privateSkill.disabled=!current;privateSkill.title=current?'为此智能体创建专属技能':'先保存智能体，再创建专属技能';
  }
  const valid=()=>session===agent.selected&&epoch===(agent.identityEpoch||0)&&!modal.classList.contains('hidden');
  function hide(){const wasVisible=!modal.classList.contains('hidden');sequence++;iconTicket++;iconBusy=false;draftIcon='';showIcon();iconFile.value='';library.hide();nav.inert=fields.inert=false;modal.classList.add('hidden');options=null;current='';name.value='';prompt.value='';model.replaceChildren();list.replaceChildren();skills.replaceChildren();mcp.replaceChildren();if(wasVisible)document.querySelector('#agent-settings-toggle')?.focus({preventScroll:true});}
  close.onclick=()=>{hide();showConversation?.();};
  cancel.onclick=()=>{if(options)edit(options.profiles.find(p=>p.id===current)||{});};
  search.oninput=()=>rows();
  function choices(target,label,items,chosen){target.replaceChildren(element('legend','',label));for(const item of items){const input=element('input');input.type='checkbox';input.value=item.id;input.checked=chosen.includes(item.id);const line=field(item.name,input);line.className='profile-choice';line.title=item.description||item.name;target.append(line);}for(const id of chosen.filter(id=>!items.some(x=>x.id===id))){const input=element('input');input.type='checkbox';input.value=id;input.checked=true;const line=field((id.startsWith('project:')?'其他项目（当前不适用）：':'不可用：')+id,input);line.className='profile-choice';target.append(line);}if(!items.length&&!chosen.length)target.append(element('p','hint','暂无可用项'));}
  function edit(p={},resetTab=true){if(resetTab)activateTab(0);current=p.id||'';iconTicket++;iconBusy=false;iconUpload.disabled=false;iconFile.value='';draftIcon=p.icon||'';showIcon();name.value=p.name||'';prompt.value=p.systemPrompt||'';model.replaceChildren(element('option','','请选择模型'));model.firstChild.value='';for(const m of options.models.filter(x=>x.enabled&&x.hasApiKey)){const o=element('option','',m.name+' · '+m.model);o.value=m.id;model.append(o);}model.value=p.modelId||'';
   skillChoices(p.skills||[]);
   choices(mcp,'关联 MCP 工具',options.mcpTools.map(x=>({id:x.function.name,name:x.function.description.slice(0,150)})),p.mcpTools||[]);status.textContent='';editorTitle.textContent=current?name.value:'新建智能体';remove.disabled=!current;rows();if(resetTab)name.focus();}
  function rows(){list.replaceChildren();if(!options)return;for(const p of options.profiles.filter(p=>p.name.toLocaleLowerCase().includes(search.value.toLocaleLowerCase()))){const row=element('button','profiles-list-item'),label=element('strong','',p.name),summary=element('small','',(p.skills?.length||0)+' 个技能 · '+(p.mcpTools?.length||0)+' 个工具');row.type='button';row.classList.toggle('active',p.id===current);row.setAttribute('aria-current',p.id===current?'true':'false');const details=element('span','profile-list-details');details.append(label,summary);row.append(window.createKeeperProfileIcon(p.icon),details);row.onclick=()=>edit(p);list.append(row);}if(!list.children.length)list.append(element('p','hint',search.value?'没有匹配的智能体':'还没有智能体，点击上方新建。'));}
  remove.onclick=async()=>{const p=options?.profiles.find(p=>p.id===current);if(!p||!await window.uiConfirm('删除智能体“'+p.name+'”？历史回复快照保留，使用中的智能体不能删除。','删除智能体')||!valid())return;const ticket=sequence;try{await invoke('agent_profile_delete',{sessionId:session,id:p.id});if(ticket!==sequence||!valid())return;options=await invoke('agent_profile_options',{sessionId:session});if(ticket!==sequence||!valid())return;edit(options.profiles[0]||{});await select(session);}catch(e){if(ticket===sequence&&valid())status.textContent=String(e);}};
  async function open(){agent.earth?.hide();await ensureSession();session=agent.selected;epoch=agent.identityEpoch||0;const ticket=++sequence;agent.projectBrowser?.hide();for(const id of ['agent-main','keeper-account','keeper-settings'])document.getElementById(id)?.classList.add('hidden');modal.classList.remove('hidden');status.textContent='正在读取配置…';save.disabled=true;try{const data=await invoke('agent_profile_options',{sessionId:session});if(ticket!==sequence||!valid())return;options=data;rows();edit(data.selected||data.profiles[0]||{});save.disabled=false;}catch(e){if(ticket===sequence&&valid())status.textContent=String(e);}}
  manage.onclick=()=>{closeAccountMenu();open().catch(e=>notice(String(e)));};add.onclick=()=>{if(options)edit();};
  form.onsubmit=async e=>{e.preventDefault();if(!valid()||!options||save.disabled)return;if(iconBusy){status.textContent='请等待图标处理完成';return;}save.disabled=true;nav.inert=fields.inert=true;const target=session,ticket=sequence;try{const p=await invoke('agent_profile_save',{sessionId:target,id:current,name:name.value,icon:draftIcon,systemPrompt:prompt.value,modelId:model.value,skills:[...skills.querySelectorAll('input:checked')].map(x=>x.value).filter(value=>!value.startsWith('system:')),mcpTools:[...mcp.querySelectorAll('input:checked')].map(x=>x.value)});if(ticket!==sequence||!valid())return;const fresh=await invoke('agent_profile_options',{sessionId:target});if(ticket!==sequence||!valid())return;options=fresh;edit(p.id?p:{...options.profiles.find(x=>x.id===current),name:name.value},false);await select(target);if(valid())status.textContent='已保存，可在输入框底部选择此智能体。';}catch(e){if(ticket===sequence&&valid())status.textContent=String(e);}finally{if(ticket===sequence){save.disabled=false;nav.inert=fields.inert=false;}}};
  picker.onchange=async()=>{const target=agent.selected,identity=agent.identityEpoch;picker.disabled=true;try{await invoke('agent_profile_select',{sessionId:target,id:picker.value});if(target===agent.selected&&identity===agent.identityEpoch)await select(target);}catch(e){if(target===agent.selected&&identity===agent.identityEpoch){update();notice(String(e));}}};
  function update(){if(!modal.classList.contains('hidden')&&!valid()){hide();showConversation?.();}picker.value=agent.detail.agentProfile?.id||'';picker.replaceChildren(element('span','',agent.detail.agentProfile?.name||'通用助手'),pickerArrow.cloneNode(true));
    if(!popup.hidden){if(popupSession!==agent.selected||popupEpoch!==agent.identityEpoch||agent.detail.modelPending)closePopup();else renderChoices();}
    picker.disabled=!agent.selected||!!agent.detail.modelPending;}
  return {open,update,hide,reset(){closePopup();const visible=!modal.classList.contains('hidden');hide();if(visible)showConversation?.();picker.replaceChildren();picker.disabled=true;}};
 };
})();
