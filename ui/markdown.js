// Local-only Markdown dependencies; never execute HTML supplied by a model.
(() => {
  'use strict';
  const escape = value => String(value || '').replace(/[&<>"']/g, char => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));
  let math=[];
  const mathSlot=(text,display)=>{const index=math.push({text,display})-1;return '<'+(display?'div':'span')+' class="agent-math-slot-'+index+'"></'+(display?'div':'span')+'>';};
  const safeHtml=text=>String(text).split(/(\\?<\/?(?:details|summary)(?:\s+open)?\s*>)/ig).map(part=>/^\\?<\/?(?:details|summary)(?:\s+open)?\s*>$/i.test(part)?part.replace(/^\\/,''):escape(part)).join('');
  const parser = new marked.Marked({gfm:true,breaks:false,renderer:{
    html(token){return safeHtml(token.text);},
    image(token){return escape(token.text || '图片');},
  },extensions:[{
    name:'safeDetails',level:'block',start(source){const index=source.search(/\\?<details(?:\s+open)?\s*>/i);return index<0?undefined:index;},
    tokenizer(source){
      const opening=/^\\?<details(\s+open)?\s*>\s*\\?<summary\s*>/i.exec(source);if(!opening)return;
      const summaryEnd=/\\?<\/summary\s*>/i.exec(source.slice(opening[0].length));if(!summaryEnd)return;
      const titleEnd=opening[0].length+summaryEnd.index,bodyStart=titleEnd+summaryEnd[0].length;
      const tags=/\\?<\/?details(?:\s+open)?\s*>/ig;tags.lastIndex=bodyStart;let depth=1,closing;
      while((closing=tags.exec(source))){depth+=/<\//.test(closing[0])?-1:1;if(depth===0)break;}
      if(depth!==0 || !closing)return;
      return {type:'safeDetails',raw:source.slice(0,tags.lastIndex),open:!!opening[1],title:this.lexer.inlineTokens(source.slice(opening[0].length,titleEnd)),tokens:this.lexer.blockTokens(source.slice(bodyStart,closing.index))};
    },
    renderer(token){return '<details'+(token.open?' open':'')+'><summary>'+this.parser.parseInline(token.title)+'</summary>'+this.parser.parse(token.tokens)+'</details>';},
  },{
    name:'blockMath',level:'block',start(source){const index=source.search(/\$\$|\\\[/);return index<0?undefined:index;},
    tokenizer(source){const match=/^\$\$([\s\S]+?)\$\$(?:[ \t]*\n)?/.exec(source) || /^\\\[([\s\S]+?)\\\](?:[ \t]*\n)?/.exec(source);if(match)return {type:'blockMath',raw:match[0],text:match[1]};},
    renderer(token){return mathSlot(token.text,true);},
  },{
    name:'inlineMath',level:'inline',start(source){const index=source.search(/\$|\\\(/);return index<0?undefined:index;},
    tokenizer(source){const match=/^\$(?!\$|\s)((?:\\.|[^$\n])+?\S|[^$\s])\$(?!\$)/.exec(source) || /^\\\(([\s\S]+?)\\\)/.exec(source);if(match)return {type:'inlineMath',raw:match[0],text:match[1]};},
    renderer(token){return mathSlot(token.text,false);},
  }]});
  const allowedTags=['p','br','h1','h2','h3','h4','h5','h6','strong','em','del','blockquote','ul','ol','li','pre','code','hr','a','table','thead','tbody','tr','th','td','details','summary','span','div'];
  async function copy(text){
    if(navigator.clipboard?.writeText){await navigator.clipboard.writeText(text);return;}
    const active=document.activeElement,input=document.createElement('textarea');input.value=text;input.style.cssText='position:fixed;left:-10000px;top:0';input.setAttribute('readonly','');document.body.append(input);input.select();
    try{if(!document.execCommand('copy'))throw new Error('复制失败');}finally{input.remove();active?.focus();}
  }
  function codeBlock(pre,previous){
    const code=pre.querySelector('code');if(!code)return pre;
    const source=code.textContent,token=(code.className.match(/language-([^\s]+)/) || [,''])[1].toLowerCase();
    const language=/^[a-z0-9_+#.-]{1,32}$/.test(token)?token:'';
    if(previous?.source===source && previous.language===language)return previous.node;
    code.className='';
    let detected=language;
    if(source.length<=60000){
      try{
        const aliases={shell:'bash',console:'bash',sh:'bash'};const lang=aliases[language] || language;
        const result=hljs.getLanguage(lang)?hljs.highlight(source,{language:lang,ignoreIllegals:true}):!language && source.length<=12000?hljs.highlightAuto(source,['javascript','typescript','python','java','bash','json','sql','xml','css']):null;
        if(result){code.innerHTML=DOMPurify.sanitize(result.value,{ALLOWED_TAGS:['span'],ALLOWED_ATTR:['class'],ALLOW_DATA_ATTR:false,ALLOW_ARIA_ATTR:false});detected=language || result.language || '';}
      }catch(_){code.textContent=source;}
    }
    code.classList.add('hljs');
    const wrapper=document.createElement('div');wrapper.className='agent-code-block';
    const header=document.createElement('div');header.className='agent-code-header';
    const label=document.createElement('span');label.textContent=detected || '纯文本';
    const button=document.createElement('button');button.type='button';button.className='agent-code-copy';button.textContent='复制代码';button.setAttribute('aria-label','复制'+(detected?' '+detected:'')+'代码');
    button.onclick=async()=>{button.disabled=true;try{await copy(source);button.textContent='已复制';}catch(_){button.textContent='复制失败，请手动选择';}finally{button.disabled=false;setTimeout(()=>{button.textContent='复制代码';},1800);}};
    header.append(label,button);wrapper.append(header);wrapper._pre=pre;
    if(previous)pre.scrollLeft=previous.node.querySelector('pre')?.scrollLeft || 0;
    wrapper._codeSnapshot={source,language,node:wrapper};return wrapper;
  }
  function render(body,source,markdown=true){
    if(body._source===source && body._markdown===markdown)return;
    body._source=source;body._markdown=markdown;
    body.classList.toggle('agent-markdown',markdown);
    if(!markdown){body.textContent=source;return;}
    const previous=Array.from(body.querySelectorAll('.agent-code-block')).map(node=>node._codeSnapshot);
    const expanded=Array.from(body.querySelectorAll('details')).map(node=>({title:node.querySelector('summary')?.textContent,open:node.open}));
    try{
      math=[];
      body.innerHTML=DOMPurify.sanitize(parser.parse(source),{ALLOWED_TAGS:allowedTags,ALLOWED_ATTR:['href','title','class','start','align','open'],ALLOW_DATA_ATTR:false,ALLOW_ARIA_ATTR:false});
      for(const link of body.querySelectorAll('a')){
        const href=link.getAttribute('href') || '';
        try{if(!['http:','https:','mailto:'].includes(new URL(href,location.href).protocol))link.removeAttribute('href');}catch(_){link.removeAttribute('href');}
        link.target='_blank';link.rel='noopener noreferrer';
      }
      Array.from(body.querySelectorAll('pre')).forEach((pre,index)=>{const block=codeBlock(pre,previous[index]);if(block!==pre){pre.replaceWith(block);if(block._pre){block.append(block._pre);delete block._pre;}}});
      for(const slot of body.querySelectorAll('[class^="agent-math-slot-"]')){
        const index=Number(slot.className.slice('agent-math-slot-'.length)),formula=math[index];if(!formula)continue;
        slot.className=formula.display?'agent-math-block':'agent-math-inline';
        katex.render(formula.text,slot,{displayMode:formula.display,throwOnError:false,trust:false,maxExpand:1000,maxSize:20,strict:'ignore',macros:{}});
      }
      Array.from(body.querySelectorAll('details')).forEach((node,index)=>{if(expanded[index]?.title===node.querySelector('summary')?.textContent)node.open=expanded[index].open;});
      for(const table of body.querySelectorAll('table')){const wrapper=document.createElement('div');wrapper.className='agent-markdown-table';table.replaceWith(wrapper);wrapper.append(table);}
    }catch(_){body.textContent=source;}
  }
  window.keeperMarkdown={render};
})();
