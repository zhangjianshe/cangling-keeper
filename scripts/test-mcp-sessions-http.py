#!/usr/bin/env python3
"""MCP stateful subprocess reuse, isolation, cleanup and no-replay tests."""
import http.server,json,os,socket,subprocess,sys,tempfile,threading,time,urllib.request,urllib.error
from pathlib import Path
class Model(http.server.BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def do_POST(self):
  data=json.loads(self.rfile.read(int(self.headers['Content-Length'])));last=max(i for i,m in enumerate(data['messages']) if m['role']=='user');prompt=data['messages'][last]['content'];results=[m for m in data['messages'][last+1:] if m['role']=='tool']
  if results:delta={'content':results[-1]['content']};reason='stop'
  else:
   name=next(t['function']['name'] for t in data['tools'] if t['function']['name'].startswith('mcp_'));delta={'tool_calls':[{'index':0,'id':'stateful-call','type':'function','function':{'name':name,'arguments':json.dumps({'mode':prompt})}}]};reason='tool_calls'
  body=('data: '+json.dumps({'choices':[{'delta':delta,'finish_reason':reason}]})+'\n\ndata: [DONE]\n\n').encode();self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers();self.wfile.write(body)
model=http.server.ThreadingHTTPServer(('127.0.0.1',0),Model);model.daemon_threads=True;threading.Thread(target=model.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-mcp-session-') as folder:
 root=Path(folder);script=root/'mcp.py';logfile=root/'calls.jsonl'
 script.write_text('''import os,sys,json,time
counter=0
for line in sys.stdin:
 r=json.loads(line);method=r.get('method');ident=r.get('id')
 if ident is None:continue
 if method=='initialize':result={'protocolVersion':'2025-06-18','capabilities':{'tools':{}},'serverInfo':{'name':'stateful','version':'1'}}
 elif method=='tools/list':result={'tools':[{'name':'increment','inputSchema':{'type':'object','properties':{'mode':{'type':'string'}}}}]}
 elif method=='tools/call':
  counter+=1;record={'pid':os.getpid(),'count':counter,'id':ident,'mode':r['params']['arguments']['mode']}
  with open(sys.argv[1],'a') as f:f.write(json.dumps(record)+'\\n')
  if record['mode']=='break':sys.exit(0)
  if record['mode']=='slow':time.sleep(1.5)
  result={'isError':record['mode']=='error','content':[{'type':'text','text':json.dumps(record)}]}
 else:result={}
 print(json.dumps({'jsonrpc':'2.0','id':ident,'result':result}),flush=True)
''')
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}';proc=None
 def call(name,args=None,expected=200):
  req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base})
  try:
   with urllib.request.urlopen(req,timeout=15) as response:status=response.status;data=json.load(response)
  except urllib.error.HTTPError as error:status=error.code;data=json.load(error)
  assert status==expected,(name,status,data);return data.get('result') if status==200 else data
 def wait(session,predicate):
  for _ in range(400):
   d=call('agent_session',{'sessionId':session})
   if predicate(d):return d
   time.sleep(.025)
  raise AssertionError('timeout')
 def send(s,prompt):
  call('agent_send_message',{'sessionId':s,'content':prompt});d=wait(s,lambda x:any(r['status']=='pending_approval' for r in x['runs']));run=next(r for r in reversed(d['runs']) if r['status']=='pending_approval');call('agent_review_tool',{'sessionId':s,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True});return d
 def turn(s,prompt):send(s,prompt);return wait(s,lambda x:not x['modelPending'])
 def record(d):return json.loads(json.loads(d['runs'][-1]['output'])['output'])
 def records():return [json.loads(x) for x in logfile.read_text().splitlines()] if logfile.exists() else []
 def alive(pid):
  try:os.kill(pid,0);return True
  except ProcessLookupError:return False
 def dead(pid):
  for _ in range(100):
   if not alive(pid):return
   time.sleep(.02)
  raise AssertionError('MCP process did not terminate')
 try:
  proc=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'HOME':folder,'TMPDIR':folder},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  for _ in range(100):
   try:call('agent_bootstrap');break
   except Exception:time.sleep(.05)
  config={'name':'Stateful','transport':'stdio','command':sys.executable,'args':[str(script),str(logfile)]};sid=call('agent_mcp_save',config)['id'];call('agent_mcp_refresh',{'id':sid})
  call('agent_model_save',{'enabled':True,'baseUrl':f'http://127.0.0.1:{model.server_port}','model':'fixture','apiKey':'fixture-only'})
  s=call('agent_new_session')['id'];a=record(turn(s,'first'));b=record(turn(s,'second'));assert a['pid']==b['pid'] and (a['count'],b['count'])==(1,2) and (a['id'],b['id'])==(2,3)
  failed=turn(s,'error');assert failed['runs'][-1]['status']=='failed' and call('agent_mcp_list')[0]['connectedSessions']==1
  after_error=record(turn(s,'after tool error'));assert after_error['pid']==a['pid'] and after_error['count']==4
  other=call('agent_new_session')['id'];c=record(turn(other,'other'));assert c['pid']!=a['pid'] and c['count']==1;assert call('agent_mcp_list')[0]['connectedSessions']==2
  call('agent_mcp_save',{**config,'id':sid,'enabled':True});dead(a['pid']);dead(c['pid']);assert call('agent_mcp_list')[0]['connectedSessions']==0
  fresh=record(turn(s,'after config'));assert fresh['count']==1 and fresh['pid']!=a['pid']
  call('agent_mcp_disconnect');dead(fresh['pid']);assert call('agent_mcp_list')[0]['connectedSessions']==0
  fresh=record(turn(s,'after disconnect'));assert fresh['count']==1
  before=len(records());d=turn(s,'break');assert len(records())==before+1 and d['runs'][-1]['status']=='failed';assert call('agent_mcp_list')[0]['connectedSessions']==0
  fresh=record(turn(s,'after failure'));assert fresh['count']==1
  pending=send(s,'slow')
  for _ in range(100):
   if records()[-1]['mode']=='slow':break
   time.sleep(.02)
  assert records()[-1]['mode']=='slow';call('agent_stop_generation',{'sessionId':s,'messageId':pending['streamingMessageId']});wait(s,lambda x:not x['modelPending']);dead(fresh['pid'])
  fresh=record(turn(s,'after stop'));assert fresh['count']==1
  pending=send(s,'slow')
  for _ in range(100):
   if records()[-1]['mode']=='slow':break
   time.sleep(.02)
  call('agent_mcp_disconnect');d=wait(s,lambda x:not x['modelPending']);assert d['runs'][-1]['status']=='failed';dead(fresh['pid'])
  fresh=record(turn(s,'after active disconnect'));assert fresh['count']==1
  call('logout');dead(fresh['pid']);assert call('agent_mcp_list')[0]['connectedSessions']==0
  fresh=record(turn(s,'before shutdown'));proc.terminate();proc.wait(timeout=10);dead(fresh['pid'])
  print('PASS: MCP state persists across turns; sessions isolated; request IDs advance; config/disconnect/logout/shutdown clean up; errors never replay; stopped transports discarded.')
 finally:
  if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=10)
  model.shutdown();model.server_close()
