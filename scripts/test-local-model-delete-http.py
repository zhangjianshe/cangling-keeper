#!/usr/bin/env python3
"""Exercise local model deletion using a disposable Keeper data directory."""
import json,os,socket,sqlite3,subprocess,tempfile,time,urllib.request,urllib.error
from pathlib import Path
with tempfile.TemporaryDirectory(prefix='keeper-model-delete-') as folder:
 sock=socket.socket();sock.bind(('127.0.0.1',0));port=sock.getsockname()[1];sock.close();base=f'http://127.0.0.1:{port}'
 process=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'HOME':folder,'TMPDIR':folder},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 def call(name,args=None,expected=200):
  req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base})
  try:
   with urllib.request.urlopen(req,timeout=10) as response:status=response.status;data=json.load(response)
  except urllib.error.HTTPError as error:status=error.code;data=json.load(error)
  assert status==expected,(name,status,data);return data.get('result')
 try:
  for _ in range(100):
   try:call('agent_bootstrap');break
   except Exception:time.sleep(.05)
  def add(name,key):return call('agent_model_save',{'name':name,'provider':'deepseek','baseUrl':'http://127.0.0.1:9999','model':'fixture','apiKey':key,'enabled':True})['savedServerId']
  first=add('First','fixture-key-one');second=add('Second','fixture-key-two');call('agent_model_select',{'id':first})
  session=call('agent_new_session')['id'];call('agent_session_model',{'sessionId':session,'id':first})
  dbpath=str(Path(folder)/'data.sql')
  with sqlite3.connect(dbpath) as db:
   db.execute("INSERT INTO settings(key,value) VALUES ('agent_llm_active:other',?)",(first,))
   db.execute("INSERT INTO agent_messages(id,session_id,role,content,created_at,model_metadata) VALUES ('pending-fixture',?,'assistant','partial',1,?)",(session,json.dumps({'serverId':first})))
   db.execute("INSERT INTO agent_model_pending(session_id,message_id) VALUES (?,'pending-fixture')",(session,))
  call('agent_model_delete',{'id':first},expected=400)
  with sqlite3.connect(dbpath) as db:db.execute('DELETE FROM agent_model_pending WHERE session_id=?',(session,))
  call('agent_model_delete',{'id':'cis-model:fixture'},expected=400)
  config=call('agent_model_delete',{'id':first});assert config['activeServerId']=='' and not any(s['id']==first for s in config['servers']);assert any(s['id']==second for s in config['servers'])
  detail=call('agent_session',{'sessionId':session});assert detail['model']['activeServerId']==first and not detail['model']['enabled'];assert any(m['content']=='partial' for m in detail['messages'])
  call('agent_send_message',{'sessionId':session,'content':'should require model'},expected=400)
  with sqlite3.connect(dbpath) as db:
   assert db.execute('SELECT COUNT(*) FROM agent_llm_servers WHERE id=?',(first,)).fetchone()[0]==0
   assert db.execute("SELECT value FROM settings WHERE key='agent_llm_active:other'").fetchone()[0]==''
   assert db.execute('SELECT api_key FROM agent_llm_servers WHERE id=?',(second,)).fetchone()[0]=='fixture-key-two'
  call('agent_model_delete',{'id':first},expected=400)
  replacement=add('Replacement','fixture-key-three');assert call('agent_model_config')['activeServerId']==replacement
  assert call('agent_session',{'sessionId':session})['model']['activeServerId']==first
  print('PASS: local delete, pending guard, default cleanup across accounts, no session fallback, history and other models preserved')
 finally:
  process.terminate();process.wait(10)
