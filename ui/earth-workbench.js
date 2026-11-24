(() => {
  'use strict';
  window.createKeeperEarthWorkbench = ({agent, invoke, select, switchProject, notice}) => {
    const $ = s => document.querySelector(s);
    const node = (tag, cls, text) => {const n=document.createElement(tag);n.className=cls;if(text!==undefined)n.textContent=text;return n;};
    const button = (text, action) => {const b=node('button','btn',text);b.type='button';b.onclick=action;return b;};
    const main=$('#agent-main'), input=$('#agent-input');
    const active=()=>document.body.classList.contains('earth-workbench');
    const entry=button('地球工作台',()=>{const p=agent.projects.find(p=>p.id===(agent.detail.session?.projectId||agent.project));if(p)agent.earth.openWorkbench(p);});
    entry.className='guidance-trigger earth-workbench-launcher';$('.agent-header-tools').append(entry);
    const browse=button('项目 / 会话',()=>openPicker());browse.classList.add('earth-workbench-action','earth-project-browse');
    const collapse=button('收起消息',()=>{document.body.classList.add('earth-chat-collapsed');remember();});collapse.classList.add('earth-workbench-action','earth-messages-collapse');
    const reopen=button('显示消息',()=>{document.body.classList.remove('earth-chat-collapsed');remember();});reopen.className='btn earth-chat-reopen';$('.earth-toolbar').append(browse,collapse,reopen);
    const picker=node('section','earth-project-picker');picker.hidden=true;picker.setAttribute('role','dialog');picker.setAttribute('aria-label','查询项目和会话');
    const search=node('input','');search.type='search';search.placeholder='搜索项目或会话';search.setAttribute('aria-label','搜索项目或会话');
    const dismiss=button('关闭',()=>closePicker());const heading=node('header','');heading.append(search,dismiss);
    const results=node('div','earth-project-results');picker.append(heading,results);main.append(picker);
    let revision=0, debounce, page=null, filter=null, cursor=null, selected=0, command=false, items=[];
    const key=()=> 'keeper-earth-window:'+(agent.ownerKey||'guest');
    const remember=()=>{if(!active())return;try{localStorage.setItem(key(),JSON.stringify({collapsed:document.body.classList.contains('earth-chat-collapsed')}));}catch{}};
    const composer=$('.agent-compose-area');
    function measureComposer(){if(active()&&composer)main.style.setProperty('--earth-compose-height',Math.ceil(composer.getBoundingClientRect().height)+'px');}
    window.addEventListener('earth-layout',()=>{
      closePicker(false);
      for(const k of ['width','height','left','top'])main.style[k]='';
      if(active()){let saved={};try{saved=JSON.parse(localStorage.getItem(key())||'{}');}catch{}document.body.classList.toggle('earth-chat-collapsed',!!saved.collapsed);measureComposer();}
      else main.style.removeProperty('--earth-compose-height');
    });
    window.addEventListener('resize',measureComposer);
    if(window.ResizeObserver&&composer)new ResizeObserver(measureComposer).observe(composer);
    function closePicker(focus=true){picker.hidden=true;++revision;clearTimeout(debounce);if(focus)input.focus();}
    function openPicker(fromCommand=false){command=fromCommand;filter=null;search.value='';picker.hidden=false;load();search.focus();}
    function mark(){const buttons=[...results.querySelectorAll('button')];buttons.forEach((b,i)=>b.classList.toggle('current',i===selected));buttons[selected]?.scrollIntoView?.({block:'nearest'});}
    async function choose(p,session){const epoch=agent.identityEpoch||0;closePicker(false);try{if(command&&/^\/projects\s*$/.test(input.value)){input.value='';input.dispatchEvent(new Event('input',{bubbles:true}));}if(session){if(!await select(session.id))return;agent.project=p.id;}else{await switchProject(p.id);if(agent.project!==p.id)return;}if(epoch!==(agent.identityEpoch||0))return;await agent.earth.openWorkbench(p);input.focus();}catch(e){notice(String(e));}}
    async function load(append=false){const ticket=++revision,epoch=agent.identityEpoch||0,q=search.value.trim();if(!append){results.replaceChildren(node('p','hint','正在查询…'));cursor=null;}try{page=await invoke('agent_sessions',{state:'active',q,projectId:filter?.id,limit:30,cursor:append?cursor:undefined});if(ticket!==revision||epoch!==(agent.identityEpoch||0)||picker.hidden)return;
      items=append?[...new Map([...items,...page.items].map(s=>[s.id,s])).values()]:page.items;
      results.replaceChildren();if(filter)results.append(button('← 所有项目',()=>{filter=null;load();}));
      for(const p of agent.projects){const sessions=items.filter(s=>s.projectId===p.id);if(filter&&p.id!==filter.id)continue;if(!sessions.length&&!p.name.toLowerCase().includes(q.toLowerCase()))continue;
        const group=node('section','earth-project-section'),row=node('div','earth-project-group');row.append(button('▸ '+p.name,()=>{filter=p;search.value='';load();}),button('打开项目',()=>choose(p)));group.append(row);
        for(const session of sessions){const b=button(session.title,()=>choose(p,session));b.className='earth-session-result';b.title=p.name+' / '+session.title;group.append(b);}results.append(group);
      }cursor=page.nextCursor;
      if(page.hasMore){const more=button('加载更多',()=>load(true));more.className='earth-project-more';results.append(more);}if(!results.children.length)results.append(node('p','hint','未找到项目或会话'));selected=0;mark();
    }catch(e){if(ticket===revision&&!picker.hidden)results.replaceChildren(node('p','hint','查询失败：'+String(e)));}}
    search.addEventListener('input',()=>{++revision;clearTimeout(debounce);debounce=setTimeout(()=>load(),180);});
    picker.addEventListener('keydown',e=>{if(e.key==='Escape'){e.preventDefault();e.stopPropagation();closePicker();}else if(['ArrowDown','ArrowUp','Enter'].includes(e.key)&&e.target===search){e.preventDefault();const buttons=results.querySelectorAll('button');if(e.key==='Enter')buttons[selected]?.click();else{selected=Math.max(0,Math.min(buttons.length-1,selected+(e.key==='ArrowDown'?1:-1)));mark();}}});
    input.addEventListener('input',()=>{if(active()&&/^\/projects\s*$/.test(input.value)&&picker.hidden)openPicker(true);});
    input.addEventListener('keydown',e=>{if(active()&&/^\/projects\s*$/.test(input.value)&&e.key==='Enter'){e.preventDefault();e.stopImmediatePropagation();openPicker(true);}},true);
    $('#agent-compose').addEventListener('submit',e=>{if(active()&&/^\/projects\s*$/.test(input.value)){e.preventDefault();e.stopImmediatePropagation();openPicker(true);}},true);
    return {openPicker,restoreProject:async p=>{if(agent.detail.session?.projectId!==p.id)await switchProject(p.id);}};
  };
})();
