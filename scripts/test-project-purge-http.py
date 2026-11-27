#!/usr/bin/env python3
"""Permanent deletion in an isolated HOME/database; never touches user projects."""
import hashlib,json,os,socket,sqlite3,subprocess,tempfile,time,urllib.request,urllib.error
from pathlib import Path
with tempfile.TemporaryDirectory(prefix='keeper-project-purge-') as folder:
 root=Path(folder)
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}'
 def call(name,args=None,ok=True):
  req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base})
  try:
   with urllib.request.urlopen(req,timeout=15) as r:data=json.load(r)
   assert ok,(name,data)
   return data['result']
  except urllib.error.HTTPError as e:
   data=json.load(e);assert not ok,(name,data);return data
 def start():
  p=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'HOME':folder,'TMPDIR':folder},stdout=subprocess.DEVNULL,stderr=open(root/'server.log','ab'))
  for _ in range(100):
   try:call('agent_bootstrap');return p
   except OSError:time.sleep(.1)
  p.terminate();p.wait()
  raise AssertionError('startup failed: '+(root/'server.log').read_text()[-4000:])
 def create(name):
  p=call('agent_new_project',{'name':name});sids=[call('agent_new_session',{'projectId':p['id']})['id'] for _ in range(3)]
  path=Path(p['directory']);(path/'keep.txt').write_text(name)
  with sqlite3.connect(root/'data.sql') as db:
   db.execute('UPDATE agent_sessions SET archived_at=1 WHERE id=?',(sids[1],))
   db.execute("UPDATE agent_sessions SET deleted_at=1,owner_key='other' WHERE id=?",(sids[2],))
   db.execute("INSERT INTO agent_messages(id,session_id,role,content,created_at) VALUES (?,?,'user','fixture',0)",(sids[0],sids[0]))
   db.execute("INSERT INTO agent_model_contexts VALUES (?,'[]')",(sids[0],))
   db.execute("INSERT INTO agent_drafts VALUES (?,'draft',0)",(sids[0],))
   db.execute("INSERT INTO agent_attachments(id,owner_key,session_id,name,mime_type,kind,data,sha256,created_at) VALUES (?,'guest',?,'notes','text/plain','text',x'6162',?,0)",(sids[0],sids[0],hashlib.sha256(b'ab').hexdigest()))
   db.execute('INSERT INTO agent_message_attachments VALUES (?,?,0)',(sids[0],sids[0]))
  return p,sids
 def recycle(p):
  r=call('agent_project_delete',{'projectId':p['id'],'confirmed':True})
  return next(item for item in call('agent_project_trash_list') if item['id']==p['id'])
 def purge_args(item):return {'projectId':item['id'],'deletedAt':item['deletedAt'],'confirmed':True}
 def snapshot():return [{k:p[k] for k in ('id','deletedAt')} for p in call('agent_project_trash_list')]
 p=start()
 try:
  active,active_ids=create('Active sentinel');active_path=Path(active['directory'])
  a,aids=create('删除测试');a=recycle(a);ap=Path(a['directory'])
  # Links inside a project must never delete their external targets.
  external=root/'external';external.mkdir();(external/'sentinel').write_text('safe');(ap/'link').symlink_to(external,target_is_directory=True)
  call('agent_project_purge',{'projectId':a['id'],'deletedAt':a['deletedAt']},False)
  call('agent_project_purge',{**purge_args(a),'deletedAt':0},False)
  call('agent_project_purge',{'projectId':active['id'],'deletedAt':0,'confirmed':True},False)
  call('agent_project_purge',{'projectId':'general','deletedAt':0,'confirmed':True},False)
  assert ap.exists()
  # SQL failure must not remove files or leave an authorized cleanup job.
  with sqlite3.connect(root/'data.sql') as db:db.execute("CREATE TRIGGER reject_purge BEFORE DELETE ON agent_projects BEGIN SELECT RAISE(ABORT,'fixture'); END")
  call('agent_project_purge',purge_args(a),False)
  assert (ap/'keep.txt').exists()
  with sqlite3.connect(root/'data.sql') as db:
   assert db.execute('SELECT count(*) FROM agent_project_purges').fetchone()[0]==0
   assert db.execute('SELECT count(*) FROM agent_sessions WHERE project_id=?',(a['id'],)).fetchone()[0]==3
   db.execute('DROP TRIGGER reject_purge')
  # Reject altered top-level symlinks before touching any metadata/files.
  hidden=ap.with_name('saved');ap.rename(hidden);ap.symlink_to(external,target_is_directory=True)
  call('agent_project_purge',purge_args(a),False);assert (external/'sentinel').read_text()=='safe'
  ap.unlink();hidden.rename(ap)
  assert call('agent_project_purge',purge_args(a))['filesDeleted']
  assert not ap.exists() and (external/'sentinel').read_text()=='safe'
  with sqlite3.connect(root/'data.sql') as db:
   for table,column in [('agent_sessions','project_id'),('agent_projects','id'),('agent_project_trash','project_id'),('agent_project_purges','project_id')]:
    assert db.execute(f'SELECT count(*) FROM {table} WHERE {column}=?',(a['id'],)).fetchone()[0]==0,table
   for table,column in [('agent_messages','session_id'),('agent_drafts','session_id'),('agent_attachments','session_id')]:
    assert db.execute(f'SELECT count(*) FROM {table} WHERE {column}=?',(aids[0],)).fetchone()[0]==0,table
   assert db.execute('SELECT count(*) FROM agent_model_contexts WHERE message_id=?',(aids[0],)).fetchone()[0]==0
   assert db.execute('SELECT count(*) FROM agent_message_attachments WHERE attachment_id=?',(aids[0],)).fetchone()[0]==0
  b,bids=create('Bulk 1');b=recycle(b);stale=snapshot()
  c,cids=create('Bulk 2');c=recycle(c)
  call('agent_project_trash_empty',{'confirmed':True,'projects':stale},False)
  call('agent_project_trash_empty',{'projects':snapshot()},False)
  assert Path(b['directory']).exists() and Path(c['directory']).exists()
  with sqlite3.connect(root/'data.sql') as db:
   db.execute("CREATE TRIGGER reject_batch BEFORE DELETE ON agent_projects WHEN OLD.id='%s' BEGIN SELECT RAISE(ABORT,'fixture'); END"%c['id'])
  call('agent_project_trash_empty',{'confirmed':True,'projects':snapshot()},False)
  assert Path(b['directory']).exists() and Path(c['directory']).exists()
  with sqlite3.connect(root/'data.sql') as db:
   assert db.execute('SELECT count(*) FROM agent_project_purges').fetchone()[0]==0
   db.execute('DROP TRIGGER reject_batch')
  assert call('agent_project_trash_empty',{'confirmed':True,'projects':snapshot()})['deletedProjects']==2
  assert not Path(b['directory']).exists() and not Path(c['directory']).exists()
  assert call('agent_project_trash_list')==[]
  assert (active_path/'keep.txt').read_text()=='Active sentinel'
  # A filesystem error keeps an explicit retry item after metadata is committed.
  if hasattr(os,'geteuid') and os.geteuid()!=0:
   retry,rids=create('Retry deletion');retry=recycle(retry);rp=Path(retry['directory']);bucket=rp.parent
   bucket.chmod(0o500)
   try:
    call('agent_project_purge',purge_args(retry),False)
    item=next(x for x in call('agent_project_trash_list') if x['id']==retry['id'])
    assert item['pendingPurge']
    call('agent_project_restore',{'projectId':retry['id']},False)
    with sqlite3.connect(root/'data.sql') as db:
     assert db.execute('SELECT count(*) FROM agent_sessions WHERE project_id=?',(retry['id'],)).fetchone()[0]==0
   finally:bucket.chmod(0o700)
   assert call('agent_project_purge',purge_args(item))['filesDeleted'] and not rp.exists()
   # Startup resumes an already confirmed deletion after file cleanup was interrupted.
   job,jids=create('Interrupted deletion');job=recycle(job);jp=Path(job['directory']);bucket=jp.parent
   bucket.chmod(0o500)
   try:call('agent_project_purge',purge_args(job),False)
   finally:bucket.chmod(0o700)
   p.terminate();p.wait();p=start()
   assert not jp.exists() and call('agent_project_trash_list')==[]
  print('PASS: permanent and bulk deletion; confirmation/stale snapshot guards; SQL rollback; all owner/lifecycle cleanup; external symlink protection; active projects untouched; restart cleanup')
 finally:p.terminate();p.wait()
