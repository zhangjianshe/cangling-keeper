#!/usr/bin/env python3
"""Isolated SSE and Streamable HTTP MCP fixtures; no external services."""
import http.server,json,os,queue,socket,subprocess,tempfile,threading,time,urllib.request,urllib.error,uuid
from pathlib import Path
from fixture_scope import respond_scope
sessions={};http_sessions=set();connections=[];calls=[];cancelled=[];fixture_header='fixture-secret-header';mcp_schema={'name':'echo','description':'fixture echo','inputSchema':{'type':'object','properties':{'value':{'type':'string'}}}}
class Handler(http.server.BaseHTTPRequestHandler):
 protocol_version='HTTP/1.1'
 def log_message(self,*args):pass
 def body(self,obj,status=200,headers=None):
  raw=json.dumps(obj).encode();self.send_response(status);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(raw)))
  for k,v in (headers or {}).items():self.send_header(k,v)
  self.end_headers();self.wfile.write(raw);self.wfile.flush()
 def empty(self):self.send_response(202);self.send_header('Content-Length','0');self.end_headers()
 def do_GET(self):
  assert self.headers.get('X-Test-Key')==fixture_header
  if self.path=='/cross':
   self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers();self.wfile.write(b'event: endpoint\ndata: http://127.0.0.1:1/evil\n\n');self.wfile.flush();return
  if self.path!='/sse':self.send_error(404);return
  sid=uuid.uuid4().hex;q=queue.Queue();sessions[sid]=q
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers();self.wfile.write(('event: endpoint\r\ndata: /messages?session='+sid+'\r\n\r\n').encode());self.wfile.flush()
  try:
   while True:
    event=q.get(timeout=30);self.wfile.write(('event: message\ndata: '+json.dumps(event)+'\n\n').encode());self.wfile.flush()
  except (BrokenPipeError,ConnectionResetError,queue.Empty):pass
 def do_DELETE(self):
  http_sessions.discard(self.headers.get('Mcp-Session-Id'));self.empty()
 def do_POST(self):
  assert self.headers.get('X-Test-Key')==fixture_header
  request=json.loads(self.rfile.read(int(self.headers.get('Content-Length','0'))) or b'{}');method=request.get('method');ident=request.get('id')
  if method.startswith('notifications/'):
   if method=='notifications/cancelled':cancelled.append(request['params']['requestId'])
   self.empty();return
  if method=='initialize':result={'protocolVersion':'2025-06-18','capabilities':{'tools':{}},'serverInfo':{'name':'fixture','version':'1'}}
  elif method=='tools/list':result={'tools':[mcp_schema]}
  elif method=='tools/call':
   calls.append(request['params']);connections.append((self.path.split('=',1)[1] if self.path.startswith('/messages?') else self.headers.get('Mcp-Session-Id'),ident))
   if request['params']['arguments']['value']=='slow':time.sleep(3)
   result={'content':[{'type':'text','text':'remote '+request['params']['arguments']['value']}]}
  else:self.send_error(400);return
  msg={'jsonrpc':'2.0','id':ident,'result':result}
  if self.path.startswith('/messages?session='):
   sessions[self.path.split('=',1)[1]].put(msg);self.empty();return
  assert self.path=='/mcp'
  if method!='initialize':assert self.headers.get('Mcp-Session-Id') in http_sessions and self.headers.get('MCP-Protocol-Version')=='2025-06-18'
  session_id=uuid.uuid4().hex if method=='initialize' else None
  if session_id:http_sessions.add(session_id)
  if method=='tools/list':
   raw=('event: message\ndata: '+json.dumps(msg)+'\n\n').encode();self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(raw)));self.end_headers();self.wfile.write(raw);return
  self.body(msg,headers={'Mcp-Session-Id':session_id} if method=='initialize' else None)
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);server.daemon_threads=True;threading.Thread(target=server.serve_forever,daemon=True).start()
class Model(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  data=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
  if respond_scope(self,data):return
  assert fixture_header not in json.dumps(data)
  tools=[x['function']['name'] for x in data['tools'] if x['function']['name'].startswith('mcp_')];assert len(tools)==1
  last_user=max(i for i,x in enumerate(data['messages']) if x['role']=='user');results=[x for x in data['messages'][last_user+1:] if x['role']=='tool'];delta={'content':'finished '+results[-1]['content']} if results else {'tool_calls':[{'index':0,'id':'remote-call','type':'function','function':{'name':tools[0],'arguments':json.dumps({'value':'slow' if any(x.get('content')=='slow test' for x in data['messages']) else 'hello'})}}]};reason='stop' if results else 'tool_calls'
  raw=('data: '+json.dumps({'choices':[{'delta':delta,'finish_reason':reason}]})+'\n\ndata: [DONE]\n\n').encode();self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(raw)));self.end_headers();self.wfile.write(raw)
model=http.server.ThreadingHTTPServer(('127.0.0.1',0),Model);model.daemon_threads=True;threading.Thread(target=model.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-mcp-remote-') as folder:
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}';log=open(Path(folder)/'log','wb');proc=None
 def call(name,args=None,expected=200):
  request=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base})
  try:
   with urllib.request.urlopen(request,timeout=15) as response: status=response.status;data=json.load(response)
  except urllib.error.HTTPError as error:status=error.code;data=json.load(error)
  assert status==expected,(name,status,data);return data.get('result') if status==200 else data
 def wait(s,pred):
  for _ in range(400):
   data=call('agent_session',{'sessionId':s})
   if pred(data):return data
   time.sleep(.025)
  raise AssertionError('timeout')
 try:
  proc=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'KEEPER_MAP_MCP_URL':'','HOME':folder,'TMPDIR':folder},stdout=log,stderr=log)
  for _ in range(100):
   try:call('agent_bootstrap');break
   except Exception:time.sleep(.05)
  call('agent_model_save',{'enabled':True,'baseUrl':f'http://127.0.0.1:{model.server_port}','model':'fixture','apiKey':'fixture-only-key'})
  for kind,path in [('sse','/sse'),('http','/mcp')]:
   value=call('agent_mcp_save',{'name':kind,'transport':kind,'command':f'http://127.0.0.1:{server.server_port}{path}','args':['X-Test-Key: '+fixture_header]});sid=value['id']
   listing=call('agent_mcp_refresh',{'id':sid});assert listing[0]['enabled'] and listing[0]['tools'][0]['name']=='echo' and listing[0]['tools'][0]['enabled']
   listing=call('agent_mcp_tools',{'id':sid,'tools':[]});assert listing[0]['tools'][0]['enabled'] is False
   listing=call('agent_mcp_refresh',{'id':sid});assert listing[0]['tools'][0]['enabled'] is False,'refresh must preserve selection'
   listing=call('agent_mcp_tools',{'id':sid,'tools':['echo']});assert listing[0]['tools'][0]['enabled'] is True
   call('agent_mcp_tools',{'id':sid,'tools':['missing']},400)
   session=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':session,'content':'remote test'})
   pending=wait(session,lambda x:any(r['status']=='pending_approval' for r in x['runs']));run=pending['runs'][-1];assert fixture_header not in json.dumps(run)
   call('agent_review_tool',{'sessionId':session,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True});done=wait(session,lambda x:not x['modelPending']);assert 'remote hello' in done['messages'][-1]['content']
   first_connection=connections[-1];call('agent_send_message',{'sessionId':session,'content':'second remote turn'})
   pending=wait(session,lambda x:any(r['status']=='pending_approval' for r in x['runs']));run=next(r for r in reversed(pending['runs']) if r['status']=='pending_approval');call('agent_review_tool',{'sessionId':session,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True});wait(session,lambda x:not x['modelPending'])
   assert connections[-1]==(first_connection[0],first_connection[1]+1),'MCP connection and request sequence must persist across turns'
   assert call('agent_mcp_list')[0]['connectedSessions']==1
   other=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':other,'content':'isolated remote turn'})
   pending=wait(other,lambda x:any(r['status']=='pending_approval' for r in x['runs']));run=pending['runs'][-1];call('agent_review_tool',{'sessionId':other,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True});wait(other,lambda x:not x['modelPending']);assert connections[-1][0]!=first_connection[0] and connections[-1][1]==2
   assert call('agent_mcp_list')[0]['connectedSessions']==2
   call('agent_mcp_delete',{'id':sid})
  value=call('agent_mcp_save',{'name':'slow','transport':'sse','command':f'http://127.0.0.1:{server.server_port}/sse','args':['X-Test-Key: '+fixture_header]});sid=value['id'];call('agent_mcp_refresh',{'id':sid})
  session=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':session,'content':'slow test'});pending=wait(session,lambda x:any(r['status']=='pending_approval' for r in x['runs']));run=pending['runs'][-1]
  call('agent_review_tool',{'sessionId':session,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True})
  for _ in range(100):
   if any(c['arguments']['value']=='slow' for c in calls):break
   time.sleep(.02)
  assert any(c['arguments']['value']=='slow' for c in calls);call('agent_stop_generation',{'sessionId':session,'messageId':pending['streamingMessageId']});wait(session,lambda x:not x['modelPending'])
  for _ in range(100):
   if cancelled:break
   time.sleep(.02)
  assert 2 in cancelled,'cancellation notification was not sent';call('agent_mcp_delete',{'id':sid})
  bad=call('agent_mcp_save',{'name':'bad','transport':'sse','command':f'http://127.0.0.1:{server.server_port}/cross','args':['X-Test-Key: '+fixture_header]});call('agent_mcp_refresh',{'id':bad['id']},400)
  call('agent_mcp_save',{'name':'bad','transport':'sse','command':'http://example.com/sse','args':[]},400)
  call('agent_mcp_save',{'name':'bad','transport':'sse','command':f'http://127.0.0.1:{server.server_port}/sse','args':['Host: evil']},400)
  print('Remote MCP checks passed: legacy SSE, Streamable HTTP JSON/SSE, session/version headers, custom headers, approval, secret isolation, same-origin endpoint and invalid configuration.')
 finally:
  if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=10)
  log.close();server.shutdown();model.shutdown();server.server_close();model.server_close()
