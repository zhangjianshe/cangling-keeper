const assert=require('node:assert/strict');
const {JSDOM}=require('jsdom');
const dom=new JSDOM('<textarea id="agent-input"></textarea>');
global.window=dom.window;global.document=dom.window.document;
require('../ui/drafts.js');
(async()=>{
 const agent={identityEpoch:0},records=new Map(),input=document.querySelector('textarea');let delayed=null;
 const invoke=async(name,args)=>{
  if(delayed&&name==='agent_draft_get'&&args.sessionId==='slow')await delayed;
  const row=records.get(args.sessionId)||{content:'',revision:0};
  if(name==='agent_draft_get')return {...row};
  if(row.revision!==args.revision)throw Error('conflict');
  const next={content:args.content,revision:row.revision+1};records.set(args.sessionId,next);return {...next};
 };
 const drafts=window.createKeeperDrafts({agent,invoke,notice:()=>{},ensureSession:async()=>'a'});
 await drafts.load('a');input.value='draft A';await drafts.flush();await drafts.load('b');input.value='draft B';await drafts.flush();await drafts.load('a');assert.equal(input.value,'draft A');
 await assert.rejects(drafts.send(async()=>{throw Error('network')}));assert.equal(input.value,'draft A');
 await drafts.send(async revision=>{const row=records.get('a');assert.equal(revision,row.revision);records.set('a',{content:'',revision:revision+1});});assert.equal(input.value,'');
 let release;delayed=new Promise(r=>release=r);const old=drafts.load('slow');await new Promise(r=>setImmediate(r));await drafts.load('b');release();assert.equal(await old,false);assert.equal(input.value,'draft B');
 records.set('b',{content:'other tab',revision:9});input.value='my unsaved text';await assert.rejects(drafts.flush());await assert.rejects(drafts.load('a'));assert.equal(input.value,'my unsaved text');assert.equal(records.get('b').content,'other tab');
 agent.identityEpoch++;drafts.reset();await drafts.load('a');assert.equal(input.value,'');
 console.log('PASS: draft switching, send failure/success, stale navigation, tab conflict and account reset');
})().catch(e=>{console.error(e);process.exitCode=1});
