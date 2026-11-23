const assert=require('node:assert/strict'),fs=require('node:fs'),{JSDOM}=require('jsdom');
const dom=new JSDOM('<div class="agent-header-tools"></div>',{runScripts:'outside-only'}),w=dom.window,d=w.document;
let opened;const project={id:'p'},agent={selected:'s',projects:[project],detail:{session:{projectId:'p'}},projectBrowser:{openGuidance:async p=>{opened=p;}}};
w.eval(fs.readFileSync('ui/project-guidance.js','utf8'));
const ui=w.createKeeperGuidance({agent,element:(tag,cls,text)=>{const n=d.createElement(tag);n.className=cls;n.textContent=text;return n;}});
(async()=>{assert(d.querySelector('button').disabled);ui.update();await ui.open();assert.equal(opened,project);assert.equal(d.querySelector('[role=dialog]'),null);agent.selected=null;ui.update();assert(d.querySelector('button').disabled);opened=null;await ui.open();assert.equal(opened,null);ui.reset();dom.window.close();console.log('PASS guidance navigates to project page and requires selected session');})().catch(e=>{console.error(e);process.exitCode=1;dom.window.close();});
