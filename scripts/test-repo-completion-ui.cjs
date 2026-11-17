const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const source=fs.readFileSync('ui/main.js','utf8');
const code=source.slice(source.indexOf('async function onRepoCloneUpdateClick()'),source.indexOf('// ---- certificate dropdown'));
async function scenario(fail){
 let releaseRefresh;const refresh=new Promise(r=>releaseRefresh=r);let hidden=false;
 const state={selectedSetName:'cangling-repo',repoSyncing:false,repoStatus:{cloned:false}};
 const c=vm.createContext({state,selectedSetKind:()=> 'git',uiAlert:()=>{},renderRepoProgress:()=>hidden=false,hideRepoProgress:()=>hidden=true,renderRepoStatus:()=>{},invoke:async()=>{if(fail)throw new Error('LFS unavailable');return {cloned:true};},loadSoftwareSets:()=>refresh,loadRepoStatus:async()=>{},renderSetList:()=>{},loadRepoDir:async()=>true,showRepoUnsynced:()=>{}});
 vm.runInContext(code,c);const running=c.onRepoCloneUpdateClick();
 await new Promise(r=>setImmediate(r));
 assert.equal(hidden,true,'Progress must be hidden before slow metadata refresh finishes');
 assert.equal(state.repoSyncing,false);
 releaseRefresh();await running;assert.equal(hidden,true);
}
(async()=>{await scenario(false);await scenario(true);console.log('Repository UI: success/error close progress before metadata refresh');})().catch(e=>{console.error(e);process.exitCode=1});
