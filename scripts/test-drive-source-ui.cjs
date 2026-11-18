const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const source=fs.readFileSync('ui/main.js','utf8');
const labels=source.slice(source.indexOf('function setKindLabel('),source.indexOf('function setListSub('));
const sync=source.slice(source.indexOf('async function onRepoCloneUpdateClick()'),source.indexOf('// ---- certificate dropdown'));
(async()=>{
 let called=false;const notices=[];const state={selectedSetName:'cangling-repo',repoSyncing:false,repoStatus:{cloned:false}};
 const context=vm.createContext({state,selectedSetKind:()=> 'drive',uiAlert:message=>notices.push(message),renderRepoProgress:()=>{},hideRepoProgress:()=>{},renderRepoStatus:()=>{},invoke:async(method,args)=>{assert.equal(method,'sync_software_set');assert.equal(args.setName,'cangling-repo');called=true;return {cloned:true,kind:'drive'};},loadSoftwareSets:async()=>{},loadRepoStatus:async()=>{},renderSetList:()=>{},loadRepoDir:async()=>true,showRepoUnsynced:()=>{}});
 vm.runInContext(labels+sync,context);assert.equal(context.setKindLabel('drive'),'网盘');await context.onRepoCloneUpdateClick();assert.equal(called,true);assert.deepEqual(notices,[]);assert.equal(state.repoSyncing,false);
 console.log('Drive UI can synchronize without a maintenance-center login');
})().catch(e=>{console.error(e);process.exitCode=1;});
