const assert=require('node:assert/strict'),{JSDOM}=require('jsdom');
const dom=new JSDOM('<!doctype html><body></body>');global.window=dom.window;global.document=dom.window.document;
dom.window.HTMLDialogElement.prototype.showModal=function(){this.open=true};dom.window.HTMLDialogElement.prototype.close=function(){this.dispatchEvent(new dom.window.Event('close'))};
require('../ui/project-management.js');
const tick=()=>new Promise(setImmediate),click=async name=>{[...document.querySelectorAll('button')].find(b=>b.textContent===name&&!b.disabled).click();await tick();};
const projects=[{id:'p',name:'<项目>',directory:'/tmp/.recycle-bin/p/项目',deletedAt:10},{id:'q',name:'另一项目',directory:'/tmp/.recycle-bin/q/另一项目',deletedAt:20}];
(async()=>{
 let items=projects.slice(),calls=[],refreshes=0,fail=true;
 await window.keeperProjectTrash({invoke:async(name,args)=>{
  if(name==='agent_project_trash_list')return items;
  calls.push([name,args]);
  if(name==='agent_project_restore'){
   if(fail){fail=false;throw new Error('目标目录不可写');}items=items.filter(p=>p.id!==args.projectId);
  }else if(name==='agent_project_purge')items=items.filter(p=>p.id!==args.projectId);
  else if(name==='agent_project_trash_empty')items=[];
 },notice:()=>{},onChanged:async()=>refreshes++});
 assert.equal(document.querySelector('strong').textContent,'<项目>');assert(!document.querySelector('项目'));
 let button=document.querySelector('[aria-label="恢复项目 <项目>"]');button.click();await tick();
 assert.match(document.querySelector('[role=status]').textContent,/目标目录不可写/);assert.equal(refreshes,0);
 button=document.querySelector('[aria-label="恢复项目 <项目>"]');assert(!button.disabled);
 // Both destructive actions require a separate dialog; cancel never invokes the API.
 await click('彻底删除');assert.equal(document.querySelectorAll('dialog').length,2);assert.match(document.querySelectorAll('dialog')[1].textContent,/全部会话.*磁盘目录.*无法撤销/);
 assert.equal(document.activeElement.textContent,'取消');await click('取消');assert.equal(calls.length,1);
 await click('彻底删除');await click('确认彻底删除');
 assert.deepEqual(calls.at(-1),['agent_project_purge',{projectId:'p',deletedAt:10,confirmed:true}]);assert.equal(refreshes,1);
 await click('清空回收站');assert.match(document.querySelectorAll('dialog')[1].textContent,/1 个项目/);await click('取消');assert.equal(calls.length,2);
 await click('清空回收站');await click('确认清空回收站');
 assert.deepEqual(calls.at(-1),['agent_project_trash_empty',{confirmed:true,projects:[{id:'q',deletedAt:20}]}]);assert.match(document.body.textContent,/回收站为空/);
 assert([...document.querySelectorAll('button')].find(b=>b.textContent==='清空回收站').disabled);await click('关闭');assert(!document.querySelector('dialog'));
 await window.keeperProjectTrash({invoke:async name=>name==='agent_project_trash_list'?[{...projects[0],pendingPurge:true}]:null,notice(){},onChanged(){}});
 assert(document.querySelector('[aria-label="恢复项目 <项目>"]').disabled);assert.match(document.body.textContent,/记录已删除.*磁盘文件.*重试/);await click('关闭');
 console.log('PASS: permanent/bulk confirmation and cancellation; snapshots; safe names; failure refresh; pending cleanup retry state; empty protection');
})().catch(e=>{console.error(e);process.exitCode=1});
