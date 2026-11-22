#!/usr/bin/env python3
"""Disposable account-scoped history backup/restore regression; no external calls."""
import copy,hashlib,json,os,socket,sqlite3,subprocess,tempfile,time,urllib.request,urllib.error
from pathlib import Path
with tempfile.TemporaryDirectory(prefix='keeper-history-test-') as folder:
 root=Path(folder)
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}';proc=None;log=open(root/'server.log','wb')
 def call(name,args=None,expected=200,origin=None,path=None):
  req=urllib.request.Request(base+(path or '/api/invoke/'+name),data=json.dumps(args or {},ensure_ascii=False).encode(),headers={'Content-Type':'application/json','Origin':origin or base})
  try:
   with urllib.request.urlopen(req,timeout=30) as r:status=r.status;data=json.load(r)
  except urllib.error.HTTPError as e:status=e.code;data=json.load(e)
  assert status==expected,(name,status,data);return data.get('result') if status==200 else data
 def restore(args,expected=200,origin=None):return call('restore',args,expected,origin,'/api/history/restore')
 def identity(user):
  with sqlite3.connect(root/'data.sql') as db:
   for k,v in {'server_url':'https://fixture.invalid/ib','login_token':'login-private-fixture','login_user_id':str(user),'login_username':'alice' if user==1 else 'bob'}.items():db.execute('INSERT OR REPLACE INTO settings(key,value) VALUES (?,?)',(k,v))
 def totals():
  with sqlite3.connect(root/'data.sql') as db:return tuple(db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ('agent_sessions','agent_messages','agent_runs','agent_projects'))
 def signed(archive):
  a=copy.deepcopy(archive);a['sha256']=hashlib.sha256(json.dumps(a['snapshot'],ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()).hexdigest();return a
 def apply(archive):
  preview=restore({'mode':'preview','backup':archive});return restore({'mode':'restore','backup':archive,'confirmed':True,'expectedAccount':preview['accountKey']})
 try:
  proc=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'TMPDIR':folder},stdout=log,stderr=log)
  for _ in range(100):
   try:call('agent_bootstrap');break
   except Exception:time.sleep(.05)
  identity(1)
  project=call('agent_new_project',{'name':'备份测试项目'})['id'];s=call('agent_new_session',{'projectId':project})['id']
  call('agent_model_save',{'name':'Private fixture','apiKey':'model-private-fixture','baseUrl':'https://model.invalid','model':'fixture','enabled':True})
  with sqlite3.connect(root/'data.sql') as db:
   owner=db.execute('SELECT owner_key FROM agent_sessions WHERE id=?',(s,)).fetchone()[0]
   messages=[('u1','user','original',{}),('a1','assistant','answer one',{'source':'llm','serverId':'original-model','requestedModel':'fixture','model':'fixture','generationStatus':'complete'}),('u2','user','edited',{'editOf':'u1'}),('a2','assistant','answer two',{'source':'llm','serverId':'original-model','requestedModel':'fixture','model':'fixture','generationStatus':'streaming','regenerateOf':'a1','promptMessageId':'u2'})]
   for i,(mid,role,content,meta) in enumerate(messages):db.execute('INSERT INTO agent_messages(id,session_id,role,content,created_at,model_metadata) VALUES (?,?,?,?,?,?)',(mid,s,role,content,100+i,json.dumps(meta)))
   db.execute('INSERT INTO agent_reply_versions VALUES (?,?,?)',(s,'u1','u2'));db.execute('INSERT INTO agent_reply_versions VALUES (?,?,?)',(s,'a1','a2'))
   db.execute('INSERT INTO agent_model_contexts VALUES (?,?)',('a2',json.dumps([{'role':'system','content':'fixture system'},{'role':'user','content':'edited'}])))
   for i,status in enumerate(('succeeded','pending_approval','running','approved')):
    rid='r'+str(i);review=json.dumps({'kind':'command','command':'touch NEVER_EXECUTE','hash':'old'})
    db.execute('INSERT INTO agent_runs(id,session_id,tool,arguments,status,output,created_at,updated_at,response_id,tool_call_id,step) VALUES (?,?,?,?,?,?,?,?,?,?,?)',(rid,s,'run_command','{"command":"touch NEVER_EXECUTE"}',status,'saved output',200+i,200+i,'a2','call'+str(i),i))
    db.execute('INSERT INTO agent_reviews VALUES (?,?,?)',(rid,review,hashlib.sha256(review.encode()).hexdigest()))
  identity(2);foreign=call('agent_new_session')['id']
  with sqlite3.connect(root/'data.sql') as db:db.execute('INSERT INTO agent_messages(id,session_id,role,content,created_at) VALUES (?,?,?,?,?)',('foreign',foreign,'user','BOB-PRIVATE',1))
  identity(1);backup=call('agent_history_backup');text=json.dumps(backup,ensure_ascii=False);assert 'BOB-PRIVATE' not in text and 'model-private-fixture' not in text and 'login-private-fixture' not in text
  assert len(backup['snapshot']['sessions'])==1 and len(backup['snapshot']['projects'])==1
  before=totals();preview=restore({'mode':'preview','backup':backup});assert totals()==before and preview['messages']==4 and preview['runs']==4
  restore({'mode':'restore','backup':backup,'confirmed':True,'expectedAccount':'wrong'},400);assert totals()==before
  restore({'mode':'preview','backup':backup},403,'http://evil.invalid');call('agent_history_backup',expected=403,origin='http://evil.invalid')
  with sqlite3.connect(root/'data.sql') as db:db.execute('INSERT INTO agent_model_pending(session_id,message_id) VALUES (?,?)',(s,'a2'))
  restore({'mode':'restore','backup':backup,'confirmed':True,'expectedAccount':preview['accountKey']},400)
  with sqlite3.connect(root/'data.sql') as db:db.execute('DELETE FROM agent_model_pending WHERE session_id=?',(s,))
  result=apply(backup);new=result['sessionIds'][0];assert new!=s
  detail=call('agent_session',{'sessionId':new});assert detail['securityMode']=='ask' and not detail['modelPending'];assert [m['content'] for m in detail['messages']]==['original','answer one','edited','answer two']
  m=detail['messages'];assert m[2]['modelMetadata']['editOf']==m[0]['id'] and m[3]['modelMetadata']['regenerateOf']==m[1]['id'] and m[3]['modelMetadata']['promptMessageId']==m[2]['id'];assert m[3]['modelMetadata']['generationStatus']=='interrupted'
  assert any(g['selectedId']==m[2]['id'] for g in detail['replyGroups']) and any(g['selectedId']==m[3]['id'] for g in detail['replyGroups'])
  assert [r['status'] for r in detail['runs']]==['succeeded','interrupted','interrupted','interrupted'];assert all(r['responseId']==m[3]['id'] for r in detail['runs'])
  r=detail['runs'][1];call('agent_review_tool',{'sessionId':new,'runId':r['id'],'reviewHash':r['review']['hash'],'approved':True},400);assert not (Path(call('agent_bootstrap')['projects'][0]['directory'])/'NEVER_EXECUTE').exists()
  assert call('agent_session',{'sessionId':s})['messages'][3]['modelMetadata']['generationStatus']=='streaming','original history was modified'
  before=totals();bad=copy.deepcopy(backup);bad['snapshot']['sessions'][0]['title']='tampered';restore({'mode':'preview','backup':bad},400);assert totals()==before
  bad=copy.deepcopy(backup);bad['snapshot']['sessions'][0]['messages'][2]['metadata']['editOf']='foreign';restore({'mode':'preview','backup':signed(bad)},400);assert totals()==before
  bad=copy.deepcopy(backup);bad['snapshot']['sessions'][0]['messages'][0]['metadata']['regenerateOf']='foreign';restore({'mode':'preview','backup':signed(bad)},400);assert totals()==before
  # A storage failure after a new project/session is inserted must roll back all rows and empty directories.
  bad=copy.deepcopy(backup);bad['snapshot']['projects'][0]['id']='different-project';bad['snapshot']['sessions'][0]['projectId']='different-project';bad['snapshot']['sessions'][0]['messages'][0]['content']='trigger rollback';bad=signed(bad)
  with sqlite3.connect(root/'data.sql') as db:db.execute("CREATE TRIGGER fixture_failure BEFORE INSERT ON agent_messages WHEN NEW.content='trigger rollback' BEGIN SELECT RAISE(ABORT,'fixture rollback'); END")
  dirs=set((Path(call('agent_bootstrap')['projects'][0]['directory']).parent).iterdir());preview_bad=restore({'mode':'preview','backup':bad});restore({'mode':'restore','backup':bad,'confirmed':True,'expectedAccount':preview_bad['accountKey']},400);assert totals()==before and set((Path(call('agent_bootstrap')['projects'][0]['directory']).parent).iterdir())==dirs
  with sqlite3.connect(root/'data.sql') as db:db.execute('DROP TRIGGER fixture_failure')
  # Backups larger than the ordinary invoke JSON limit still restore through the dedicated bounded route.
  large=copy.deepcopy(backup);large['snapshot']['sessions'][0]['messages'][0]['content']='x'*(2100*1024);large=signed(large);assert apply(large)['sessions']==1
  identity(2);preview=restore({'mode':'preview','backup':backup});assert preview['differentAccount'];result=apply(backup);assert call('agent_session',{'sessionId':result['sessionIds'][0]})['messages'][0]['content']=='original';call('agent_session',{'sessionId':s},400)
  exported=call('agent_history_backup');assert all(item['id']!=s for item in exported['snapshot']['sessions'])
  print('PASS: account-only backup; credentials excluded; checksum/references/origin validation; additive restore; versions/context/run remapping; interrupted operations never replay; busy/account guards; transactional rollback and directory cleanup; >2 MiB restore.')
 finally:
  if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=10)
  log.close()
