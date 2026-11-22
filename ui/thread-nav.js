/* In-session navigation. Persists only account-scoped message anchors, never message text. */
(() => {
  'use strict';
  window.createKeeperThreadNav=()=>{
    const timeline=document.querySelector('#agent-timeline'),messages=document.querySelector('#agent-messages'),toggle=document.querySelector('#thread-directory-toggle'),directory=document.querySelector('#thread-directory'),search=document.querySelector('#thread-search'),list=document.querySelector('#thread-directory-list'),rail=document.querySelector('#thread-rail'),preview=document.querySelector('#thread-preview');
    let key=null,items=[],positions=[],active=null,restorePending=false,previous=null,following=true,saveTimer=null,frame=null,flashTimer=null,previewOwner=null,signature='';
    const make=(tag,cls,text)=>{const n=document.createElement(tag);if(cls)n.className=cls;if(text!==undefined)n.textContent=text;return n;};
    const storageKey=(account,session)=>session?'keeper-reading:v1:'+encodeURIComponent(account || 'guest')+':'+encodeURIComponent(session):null;
    const clip=(value,max=180)=>Array.from(String(value || '').replace(/\s+/g,' ').trim()).slice(0,max).join('');
    const articles=()=>Array.from(messages.querySelectorAll('[data-root-message-id]'));
    const nodeFor=id=>articles().find(n=>n.dataset.rootMessageId===id);
    function snapshot(){
      if(!key || !articles().length)return null;
      const top=timeline.getBoundingClientRect().top,nodes=articles();let node=nodes[0];
      for(const n of nodes){if(n.getBoundingClientRect().top-top<=64)node=n;else break;}
      return {root:node.dataset.rootMessageId,offset:node.getBoundingClientRect().top-top,bottom:timeline.scrollHeight-timeline.clientHeight-timeline.scrollTop<90};
    }
    function save(){if(saveTimer){clearTimeout(saveTimer);saveTimer=null;}const value=snapshot();if(value)try{localStorage.setItem(key,JSON.stringify(value));}catch{}}
    function restore(value){
      if(!value)return false;
      if(value.bottom===true){timeline.scrollTop=timeline.scrollHeight;return true;}
      const node=nodeFor(value.root);if(!node || !Number.isFinite(value.offset))return false;
      timeline.scrollTop+=node.getBoundingClientRect().top-timeline.getBoundingClientRect().top-value.offset;return true;
    }
    function hidePreview(){preview.hidden=true;if(previewOwner)previewOwner.removeAttribute('aria-describedby');previewOwner=null;}
    function closeDirectory(focus=false){directory.hidden=true;toggle.setAttribute('aria-expanded','false');if(focus)toggle.focus();}
    function markCurrent(){
      if(!items.length)return;
      const top=timeline.getBoundingClientRect().top;let current=items[0].root;
      for(const p of positions){if(p.node.getBoundingClientRect().top-top<=100)current=p.root;else break;}
      if(timeline.scrollHeight-timeline.clientHeight-timeline.scrollTop<6)current=items.at(-1).root;
      if(current!==active){active=current;for(const n of document.querySelectorAll('[data-thread-root]')){const yes=n.dataset.threadRoot===active;n.classList.toggle('current',yes);if(yes)n.setAttribute('aria-current','step');else n.removeAttribute('aria-current');}}
    }
    function jump(root){
      const node=nodeFor(root);if(!node)return;
      closeDirectory();hidePreview();timeline.scrollTop+=node.getBoundingClientRect().top-timeline.getBoundingClientRect().top-24;
      messages.querySelectorAll('.thread-target').forEach(n=>n.classList.remove('thread-target'));node.classList.add('thread-target');node.tabIndex=-1;node.focus({preventScroll:true});
      clearTimeout(flashTimer);flashTimer=setTimeout(()=>node.classList.remove('thread-target'),1400);markCurrent();save();
    }
    function showPreview(item,button){
      hidePreview();previewOwner=button;button.setAttribute('aria-describedby','thread-preview');preview.replaceChildren();
      preview.append(make('strong','',clip(item.question,200)),make('p','',clip(item.answer,260) || '尚无回复'));
      const date=new Date(item.time);preview.append(make('small','',item.status+' · '+(Number.isFinite(date.getTime())?date.toLocaleString():'')));preview.hidden=false;
      const r=button.getBoundingClientRect(),width=preview.getBoundingClientRect().width,height=preview.getBoundingClientRect().height;
      const left=rail.contains(button)?r.right+12:r.left-width-12;
      preview.style.left=Math.max(12,Math.min(window.innerWidth-width-12,left))+'px';preview.style.top=Math.max(12,Math.min(window.innerHeight-height-12,r.top-height/2))+'px';
    }
    function previewBindings(button,item){const current=()=>items.find(i=>i.root===item.root) || item;button.onmouseenter=()=>showPreview(current(),button);button.onmouseleave=()=>hidePreview();button.onfocus=()=>showPreview(current(),button);button.onblur=()=>hidePreview();}
    function renderDirectory(){
      const query=search.value.trim().toLocaleLowerCase(),shown=items.filter(item=>(item.question+' '+item.answer).toLocaleLowerCase().includes(query));list.replaceChildren();
      for(const item of shown){const b=make('button','thread-directory-item');b.type='button';b.dataset.threadRoot=item.root;const title=clip(item.question,128) || '未命名问题';b.append(make('span','thread-item-number',String(item.index+1)),make('span','thread-item-question',title),make('small','',item.status));b.onclick=()=>jump(item.root);previewBindings(b,item);list.append(b);}
      if(!shown.length)list.append(make('p','thread-empty',items.length?'没有匹配的问题':'暂无对话'));
      active=null;markCurrent();
    }
    function update(data){
      const visible=data.messages || [];let current=null;items=[];
      for(const entry of visible){if(entry.role==='user'){current={root:entry.replyGroup?.rootId || entry.id,question:entry.content?.trim() || (entry.attachments?.length?'附件：'+entry.attachments.map(a=>a.name).join('、'):'未命名问题'),answer:'',time:entry.createdAt,status:'尚无回复',index:items.length};items.push(current);}else if(current && entry.role==='assistant'){
        current.answer=entry.content;current.response=entry.id;const status=entry.modelMetadata?.generationStatus;current.status=({streaming:'生成中',stopped:'已停止',interrupted:'已中断',failed:'失败',complete:'已完成'})[status] || '已完成';
        if(data.detail?.runs?.some(run=>run.responseId===entry.id && run.status==='pending_approval'))current.status='等待确认';
      }else if(current && entry.role==='error')current.status='失败';}
      const nodes=new Map(articles().map(node=>[node.dataset.rootMessageId,node]));positions=items.map(item=>({...item,node:nodes.get(item.root)})).filter(p=>p.node);
      const nextSignature=JSON.stringify(items.map(i=>[i.root,i.question,i.status]));
      if(nextSignature!==signature){signature=nextSignature;rail.replaceChildren();for(const item of items){const tick=make('button','thread-tick');tick.type='button';tick.dataset.threadRoot=item.root;tick.dataset.status=item.status;tick.setAttribute('aria-label','第 '+(item.index+1)+' 轮：'+clip(item.question,90)+'，'+item.status);tick.append(make('span',''));tick.onclick=()=>jump(item.root);previewBindings(tick,item);rail.append(tick);}renderDirectory();hidePreview();}
      toggle.disabled=!items.length;toggle.textContent='会话'+(items.length?' '+items.length:'');rail.hidden=items.length<2;
      if(restorePending && items.length){let saved=null;try{saved=JSON.parse(localStorage.getItem(key));}catch{}if(!restore(saved))timeline.scrollTop=timeline.scrollHeight;restorePending=false;}
      else if(previous && !following)restore(previous);
      previous=null;active=null;markCurrent();
      if(previewOwner && !preview.hidden){const item=items.find(i=>i.root===previewOwner.dataset.threadRoot);if(item)showPreview(item,previewOwner);}
    }
    function begin({account,session}){
      const next=storageKey(account,session);
      if(next!==key){save();key=next;items=[];positions=[];signature='';active=null;restorePending=!!key;previous=null;closeDirectory();hidePreview();search.value='';}
      else{previous=snapshot();following=timeline.scrollHeight-timeline.clientHeight-timeline.scrollTop<90;}
    }
    timeline.addEventListener('scroll',()=>{if(frame!==null)cancelAnimationFrame(frame);frame=requestAnimationFrame(()=>{frame=null;markCurrent();});clearTimeout(saveTimer);saveTimer=setTimeout(save,150);hidePreview();},{passive:true});
    window.addEventListener('pagehide',save);document.addEventListener('visibilitychange',()=>{if(document.hidden)save();});window.addEventListener('resize',()=>{hidePreview();markCurrent();});
    toggle.onclick=()=>{if(directory.hidden){directory.hidden=false;toggle.setAttribute('aria-expanded','true');search.focus();}else closeDirectory(true);};search.oninput=renderDirectory;
    directory.onkeydown=event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeDirectory(true);}else if(event.key==='ArrowDown' && event.target===search){event.preventDefault();list.querySelector('button')?.focus();}};
    document.addEventListener('click',event=>{if(!event.target.closest('.thread-directory-control'))closeDirectory();});rail.onkeydown=event=>{if(event.key==='Escape'){hidePreview();toggle.focus();}};
    return {begin,update,save,jump};
  };
})();
