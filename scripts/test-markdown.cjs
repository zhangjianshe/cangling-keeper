#!/usr/bin/env node
// Run with NODE_PATH pointing to a dev-only jsdom installation.
const assert=require('node:assert/strict');
const fs=require('node:fs');const path=require('node:path');const {JSDOM}=require('jsdom');
const root=path.resolve(__dirname,'..');
const dom=new JSDOM('<!doctype html><div id="reply"></div>',{url:'http://127.0.0.1:4301/',runScripts:'outside-only'});
const w=dom.window;let copied=null;Object.defineProperty(w.navigator,'clipboard',{value:{writeText:async text=>{copied=text;}}});
for(const file of ['vendor/marked.js','vendor/purify.min.js','vendor/highlight.min.js','vendor/katex/katex.min.js','markdown.js'])w.eval(fs.readFileSync(path.join(root,'ui',file),'utf8'));
const body=w.document.getElementById('reply');
const source='## 标题\n\n**加粗**与 `inline()`\n\n- 第一项\n- 第二项\n\n> 引用\n\n| 名称 | 值 |\n| --- | --- |\n| 测试 | 1 |\n\n```javascript\nconst answer = 42;\nconsole.log(answer);\n```\n\n[链接](https://example.com)';
w.keeperMarkdown.render(body,source);
assert.equal(body.querySelector('h2').textContent,'标题');assert.equal(body.querySelectorAll('li').length,2);assert.ok(body.querySelector('table'));assert.ok(body.querySelector('blockquote'));assert.ok(body.querySelector('strong'));
const block=body.querySelector('.agent-code-block');assert.ok(block);assert.ok(block.querySelector('.hljs-keyword'));assert.equal(body.querySelector('a').rel,'noopener noreferrer');
const button=block.querySelector('button');button.click();
(async()=>{
 await new Promise(resolve=>setImmediate(resolve));assert.equal(copied,'const answer = 42;\nconsole.log(answer);\n');assert.equal(button.textContent,'已复制');
 w.keeperMarkdown.render(body,source+'\n\n新增流式段落');assert.equal(body.querySelector('.agent-code-block'),block);assert.equal(button.textContent,'已复制');
 w.keeperMarkdown.render(body,'```python\nprint("你好');assert.equal(body.querySelector('code').textContent,'print("你好\n');assert.ok(body.querySelector('.agent-code-copy'));
 w.keeperMarkdown.render(body,'```python\nprint("你好")\n```');assert.equal(body.querySelector('code').textContent,'print("你好")\n');assert.ok(body.querySelector('.hljs-string'));
 const malicious='<script>window.pwned=1</script>\n\n<img src="https://example.com/track" onerror="window.pwned=1">\n\n[bad](javascript:alert(1))\n\n[bad-data](data:text/html,test)\n\n<svg onload="alert(1)"></svg>\n\n```html\n<script>alert(1)</script>\n```';
 w.keeperMarkdown.render(body,malicious);assert.equal(body.querySelectorAll('script,img,svg:not(.agent-copy-icon),iframe,style,form').length,0);assert.equal(body.querySelectorAll('[onerror],[onload]').length,0);for(const a of body.querySelectorAll('a'))assert.ok(!a.getAttribute('href'));assert.equal(body.querySelector('code').textContent,'<script>alert(1)</script>\n');
 w.keeperMarkdown.render(body,'**用户文字**',false);assert.equal(body.textContent,'**用户文字**');assert.equal(body.querySelector('strong'),null);

 w.keeperMarkdown.render(body,fs.readFileSync(path.join(root,'scripts/fixtures/math.md'),'utf8'));
 assert.equal(body.querySelectorAll('.katex').length,4);assert.equal(body.querySelectorAll('.katex-display').length,2);assert.equal(body.querySelector('.katex-error'),null);
 const details=fs.readFileSync(path.join(root,'scripts/fixtures/details.md'),'utf8');
 w.keeperMarkdown.render(body,details);assert.ok(body.querySelector('details'));assert.equal(body.querySelector('summary').textContent,'Click to expand');assert.ok(body.querySelector('details strong'));assert.ok(body.querySelector('details .katex'));
 body.querySelector('details').open=true;w.keeperMarkdown.render(body,details+'\n\nNew streaming content');assert.equal(body.querySelector('details').open,true);
 w.keeperMarkdown.render(body,'<details open><summary>Outer</summary>\n\n<details><summary>Inner</summary>\n\nText\n\n</details>\n\n</details>');assert.equal(body.querySelectorAll('details').length,2);
 const fence=String.fromCharCode(96).repeat(3),fenced=fence+'text\n$E=mc^2$\n<details><summary>Example</summary></details>\n'+fence;
 w.keeperMarkdown.render(body,fenced);assert.equal(body.querySelectorAll('.katex,details').length,0);assert.ok(body.querySelector('code').textContent.includes('$E=mc^2$'));
 const slash=String.fromCharCode(92);
 w.keeperMarkdown.render(body,'$'+slash+'includegraphics{https://example.com/track}$\n\n<details onclick="alert(1)"><summary onmouseover="alert(1)">Bad</summary><img src=x onerror=alert(1)></details>');
 assert.equal(body.querySelectorAll('img,[onclick],[onmouseover],[onerror],script').length,0);
 w.keeperMarkdown.render(body,'Streaming $E = mc');assert.equal(body.querySelectorAll('.katex').length,0);w.keeperMarkdown.render(body,'Streaming $E = mc^2$');assert.equal(body.querySelectorAll('.katex').length,1);
 console.log('Markdown checks passed: GFM, safe links/HTML, syntax highlighting, exact copy, streaming fences and code block reuse.');dom.window.close();
})().catch(error=>{dom.window.close();throw error;});
