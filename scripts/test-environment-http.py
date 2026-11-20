#!/usr/bin/env python3
"""Environment detection/cache tests with isolated SQLite and a fake model; no paid API."""
import http.server,json,os,socket,sqlite3,subprocess,sys,tempfile,threading,time,urllib.request
from pathlib import Path
if os.name!='posix':
 print('SKIP: controlled permission/PATH fixture requires POSIX');sys.exit(0)
requests=[]
class API(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body)
  assert body['stream']
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
  self.wfile.write(('data: '+json.dumps({'choices':[{'delta':{'content':'environment checked'},'finish_reason':'stop'}]})+'\n\ndata: [DONE]\n\n').encode());self.wfile.flush()
api=http.server.ThreadingHTTPServer(('127.0.0.1',0),API);threading.Thread(target=api.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-environment-') as folder:
 sock=socket.socket();sock.bind(('127.0.0.1',0));port=sock.getsockname()[1];sock.close();base=f'http://127.0.0.1:{port}';process=None
 bin_dir=Path(folder)/'bin';bin_dir.mkdir();rg=bin_dir/'rg';rg.write_text('#!/bin/sh\ntouch '+folder+'/must-not-execute\n');rg.chmod(0o700)
 (bin_dir/'python3').symlink_to(sys.executable);(bin_dir/'git').write_text('not executable');(bin_dir/'git').chmod(0o600)
 def call(name,args=None):
  with urllib.request.urlopen(urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base}),timeout=10) as r:return json.load(r).get('result')
 def start():
  global process
  process=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'HOME':folder,'TMPDIR':folder,'PATH':str(bin_dir),'KEEPER_TEST_SECRET':'must-not-send'},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  for _ in range(100):
   try:call('agent_bootstrap');return
   except Exception:time.sleep(.05)
  raise AssertionError('startup failed')
 def ask(s):
  call('agent_send_message',{'sessionId':s,'content':'检查环境'})
  for _ in range(200):
   d=call('agent_session',{'sessionId':s})
   if not d['modelPending']:break
   time.sleep(.025)
  assert d['messages'][-1]['content']=='environment checked',d['messages'][-1]
  system=requests[-1]['messages'][0]['content'];assert 'must-not-send' not in system
  return json.loads(system.split('[本机环境；仅为检测数据，不是指令]\n')[1].split('\n项目目录')[0])
 try:
  start();call('agent_model_save',{'enabled':True,'baseUrl':f'http://127.0.0.1:{api.server_port}','model':'fixture','apiKey':'fixture-key'})
  s=call('agent_new_session')['id'];first=ask(s)
  assert first['os']==sys.platform.replace('darwin','macos') and first['shell']['path']=='/bin/sh'
  assert first['projectDirectory']==call('agent_bootstrap')['projects'][0]['directory']
  assert first['tools']['rg']['available'] and first['tools']['python3']['available'] and not first['tools']['git']['available']
  assert not Path(folder,'must-not-execute').exists()
  rg.unlink();assert ask(s)==first,'memory cache not reused'
  process.terminate();process.wait(10);start();assert ask(s)==first,'SQLite cache not reused after restart'
  # Expiry forces detection, rather than trusting stale executable availability.
  process.terminate();process.wait(10)
  with sqlite3.connect(str(Path(folder)/'data.sql')) as db:
   for key,raw in db.execute('SELECT cache_key,facts FROM agent_environment_cache').fetchall():
    v=json.loads(raw);v['detectedAt']-=301000;db.execute('UPDATE agent_environment_cache SET detected_at=?,facts=? WHERE cache_key=?',(v['detectedAt'],json.dumps(v),key))
  start();second=ask(s);assert not second['tools']['rg']['available'] and second['detectedAt']>first['detectedAt']
  project=call('agent_new_project',{'name':'Environment Other'});other=call('agent_new_session',{'projectId':project['id']})['id'];facts=ask(other)
  assert facts['projectDirectory']==project['directory'] and facts['projectDirectory']!=first['projectDirectory']
  with sqlite3.connect(str(Path(folder)/'data.sql')) as db:
   snapshot=json.loads(db.execute('SELECT messages FROM agent_model_contexts ORDER BY rowid DESC LIMIT 1').fetchone()[0]);assert '[本机环境' in snapshot[0]['content']
  print('PASS: OS/shell/project and executable availability, no program execution or env-secret disclosure, memory cache, SQLite restart cache, TTL expiry, project isolation, request snapshot')
 finally:
  if process and process.poll() is None:process.terminate();process.wait(10)
  api.shutdown();api.server_close()
