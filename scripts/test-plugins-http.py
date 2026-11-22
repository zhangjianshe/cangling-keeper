#!/usr/bin/env python3
"""End-to-end MCP test using only temporary localhost fixtures."""
import sqlite3,http.server,json,os,socket,subprocess,sys,tempfile,threading,time,urllib.request,urllib.error
from pathlib import Path
requests=[]
class Model(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body)
  tools=[t['function']['name'] for t in body['tools'] if t['function']['name'].startswith('mcp_')]
  assert len(tools)==1
  result=[m for m in body['messages'] if m['role']=='tool']
  if not result: delta={'tool_calls':[{'index':0,'id':'mcp-call','type':'function','function':{'name':tools[0],'arguments':json.dumps({'value':'slow' if any(m.get('content')=='running MCP' for m in body['messages']) else 'hello'})}}]};reason='tool_calls'
  else: delta={'content':'MCP complete: '+result[-1]['content']};reason='stop'
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers();self.wfile.write(('data: '+json.dumps({'choices':[{'delta':delta,'finish_reason':reason}]})+'\n\ndata: [DONE]\n\n').encode())
api=http.server.ThreadingHTTPServer(('127.0.0.1',0),Model);threading.Thread(target=api.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-mcp-test-') as folder:
 root=Path(folder);marker=root/'calls';server=root/'mcp.py'
 server.write_text('''import sys,json,time\nmarker=sys.argv[1]\nfor line in sys.stdin:\n m=json.loads(line); method=m.get("method"); ident=m.get("id")\n if ident is None: continue\n if method=="initialize": result={"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}\n elif method=="tools/list": result={"tools":[{"name":"echo","description":"Echo fixture","inputSchema":{"type":"object","properties":{"value":{"type":"string"}},"required":["value"]}}]}\n elif method=="tools/call":\n  with open(marker,"a") as f:f.write("called\\n")\n  if m["params"]["arguments"]["value"]=="slow":\n   time.sleep(1.5)\n   with open(marker,"a") as f:f.write("late\\n")\n  result={"content":[{"type":"text","text":"fixture "+m["params"]["arguments"]["value"]}]}\n else: result={}\n print(json.dumps({"jsonrpc":"2.0","id":ident,"result":result}),flush=True)\n''')
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}';log=open(root/'log','wb');proc=None
 def call(name,args=None,expected=200):
  req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base})
  try:
   with urllib.request.urlopen(req,timeout=15) as r:status=r.status;data=json.load(r)
  except urllib.error.HTTPError as e:status=e.code;data=json.load(e)
  assert status==expected,(name,status,data);return data.get('result') if status==200 else data
 def wait(session,predicate):
  for _ in range(400):
   d=call('agent_session',{'sessionId':session})
   if predicate(d):return d
   time.sleep(.025)
  raise AssertionError('timed out')
 try:
  proc=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'TMPDIR':folder},stdout=log,stderr=log)
  for _ in range(100):
   try:call('agent_bootstrap');break
   except Exception:time.sleep(.05)
  manifest={'schemaVersion':1,'id':'fixture.plugin','name':'Fixture','version':'1.0.0','source':'https://example.invalid/plugin','permissions':['executeLocal'],'mcp':{'transport':'stdio','command':sys.executable,'args':[str(server),str(marker)]}}
  args={'manifest':manifest,'projectIds':['general']};preview=call('agent_plugin_preview',args);assert preview['sourceVerified'] is False and len(preview['sha256'])==64
  assert call('agent_mcp_list')==[] and not marker.exists()
  call('agent_plugin_install',{**args,'review':preview['review'],'confirmed':False},400)
  call('agent_plugin_install',{**args,'review':'changed','confirmed':True},400)
  call('agent_plugin_preview',{**args,'manifest':{**manifest,'permissions':['connectRemote']}},400)
  call('agent_plugin_preview',{**args,'manifest':{**manifest,'installScript':'touch /tmp/forbidden'}},400)
  call('agent_plugin_preview',{**args,'manifest':{**manifest,'source':'http://untrusted.invalid'}},400)
  call('agent_plugin_preview',{**args,'projectIds':['missing']},400)
  saved=call('agent_plugin_install',{**args,'review':preview['review'],'confirmed':True});sid=saved['id'];assert not saved['servers'][0]['enabled'] and not marker.exists()
  assert saved['servers'][0]['plugin']['manifest']==manifest
  call('agent_plugin_install',{**args,'review':preview['review'],'confirmed':True},400)
  with sqlite3.connect(root/'data.sql') as db:
   for key,value in {'server_url':'https://fixture.invalid/ib','login_token':'fixture','login_user_id':'other','login_username':'other'}.items():db.execute('INSERT OR REPLACE INTO settings(key,value) VALUES (?,?)',(key,value))
  assert call('agent_mcp_list')==[];call('agent_plugin_install',{**args,'review':preview['review'],'confirmed':True},400);call('agent_mcp_delete',{'id':sid},400)
  with sqlite3.connect(root/'data.sql') as db:db.execute("UPDATE settings SET value='' WHERE key='login_token'")
  servers=call('agent_mcp_refresh',{'id':sid});assert servers[0]['enabled'] and servers[0]['tools'][0]['name']=='echo'
  disabled=call('agent_mcp_save',{**servers[0],'enabled':False});assert not disabled['servers'][0]['enabled'];assert disabled['servers'][0]['plugin']['manifest']==manifest
  servers=call('agent_mcp_refresh',{'id':sid});assert servers[0]['enabled'] and not marker.exists()
  call('agent_model_save',{'enabled':True,'baseUrl':f'http://127.0.0.1:{api.server_port}','model':'fixture','apiKey':'fixture-only-key'})
  session=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':session,'content':'run MCP'})
  pending=wait(session,lambda d:any(r['status']=='pending_approval' for r in d['runs']));run=pending['runs'][-1];assert run['review']['kind']=='mcp' and run['review']['tool']=='echo' and not marker.exists()
  assert call('agent_session',{'sessionId':session})['runs'][-1]['review']==run['review']
  call('agent_review_tool',{'sessionId':session,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True})
  done=wait(session,lambda d:not d['modelPending']);assert marker.read_text()=='called\n';assert 'fixture hello' in done['messages'][-1]['content'];assert done['runs'][-1]['status']=='succeeded'
  call('agent_review_tool',{'sessionId':session,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True},400)
  session2=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':session2,'content':'deny MCP'})
  pending=wait(session2,lambda d:any(r['status']=='pending_approval' for r in d['runs']));run=pending['runs'][-1]
  call('agent_review_tool',{'sessionId':session2,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':False});wait(session2,lambda d:not d['modelPending']);assert marker.read_text()=='called\n'
  session3=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':session3,'content':'stop MCP'})
  pending=wait(session3,lambda d:any(r['status']=='pending_approval' for r in d['runs']));run=pending['runs'][-1]
  call('agent_stop_generation',{'sessionId':session3,'messageId':pending['streamingMessageId']});wait(session3,lambda d:not d['modelPending']);assert marker.read_text()=='called\n'
  call('agent_review_tool',{'sessionId':session3,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True},400)
  running=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':running,'content':'running MCP'})
  pending=wait(running,lambda d:any(r['status']=='pending_approval' for r in d['runs']));run=pending['runs'][-1]
  call('agent_review_tool',{'sessionId':running,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True});wait(running,lambda d:marker.read_text().count('called')==2)
  call('agent_stop_generation',{'sessionId':running,'messageId':pending['streamingMessageId']});stopped=wait(running,lambda d:not d['modelPending']);assert stopped['runs'][-1]['status']=='stopped'
  time.sleep(2);assert 'late' not in marker.read_text(), 'cancelled MCP process must not continue'
  marker.write_text('called\n')
  session4=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':session4,'content':'changed config MCP'})
  pending=wait(session4,lambda d:any(r['status']=='pending_approval' for r in d['runs']));run=pending['runs'][-1]
  call('agent_mcp_save',{'id':sid,'name':'Fixture','command':sys.executable,'args':[str(server),'changed'],'enabled':True});assert not call('agent_mcp_list')[0]['enabled']
  call('agent_review_tool',{'sessionId':session4,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True});done=wait(session4,lambda d:not d['modelPending']);assert done['runs'][-1]['status']=='failed' and marker.read_text()=='called\n'
  before=marker.read_text();call('agent_mcp_delete',{'id':sid});assert call('agent_mcp_list')==[];assert marker.read_text()==before;assert call('agent_session',{'sessionId':session})['runs']
  print('Plugin install checks passed; MCP checks passed: discovery, account config, model tools, approval, refresh, rejection, no replay and changed-command disabling.')
 finally:
  if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=10)
  log.close();api.shutdown();api.server_close()
