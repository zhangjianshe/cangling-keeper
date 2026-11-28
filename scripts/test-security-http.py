#!/usr/bin/env python3
"""Approval modes against a fake model and disposable project. No real credentials."""
import http.server,json,os,socket,subprocess,tempfile,threading,time,urllib.request,urllib.error,sys
from pathlib import Path
from fixture_scope import respond_scope
if os.name!='posix':raise SystemExit('SKIP: POSIX command fixture')
requests=[]
class API(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  b=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
  if respond_scope(self,b):return
  requests.append(b)
  prompt=next(m['content'] for m in reversed(b['messages']) if m['role']=='user');self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
  if any(m['role']=='tool' for m in b['messages']):delta={'content':'done'};reason='stop'
  else:
   if 'command' in prompt:name,args='run_command',{'command':'printf full > ../outside.txt'}
   elif 'patch' in prompt:name,args='apply_patch',{'path':'safe.txt','oldText':'before','newText':'after'}
   elif 'escape' in prompt:name,args='write_file',{'path':'../escaped.txt','content':'bad'}
   else:name,args='write_file',{'path':'safe.txt','content':'before'}
   delta={'tool_calls':[{'index':0,'id':'fixture-call','type':'function','function':{'name':name,'arguments':json.dumps(args)}}]};reason='tool_calls'
  self.wfile.write(('data: '+json.dumps({'choices':[{'delta':delta,'finish_reason':reason}]})+'\n\ndata: [DONE]\n\n').encode());self.wfile.flush()
api=http.server.ThreadingHTTPServer(('127.0.0.1',0),API);threading.Thread(target=api.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-security-') as folder:
 sock=socket.socket();sock.bind(('127.0.0.1',0));port=sock.getsockname()[1];sock.close();base=f'http://127.0.0.1:{port}';process=None
 def call(name,args=None,expected=200,origin=None):
  req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':origin or base})
  try:
   with urllib.request.urlopen(req,timeout=10) as r:status=r.status;v=json.load(r)
  except urllib.error.HTTPError as e:status=e.code;v=json.load(e)
  assert status==expected,(name,status,v)
  return v.get('result') if status==200 else v
 def start():
  global process
  process=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'HOME':folder,'TMPDIR':folder},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  for _ in range(100):
   try:call('agent_bootstrap');return
   except Exception:time.sleep(.05)
  raise AssertionError('startup')
 def detail(s):return call('agent_session',{'sessionId':s})
 def wait(s,test):
  for _ in range(300):
   d=detail(s)
   if test(d):return d
   time.sleep(.025)
  raise AssertionError('timeout '+str(d))
 def send(s,text):call('agent_send_message',{'sessionId':s,'content':text})
 def done(s):return wait(s,lambda d:not d['modelPending'])
 def pending(s):return wait(s,lambda d:bool(d['runs']) and d['runs'][-1]['status']=='pending_approval')
 def stop(s):d=detail(s);call('agent_stop_generation',{'sessionId':s,'messageId':d['streamingMessageId']});done(s)
 try:
  start();root=Path(call('agent_bootstrap')['projects'][0]['directory']);call('agent_model_save',{'enabled':True,'baseUrl':f'http://127.0.0.1:{api.server_port}','model':'fixture','apiKey':'fixture'})
  s=call('agent_new_session')['id'];assert detail(s)['securityMode']=='ask'
  send(s,'write');d=pending(s);assert not (root/'safe.txt').exists();assert d['runs'][-1]['review']['securityMode']=='ask'
  call('agent_security_mode',{'sessionId':s,'mode':'full','confirmFullAccess':True},400);stop(s)
  call('agent_security_mode',{'sessionId':s,'mode':'invalid'},400);call('agent_security_mode',{'sessionId':s,'mode':'full'},400)
  call('agent_security_mode',{'sessionId':s,'mode':'auto'},403,origin='http://untrusted.invalid')
  call('agent_security_mode',{'sessionId':s,'mode':'auto'});send(s,'write');d=done(s);assert (root/'safe.txt').read_text()=='before';assert d['runs'][-1]['review']['approvalSource']=='policy'
  send(s,'patch');done(s);assert (root/'safe.txt').read_text()=='after'
  send(s,'command');pending(s);assert not (root.parent/'outside.txt').exists();stop(s)
  call('agent_security_mode',{'sessionId':s,'mode':'full','confirmFullAccess':True});send(s,'command');d=done(s);assert (root.parent/'outside.txt').read_text()=='full';assert d['runs'][-1]['review']['securityMode']=='full'
  send(s,'escape');d=done(s);assert d['runs'][-1]['status']=='failed' and not (root.parent/'escaped.txt').exists()
  other=call('agent_new_session')['id'];assert detail(other)['securityMode']=='ask';send(other,'write');pending(other)
  process.terminate();process.wait(10);count=len(requests);start();assert detail(s)['securityMode']=='full';assert detail(other)['securityMode']=='ask';assert not detail(other)['modelPending'];time.sleep(.1);assert len(requests)==count
  assert any('[系统执行的安全策略]\n完全访问' in r['messages'][0]['content'] for r in requests)
  if '--preview' in sys.argv:print('Security preview:',base,flush=True);input('Press Enter after browser verification: ')
  print('PASS: default manual, pinned busy policy, invalid/full confirmation/origin guards, scoped auto write/patch, commands still reviewed, full command access, file scope retained, session isolation, restart persistence/no replay, model policy context')
 finally:
  if process and process.poll() is None:process.terminate();process.wait(10)
  api.shutdown();api.server_close()
