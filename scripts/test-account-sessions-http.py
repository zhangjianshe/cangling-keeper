#!/usr/bin/env python3
"""Account-scoped local conversations using a disposable CIS/model API and SQLite."""
import http.server,json,os,socket,sqlite3,subprocess,tempfile,threading,time,urllib.request,urllib.error
from pathlib import Path
class CIS(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_GET(self):self.reply({'success':True,'code':200,'data':[]})
 def reply(self,v):self.send_response(200);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(json.dumps(v).encode())
 def do_POST(self):
  b=json.loads(self.rfile.read(int(self.headers.get('Content-Length',0))) or b'{}')
  if self.path.endswith('/keeper/login'):
   name=b['userName'];self.reply({'success':True,'code':200,'data':{'token':'fixture-'+name,'userName':name,'nickName':name,'userId':1 if name=='alice' else 2}})
  elif self.path.endswith('/chat/completions'):
   self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers();delta={'tool_calls':[{'index':0,'id':'pending-owner','type':'function','function':{'name':'write_file','arguments':json.dumps({'path':'must-not-write.txt','content':'not approved'})}}]};self.wfile.write(('data: '+json.dumps({'choices':[{'delta':delta,'finish_reason':'tool_calls'}]})+'\n\ndata: [DONE]\n\n').encode());self.wfile.flush()
  else:self.reply({'success':True,'code':200,'data':[]})
api=http.server.ThreadingHTTPServer(('127.0.0.1',0),CIS);threading.Thread(target=api.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-account-') as folder:
 sock=socket.socket();sock.bind(('127.0.0.1',0));port=sock.getsockname()[1];sock.close();base=f'http://127.0.0.1:{port}';process=None
 def call(name,args=None,expected=200):
  try:
   with urllib.request.urlopen(urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base}),timeout=15) as r:status=r.status;v=json.load(r)
  except urllib.error.HTTPError as e:status=e.code;v=json.load(e)
  assert status==expected,(name,status,v)
  return v.get('result')
 def start():
  global process
  process=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'HOME':folder,'TMPDIR':folder},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  for _ in range(100):
   try:call('agent_bootstrap');return
   except Exception:time.sleep(.05)
  raise AssertionError('startup')
 def login(name,path='ib'):call('login',{'serverUrl':f'http://127.0.0.1:{api.server_port}/{path}','username':name,'password':'dummy'})
 def ids():return [s['id'] for s in call('agent_bootstrap')['sessions']]
 try:
  start();guest=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':guest,'content':'guest history'})
  login('alice');assert guest not in ids();a=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':a,'content':'Alice original history'});key=call('agent_bootstrap')['accountKey']
  login('bob');assert a not in ids();b=call('agent_new_session')['id']
  for name,args in [('agent_session',{}),('agent_send_message',{'content':'bad'}),('agent_rename_session',{'title':'bad'}),('agent_pin_session',{'pinned':True}),('agent_security_mode',{'mode':'full','confirmFullAccess':True}),('agent_run_tool',{'tool':'proxy_status','arguments':{}})]:call(name,{'sessionId':a,**args},400)
  login('alice','other');assert a not in ids();login('alice');assert a in ids() and b not in ids();assert call('agent_bootstrap')['accountKey']==key
  call('agent_rename_session',{'sessionId':a,'title':'Alice private title'});config=call('agent_model_save',{'enabled':True,'baseUrl':f'http://127.0.0.1:{api.server_port}','model':'fixture','apiKey':'fixture'})
  call('agent_session_model',{'sessionId':a,'id':config['activeServerId']})
  call('agent_send_message',{'sessionId':a,'content':'pending action'})
  for _ in range(100):
   d=call('agent_session',{'sessionId':a})
   if d['runs'] and d['runs'][-1]['status']=='pending_approval':break
   time.sleep(.03)
  assert d['runs'][-1]['status']=='pending_approval';run=d['runs'][-1]
  login('bob');call('agent_review_tool',{'sessionId':a,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True},400)
  for _ in range(100):
   with sqlite3.connect(str(Path(folder)/'data.sql')) as db:pending=db.execute('SELECT count(*) FROM agent_model_pending').fetchone()[0]
   if not pending:break
   time.sleep(.03)
  assert not pending;assert not Path(folder,'cangling/projects/general/must-not-write.txt').exists()
  call('logout');assert guest in ids() and a not in ids() and b not in ids()
  login('alice');process.terminate();process.wait(10);start();assert a in ids() and b not in ids();assert next(s for s in call('agent_bootstrap')['sessions'] if s['id']==a)['title']=='Alice private title'
  # Simulate a pre-ownership schema, then migrate all records to the current account without deleting history.
  process.terminate();process.wait(10)
  with sqlite3.connect(str(Path(folder)/'data.sql')) as db:count=db.execute('SELECT count(*) FROM agent_messages').fetchone()[0];db.execute('DROP INDEX idx_agent_sessions_owner');db.execute('ALTER TABLE agent_sessions DROP COLUMN owner_key')
  start()
  with sqlite3.connect(str(Path(folder)/'data.sql')) as db:assert db.execute('SELECT count(*) FROM agent_messages').fetchone()[0]==count;assert db.execute('SELECT count(DISTINCT owner_key) FROM agent_sessions').fetchone()[0]==1
  print('PASS: server/user isolation, guest separation, direct-ID operation guards, stable identity, login cancels pending action, restart and legacy migration preserve history')
 finally:
  if process and process.poll() is None:process.terminate();process.wait(10)
  api.shutdown();api.server_close()
