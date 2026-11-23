/* Skills are versioned local text bundles; no script is executed by this editor. */
(() => {
 'use strict';
 window.createKeeperSkillLibrary=({invoke,element,valid,saved,host,visibility=()=>{}})=>{
  const modal=element('section','skill-library-editor hidden');modal.id='agent-skill-library-editor';
  const box=element('div','skill-library-box'),head=element('header','llm-dialog-head'),title=element('h2','','编辑技能'),close=element('button','btn','← 返回配置');
  box.setAttribute('role','region');title.id='skill-library-title';box.setAttribute('aria-labelledby',title.id);close.type='button';close.setAttribute('aria-label','关闭技能编辑');head.append(title,close);
  const form=element('form'),fields=element('div','llm-dialog-fields'),name=element('input'),description=element('input'),file=element('select'),text=element('textarea'),path=element('input'),add=element('button','btn','新建文件'),status=element('p','hint'),actions=element('div','modal-actions'),cancel=element('button','btn','取消'),save=element('button','btn primary','保存技能');
  name.id='library-skill-name';name.required=true;name.maxLength=64;name.pattern='[a-z0-9]+(-[a-z0-9]+)*';description.id='library-skill-description';description.required=true;description.maxLength=1024;
  text.id='library-skill-content';text.rows=13;text.spellcheck=false;file.setAttribute('aria-label','技能文件');path.placeholder='references/example.md';path.setAttribute('aria-label','新文件路径');add.type=cancel.type='button';save.type='submit';status.setAttribute('role','status');
  function field(label,input){const e=element('label','',label);e.append(input);return e;}
  const fileActions=element('div','profile-skill-actions');fileActions.append(path,add);
  fields.append(field('技能名称（小写字母、数字、连字符）',name),field('用途简介',description),element('p','hint','技能以本机 SQLite 文件包保存。SKILL.md 中的 name 和 description 在保存时与上方字段同步；scripts、references、assets 文件按需读取，脚本不会自动执行。'),field('技能文件',file),text,fileActions,status);actions.append(cancel,save);form.append(fields,actions);box.append(head,form);modal.append(box);host.append(modal);
  let ticket=0,context={},files={},active='SKILL.md',busy=false,returnFocus;
  const live=t=>t===ticket&&valid()&&!modal.classList.contains('hidden');
  function hide(){ticket++;modal.classList.add('hidden');visibility(false);context={};files={};text.value='';name.value='';description.value='';if(returnFocus?.isConnected)returnFocus.focus();}
  close.onclick=cancel.onclick=hide;
  modal.addEventListener('keydown',e=>{if(e.key==='Escape'){e.preventDefault();e.stopPropagation();hide();}});
  function render(){file.replaceChildren();for(const key of Object.keys(files).sort()){const o=element('option','',key);o.value=key;file.append(o);}file.value=active;text.value=files[active]||'';}
  file.onchange=()=>{files[active]=text.value;active=file.value;text.value=files[active];};
  add.onclick=()=>{const key=path.value.trim();if(!/^(scripts|references|assets)\/.+/.test(key)||key.length>200||key.includes('\\')||key.split('/').some(x=>!x||x==='.'||x==='..'||x.includes(':'))){status.textContent='请填写 scripts、references 或 assets 下的相对文件路径。';return;}if(Object.hasOwn(files,key)){status.textContent='文件已经存在。';return;}files[active]=text.value;files[key]='';active=key;path.value='';status.textContent='';render();text.focus();};
  async function open(args){returnFocus=document.activeElement;const t=++ticket;context={...args};files={'SKILL.md':'---\nname: new-skill\ndescription: Describe when to use this skill\n---\n\n# 工作指导\n\n说明适用场景、操作步骤和验证方式。\n'};active='SKILL.md';busy=false;name.value='';description.value='';path.value='';status.textContent='';modal.classList.remove('hidden');visibility(true);save.disabled=!!args.id;
   if(args.id){status.textContent='正在读取技能…';try{const entry=await invoke('agent_library_skill_read',args);if(!live(t))return;context={...args,...entry,expectedRevision:entry.revision};files={...entry.files};name.value=entry.name;description.value=entry.description;status.textContent='';}catch(e){if(live(t))status.textContent=String(e);return;}}
   if(!live(t))return;title.textContent=(args.id?'编辑':'新建')+(context.scope==='agent'?'智能体专属技能':'账户共享技能');render();save.disabled=false;name.focus();
  }
  form.onsubmit=async e=>{e.preventDefault();if(busy||!valid())return;const t=ticket;busy=true;save.disabled=true;files[active]=text.value;
   // Keep all other YAML fields, but edit identity and description in one place.
   const source=files['SKILL.md'];const match=source.match(/^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/);
   if(!match){status.textContent='SKILL.md 必须包含 YAML frontmatter。';busy=false;save.disabled=false;return;}
   const lines=match[1].split(/\r?\n/),rest=[];let skipping=false;for(const line of lines){if(/^(name|description)\s*:/.test(line)){skipping=true;continue;}if(skipping&&(/^\s/.test(line)||!line.trim()))continue;skipping=false;rest.push(line);}
   files['SKILL.md']='---\nname: '+JSON.stringify(name.value.trim())+'\ndescription: '+JSON.stringify(description.value.trim())+'\n'+(rest.length?rest.join('\n')+'\n':'')+'---\n'+source.slice(match[0].length);
   try{const entry=await invoke('agent_library_skill_save',{sessionId:context.sessionId,id:context.id||'',scope:context.scope,profileId:context.profileId||'',expectedRevision:context.expectedRevision,name:name.value.trim(),description:description.value.trim(),files});if(!live(t))return;await saved(entry);if(live(t))hide();}catch(e){if(live(t))status.textContent=String(e);}finally{if(t===ticket){busy=false;save.disabled=false;}}
  };
  return {open,hide};
 };
})();
