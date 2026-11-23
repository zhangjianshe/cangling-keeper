/* Skills are versioned local text bundles; no script is executed by this editor. */
(() => {
 'use strict';
 window.createKeeperSkillLibrary=({invoke,element,valid,saved,host,visibility=()=>{}})=>{
  const modal=element('section','skill-library-editor hidden');modal.id='agent-skill-library-editor';
  const box=element('div','skill-library-box modal-box'),head=element('header','llm-dialog-head'),title=element('h2','','编辑技能'),close=element('button','btn','×');
  box.setAttribute('role','dialog');box.setAttribute('aria-modal','true');title.id='skill-library-title';box.setAttribute('aria-labelledby',title.id);close.type='button';close.setAttribute('aria-label','关闭技能编辑');head.append(title,close);
  const form=element('form'),fields=element('div','llm-dialog-fields'),name=element('input'),description=element('input'),tree=element('nav','skill-file-tree'),text=element('div','project-code-editor skill-code-editor'),path=element('input'),add=element('button','btn','新建文件'),addDirectory=element('button','btn','新建目录'),status=element('p','hint'),actions=element('div','modal-actions'),cancel=element('button','btn','取消'),save=element('button','btn primary','保存技能');
  name.id='library-skill-name';name.required=true;name.maxLength=64;name.pattern='[a-z0-9]+(-[a-z0-9]+)*';description.id='library-skill-description';description.required=true;description.maxLength=1024;
  text.id='library-skill-content';tree.setAttribute('aria-label','技能目录');text.setAttribute('aria-label','技能文件内容');path.placeholder='references/example.md';path.setAttribute('aria-label','新文件路径');add.type=addDirectory.type=cancel.type='button';save.type='submit';status.setAttribute('role','status');
  function field(label,input){const e=element('label','',label);e.append(input);return e;}
  const fileActions=element('div','skill-file-actions');fileActions.append(path,add,addDirectory);
  const metadata=element('div','skill-metadata');metadata.append(field('技能名称（小写字母、数字、连字符）',name),field('用途简介',description));
  const workspace=element('div','skill-file-workspace'),editor=element('div','skill-file-editor');const modes=element('div','project-markdown-modes'),documentArea=element('div','project-document-workspace'),preview=element('article','project-markdown-preview');
  modes.setAttribute('role','group');modes.setAttribute('aria-label','技能 Markdown 显示模式');documentArea.append(text,preview);editor.append(documentArea);fileActions.append(modes);workspace.append(tree,editor);
  const tabs=element('div','profiles-tabs'),basic=element('section','skill-tab-panel'),resources=element('section','skill-tab-panel');tabs.setAttribute('role','tablist');tabs.setAttribute('aria-label','技能编辑分类');
  basic.append(metadata,element('p','hint','设置技能名称和用途简介，工作指导请在资源编辑中的 SKILL.md 编写。'));
  resources.append(fileActions,workspace);
  const panels=[basic,resources],tabButtons=['基础信息','资源编辑'].map((label,i)=>{const b=element('button','profiles-tab',label);b.type='button';b.id='skill-tab-'+i;b.setAttribute('role','tab');b.setAttribute('aria-controls','skill-panel-'+i);panels[i].id='skill-panel-'+i;panels[i].setAttribute('role','tabpanel');panels[i].setAttribute('aria-labelledby',b.id);b.onclick=()=>activateTab(i);b.onkeydown=e=>{const keys={ArrowRight:(i+1)%2,ArrowLeft:(i+1)%2,Home:0,End:1};if(e.key in keys){e.preventDefault();activateTab(keys[e.key],true);}};tabs.append(b);return b;});
  function activateTab(index,focus=false){panels.forEach((p,i)=>{p.hidden=i!==index;tabButtons[i].setAttribute('aria-selected',String(i===index));tabButtons[i].tabIndex=i===index?0:-1;});if(focus)tabButtons[index].focus();if(index===1)aceEditor.resize();}
  form.addEventListener('invalid',e=>{if(basic.contains(e.target))activateTab(0);},true);
  fields.append(basic,resources);actions.append(status,cancel,save);form.append(tabs,fields,actions);box.append(head,form);modal.append(box);host.append(modal);
  let ticket=0,context={},files={},directories=new Set(),active='SKILL.md',busy=false,returnFocus,viewMode='edit';
  window.ace.config.set('basePath','/vendor/ace');const aceEditor=window.ace.edit(text);aceEditor.setTheme('ace/theme/github');aceEditor.session.setUseWorker(false);aceEditor.setOptions({fontSize:'var(--agent-list-font-size, 14px)',showPrintMargin:false});
  for(const [value,label] of [['edit','编辑'],['preview','预览'],['split','分屏']]){const b=element('button','btn skill-mode-icon');b.type='button';b.title=label;b.setAttribute('aria-label',label);const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 24 24');svg.setAttribute('aria-hidden','true');const shape=document.createElementNS(svg.namespaceURI,'path');shape.setAttribute('d',({edit:'m16 3 5 5-12 12-6 1 1-6ZM14 5l5 5',preview:'M2 12s3-7 10-7 10 7 10 7-3 7-10 7S2 12 2 12Zm13 0a3 3 0 1 1-6 0 3 3 0 0 1 6 0',split:'M3 4h18v16H3ZM12 4v16'})[value]);svg.append(shape);b.append(svg);b.dataset.mode=value;b.onclick=()=>{viewMode=value;updateView();};modes.append(b);}
  function clearPreview(){preview.replaceChildren();delete preview._source;}
  function updateView(){const markdown=/\.(md|markdown)$/i.test(active),mode=markdown?viewMode:'edit';modes.hidden=!markdown;documentArea.dataset.mode=mode;text.hidden=mode==='preview';preview.hidden=mode==='edit';for(const b of modes.children)b.setAttribute('aria-pressed',String(b.dataset.mode===mode));if(!preview.hidden&&!modal.classList.contains('hidden'))window.keeperMarkdown.render(preview,aceEditor.getValue());else clearPreview();aceEditor.resize();}
  aceEditor.session.on('change',updateView);
  if(window.ResizeObserver)new ResizeObserver(()=>aceEditor.resize()).observe(documentArea);
  const live=t=>t===ticket&&valid()&&!modal.classList.contains('hidden');
  function hide(){const wasVisible=!modal.classList.contains('hidden');ticket++;modal.classList.add('hidden');visibility(false);context={};files={};directories.clear();tree.replaceChildren();aceEditor.setValue('',-1);clearPreview();name.value='';description.value='';if(wasVisible&&returnFocus?.isConnected)returnFocus.focus({preventScroll:true});}
  close.onclick=cancel.onclick=hide;
  modal.addEventListener('keydown',e=>{if(e.key==='Tab'){const focusable=[...box.querySelectorAll('button,input,textarea,select,[tabindex]')].filter(x=>!x.disabled&&x.tabIndex>=0&&!x.closest('[hidden]'));const first=focusable[0],last=focusable.at(-1);if(e.shiftKey&&document.activeElement===first){e.preventDefault();last?.focus();}else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first?.focus();}}if(e.key==='Escape'){e.preventDefault();e.stopPropagation();hide();}});
  function parents(key){let p=key;while(p.includes('/')){p=p.slice(0,p.lastIndexOf('/'));directories.add(p);}}
  function icon(folder){const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 24 24');svg.setAttribute('aria-hidden','true');const shape=document.createElementNS(svg.namespaceURI,'path');shape.setAttribute('d',folder?'M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v11H3ZM3 9h18':'M5 3h9l5 5v13H5ZM14 3v6h5M8 13h8M8 17h8');svg.append(shape);return svg;}
  function render(){tree.replaceChildren();for(const key of Object.keys(files))parents(key);for(const key of [...directories])parents(key);
   function branch(container,parent=''){
    const direct=key=>key.slice(0,key.lastIndexOf('/')+1)===(parent?parent+'/':'');
    for(const key of [...directories].filter(direct).sort()){const group=element('details','skill-directory'),summary=element('summary');group.open=true;summary.append(icon(true),document.createTextNode(key.split('/').at(-1)));summary.title=key;summary.onclick=()=>{path.value=key+'/';};group.append(summary);branch(group,key);if(group.children.length===1)group.append(element('small','skill-tree-empty','空目录'));container.append(group);}
    for(const key of Object.keys(files).filter(direct).sort()){const button=element('button','skill-tree-file');button.type='button';button.title=key;button.dataset.path=key;button.setAttribute('aria-current',String(key===active));button.append(icon(false),document.createTextNode(key.split('/').at(-1)));button.onclick=()=>{files[active]=aceEditor.getValue();active=key;render();aceEditor.focus();};container.append(button);}
   }
   branch(tree);aceEditor.setValue(files[active]||'',-1);const ext=active.split('.').pop().toLowerCase(),languages={js:'javascript',ts:'typescript',json:'json',html:'html',css:'css',py:'python',java:'java',rs:'rust',c:'c_cpp',cpp:'c_cpp',h:'c_cpp',sh:'sh',yaml:'yaml',yml:'yaml',xml:'xml',md:'markdown',markdown:'markdown',sql:'sql',ini:'ini'};aceEditor.session.setMode('ace/mode/'+(languages[ext]||'text'));aceEditor.session.getUndoManager().reset();updateView();path.value=active;text.setAttribute('aria-label',active);
  }
  function create(directory){const key=path.value.trim().replace(/\/$/,'');if(!/^(scripts|references|assets)(\/|$)/.test(key)||key.length>200||key.includes('\\')||/[\x00-\x1f\x7f]/.test(key)||key.split('/').some(x=>!x||x==='.'||x==='..'||x.includes(':'))){status.textContent='请填写 scripts、references 或 assets 下的相对路径。';return;}if(Object.hasOwn(files,key)||directories.has(key)){status.textContent='文件或目录已经存在。';return;}let parent=key;while(parent.includes('/')){parent=parent.slice(0,parent.lastIndexOf('/'));if(Object.hasOwn(files,parent)){status.textContent='父路径是文件，不能创建子项。';return;}}
   files[active]=aceEditor.getValue();parents(key);if(directory)directories.add(key);else{files[key]='';active=key;}path.value='';status.textContent='尚未保存，点击“保存技能”写入文件和目录。';render();if(!directory)aceEditor.focus();
  }
  add.onclick=()=>create(false);addDirectory.onclick=()=>create(true);
  async function open(args){returnFocus=document.activeElement;const t=++ticket;context={...args};files={'SKILL.md':'---\nname: new-skill\ndescription: Describe when to use this skill\n---\n\n# 工作指导\n\n说明适用场景、操作步骤和验证方式。\n'};directories=new Set(['scripts','references','assets']);active='SKILL.md';viewMode='edit';busy=false;name.value='';description.value='';path.value='';status.textContent='';modal.classList.remove('hidden');visibility(true);activateTab(0);close.focus();save.disabled=!!args.id;tree.replaceChildren();aceEditor.setValue('',-1);clearPreview();
   if(args.id){status.textContent='正在读取技能…';try{const entry=await invoke('agent_library_skill_read',args);if(!live(t))return;context={...args,...entry,expectedRevision:entry.revision};files={...entry.files};directories=new Set(entry.directories||['scripts','references','assets']);name.value=entry.name;description.value=entry.description;status.textContent='';}catch(e){if(live(t))status.textContent=String(e);return;}}
   if(!live(t))return;title.textContent=(args.id?'编辑':'新建')+(context.scope==='agent'?'智能体专属技能':'账户共享技能');render();save.disabled=false;name.focus();
  }
  form.onsubmit=async e=>{e.preventDefault();if(busy||!valid())return;const t=ticket;busy=true;save.disabled=true;files[active]=aceEditor.getValue();
   // Keep all other YAML fields, but edit identity and description in one place.
   const source=files['SKILL.md'];const match=source.match(/^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/);
   if(!match){status.textContent='SKILL.md 必须包含 YAML frontmatter。';busy=false;save.disabled=false;return;}
   const lines=match[1].split(/\r?\n/),rest=[];let skipping=false;for(const line of lines){if(/^(name|description)\s*:/.test(line)){skipping=true;continue;}if(skipping&&(/^\s/.test(line)||!line.trim()))continue;skipping=false;rest.push(line);}
   files['SKILL.md']='---\nname: '+JSON.stringify(name.value.trim())+'\ndescription: '+JSON.stringify(description.value.trim())+'\n'+(rest.length?rest.join('\n')+'\n':'')+'---\n'+source.slice(match[0].length);
   try{const entry=await invoke('agent_library_skill_save',{sessionId:context.sessionId,id:context.id||'',scope:context.scope,profileId:context.profileId||'',expectedRevision:context.expectedRevision,name:name.value.trim(),description:description.value.trim(),files,directories:[...directories]});if(!live(t))return;await saved(entry);if(live(t))hide();}catch(e){if(live(t))status.textContent=String(e);}finally{if(t===ticket){busy=false;save.disabled=false;}}
  };
  return {open,hide};
 };
})();
