const fs=require('fs'),assert=require('node:assert/strict'),{JSDOM}=require('jsdom');
const source=fs.readFileSync('ui/agent.js','utf8');
const dom=new JSDOM('<textarea id="agent-input"></textarea><main></main>',{runScripts:'outside-only'}),w=dom.window;
w.$=s=>w.document.querySelector(s);w.element=(tag,cls,text)=>{const el=w.document.createElement(tag);el.className=cls||'';if(text)el.textContent=text;return el;};
w.agent={selected:'s',identityEpoch:1,detail:{session:{}}};w.notice=()=>{};
w.eval(source.slice(0,source.indexOf('\n')));
w.terminal=true;w.reasonCode='round_limit';w.reason='轮次预算已用尽';w.runs=[{status:'succeeded'},{status:'failed'}];w.task={status:'failed'};w.labels={failed:'执行失败'};w.compact=['执行失败'];w.caption=w.element('summary','','执行失败');w.message=w.$('main');
const a=source.indexOf("            if(terminal && ['round_limit'"),b=source.indexOf('            const counts=',a);w.eval(source.slice(a,b));
const button=w.message.querySelector('.agent-continue-task');assert(button);assert.equal(w.caption.textContent,'阶段暂停');button.click();assert(w.$('#agent-input').value.includes('成功 1，失败 1'));assert(w.$('#agent-input').value.includes('不重放'));w.$('#agent-input').value='existing';button.click();assert.equal(w.$('#agent-input').value,'existing');w.$('#agent-input').value='';w.agent.identityEpoch++;button.click();assert.equal(w.$('#agent-input').value,'');w.agent.identityEpoch--;w.agent.detail.modelPending=true;button.click();assert.equal(w.$('#agent-input').value,'');dom.window.close();console.log('PASS stage continuation draft, evidence counts, existing draft and stale-account/running guards');
