#!/usr/bin/env python3
"""Check per-model output settings, persistence, request/reserve agreement and truncation safety."""
import http.server,json,os,socket,subprocess,tempfile,threading,time,urllib.request,urllib.error
from pathlib import Path
seen=[]
class API(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));seen.append(body)
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
  cutoff=any(m.get('content')=='cutoff' for m in body['messages'])
  delta={'tool_calls':[{'index':0,'id':'cut','type':'function','function':{'name':'write_file','arguments':'{"path":"bad.txt","content":"'}}]} if cutoff else {'content':'ok'}
  self.wfile.write(('data: '+json.dumps({'choices':[{'delta':delta,'finish_reason':'length' if cutoff else 'stop'}]})+'\n\ndata: [DONE]\n\n').encode())
api=http.server.ThreadingHTTPServer(('127.0.0.1',0),API);threading.Thread(target=api.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-output-') as folder:
 sock=socket.socket();sock.bind(('127.0.0.1',0));port=sock.getsockname()[1];sock.close();base=f'http://127.0.0.1:{port}'
 def call(name,args={}):
  r=urllib.request.urlopen(urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args).encode(),headers={'Content-Type':'application/json','Origin':base}),timeout=15)
  d=json.load(r);assert d['ok'],d;return d['result']
 def start():
  p=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'HOME':folder,'TMPDIR':folder},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  for _ in range(120):
   try:call('agent_bootstrap');return p
   except Exception:time.sleep(.05)
  raise AssertionError('startup')
 def send(s,prompt):
  call('agent_send_message',{'sessionId':s,'content':prompt})
  for _ in range(200):
   d=call('agent_session',{'sessionId':s})
   if not d['modelPending']:return d
   time.sleep(.03)
  raise AssertionError('generation timeout')
 p=start()
 try:
  def add(name,provider):return call('agent_model_save',{'name':name,'provider':provider,'baseUrl':f'http://127.0.0.1:{api.server_port}','model':'fixture','apiKey':'fixture','enabled':True,'contextLimit':131072})['savedServerId']
  a=add('DeepSeek','deepseek');b=add('Compatible','openai-compatible')
  def configs():return {m['id']:m for m in call('agent_model_config')['servers']}
  assert configs()[a]['maxOutputTokens']==16384 and configs()[b]['maxOutputTokens']==4096
  for value in [0,1023,65537,1.5,'8192']:
   try:call('agent_model_output_budget',{'id':a,'maxOutputTokens':value});raise AssertionError('accepted invalid budget')
   except urllib.error.HTTPError as e:assert e.code==400
  call('agent_model_output_budget',{'id':a,'maxOutputTokens':32768})
  assert configs()[a]['maxOutputTokens']==32768 and configs()[b]['maxOutputTokens']==4096
  p.terminate();p.wait(10);p=start();assert configs()[a]['maxOutputTokens']==32768
  s=call('agent_new_session')['id'];call('agent_session_model',{'sessionId':s,'id':a})
  d=send(s,'hello');meta=d['messages'][-1]['modelMetadata'];assert meta['context']['outputReserved']==32768==seen[-1]['max_tokens']==meta['maxOutputTokens']
  call('agent_context_budget',{'sessionId':s,'workingBudget':16384})
  d=send(s,'small budget');assert seen[-1]['max_tokens']==4096==d['messages'][-1]['modelMetadata']['context']['outputReserved']
  d=send(s,'cutoff');assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='output_limit' for m in d['messages']);assert not d['runs'],'truncated tool was executed'
  print('PASS: defaults, independent models, validation, restart persistence, request/reserve equality, window clamp, truncated tool blocked')
 finally:p.terminate();p.wait(10);api.shutdown()
