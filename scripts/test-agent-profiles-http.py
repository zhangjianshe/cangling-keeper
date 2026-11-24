#!/usr/bin/env python3
"""Agent presets: real HTTP/SSE loop with local fake model; never calls paid providers."""
import http.server,json,os,socket,sqlite3,subprocess,tempfile,threading,time,urllib.request,urllib.error
from pathlib import Path
requests=[];hold=threading.Event();started=threading.Event();allowed_mcp=''
class Model(http.server.BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body)
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
  def emit(delta,reason=None):self.wfile.write(('data: '+json.dumps({'choices':[{'delta':delta,'finish_reason':reason}]})+'\n\n').encode());self.wfile.flush()
  try:
   if not any(m['role']=='tool' for m in body['messages']):
    started.set();hold.wait(10)
    calls=[('load_skill',{'name':'chosen'}),('load_skill',{'name':'other'}),('mcp_not_associated',{})]
    emit({'tool_calls':[{'index':i,'id':'t'+str(i),'type':'function','function':{'name':n,'arguments':json.dumps(a)}} for i,(n,a) in enumerate(calls)]},'tool_calls')
   else:emit({'content':'Preset complete'},'stop')
   self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush()
  except (BrokenPipeError,ConnectionResetError):pass
api=http.server.ThreadingHTTPServer(('127.0.0.1',0),Model);threading.Thread(target=api.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-profiles-') as tmp:
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}';proc=None
 def call(n,a={},status=200):
  req=urllib.request.Request(base+'/api/invoke/'+n,data=json.dumps(a).encode(),headers={'Content-Type':'application/json','Origin':base})
  try:
   with urllib.request.urlopen(req,timeout=10) as r:code=r.status;v=json.load(r)
  except urllib.error.HTTPError as e:code=e.code;v=json.load(e)
  assert code==status,(n,code,v)
  return v.get('result',v)
 def start():
  global proc
  proc=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',tmp,'--no-open'],env={**os.environ,'TMPDIR':tmp},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  for _ in range(100):
   try:call('agent_bootstrap');return
   except Exception:time.sleep(.05)
  raise AssertionError('startup')
 def wait(s):
  for _ in range(200):
   d=call('agent_session',{'sessionId':s})
   if not d['modelPending']:return d
   time.sleep(.025)
  raise AssertionError('completion')
 try:
  start();proc.terminate();proc.wait(10);root=Path(tmp)/'project';root.mkdir()
  for name in ('chosen','other'):
   folder=root/'.agents/skills'/name;folder.mkdir(parents=True);(folder/'SKILL.md').write_text(f'---\nname: {name}\ndescription: {name}-description\n---\n{name}-skill-body')
  with sqlite3.connect(Path(tmp)/'data.sql') as db:db.execute("UPDATE agent_projects SET directory=? WHERE id='general'",(str(root),))
  start();s=call('agent_new_session')['id'];other=call('agent_new_session')['id']
  model=call('agent_model_save',{'name':'Profile Model','enabled':True,'baseUrl':f'http://127.0.0.1:{api.server_port}','model':'profile-fixture','apiKey':'fixture'})['savedServerId']
  # Seed discovered tool metadata; execution of the unassociated MCP name must be rejected before launch.
  with sqlite3.connect(Path(tmp)/'data.sql') as db:
   db.execute("INSERT INTO agent_mcp_servers(id,owner_key,name,command,args,enabled,tools) VALUES ('preset-fixture','guest','Fixture','never-launch','[]',1,?)",(json.dumps([{'name':'allowed','description':'allowed MCP fixture','inputSchema':{'type':'object'},'enabled':True},{'name':'excluded','description':'excluded MCP fixture','inputSchema':{'type':'object'},'enabled':True}]),))
  opts=call('agent_profile_options',{'sessionId':s});assert len(opts['skills'])==3 and len(opts['mcpTools'])==2
  allowed=opts['mcpTools'][0]['function']['name'];excluded=opts['mcpTools'][1]['function']['name']
  icon='data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAADCAIAAAA2iEnWAAAAFUlEQVR4nGP8//8/AwMDEwMDA4ICADkbAwP+wj6MAAAAAElFTkSuQmCC'
  payload={'icon':icon,'sessionId':s,'name':'编码助手','systemPrompt':'PRESET-ORIGINAL','modelId':model,'skills':['chosen'],'mcpTools':[allowed]}
  preset=call('agent_profile_save',payload);assert preset['icon']==icon;pid=preset['id'];payload['id']=pid
  for invalid in ['https://example.com/icon.png','data:image/svg+xml;base64,PHN2Zz4=','data:image/png;base64,AAAA','data:image/png;base64,'+'A'*90000,123]:
   call('agent_profile_save',{**payload,'icon':invalid},400)
  legacy={k:v for k,v in payload.items() if k!='icon'};assert call('agent_profile_save',legacy)['icon']==icon
  call('agent_profile_save',{**payload,'skills':['missing']},400)
  call('agent_profile_save',{**payload,'mcpTools':['missing']},400)
  d=call('agent_profile_select',{'sessionId':s,'id':pid});assert d['model']['activeServerId']==model
  assert call('agent_session',{'sessionId':other})['agentProfile'] is None
  call('agent_session_model',{'sessionId':s,'id':model},400)
  call('agent_profile_delete',{'sessionId':s,'id':pid},400)
  call('agent_send_message',{'sessionId':s,'content':'profile test'});assert started.wait(5)
  call('agent_profile_select',{'sessionId':s,'id':''},400)
  call('agent_profile_save',{**payload,'systemPrompt':'PRESET-EDITED','icon':''})
  hold.set();d=wait(s);assert d['tasks'][-1]['status']=='complete'
  answer=next(m for m in d['messages'] if m['role']=='assistant');assert any(e['ref']=='system:earth-workbench' for e in answer['modelMetadata']['skillBindings']);assert answer['modelMetadata']['agentProfile']['systemPrompt']=='PRESET-ORIGINAL';assert answer['modelMetadata']['agentProfile']['icon']==icon;assert d['agentProfile']['icon']==''
  assert [r['status'] for r in d['runs']]==['succeeded','failed','failed']
  for req in requests:
   assert icon not in json.dumps(req)
   assert req['model']=='profile-fixture'
   text=req['messages'][0]['content'];assert 'PRESET-ORIGINAL' in text and 'PRESET-EDITED' not in text and 'chosen-description' in text and 'other-description' not in text
   names=[t['function']['name'] for t in req['tools']];assert allowed in names and excluded not in names
  with sqlite3.connect(Path(tmp)/'data.sql') as db:
   meta=json.loads(db.execute('SELECT model_metadata FROM agent_messages WHERE id=?',(answer['id'],)).fetchone()[0]);meta['skillBindings']=[e for e in meta['skillBindings'] if e['scope']!='system'];db.execute('UPDATE agent_messages SET model_metadata=? WHERE id=?',(json.dumps(meta),answer['id']))
  requests.clear();call('agent_regenerate',{'sessionId':s,'messageId':answer['id']});d=wait(s)
  assert any('system:earth-workbench' in r['messages'][0]['content'] for r in requests)
  assert all(m['modelMetadata']['agentProfile']['icon']==icon for m in d['messages'] if m['role']=='assistant')
  assert requests and all('PRESET-ORIGINAL' in r['messages'][0]['content'] for r in requests)
  hold.clear();started.clear();stopping=call('agent_new_session')['id'];call('agent_profile_select',{'sessionId':stopping,'id':pid})
  call('agent_send_message',{'sessionId':stopping,'content':'stop profile'});assert started.wait(5)
  call('agent_stop_generation',{'sessionId':stopping,'messageId':call('agent_session',{'sessionId':stopping})['streamingMessageId']});stopped=wait(stopping);assert stopped['tasks'][-1]['status']=='stopped';hold.set()
  call('agent_profile_select',{'sessionId':stopping,'id':''})
  with sqlite3.connect(Path(tmp)/'data.sql') as db:db.execute("UPDATE agent_profiles SET owner='foreign' WHERE id=?",(pid,))
  assert not call('agent_profile_options',{'sessionId':other})['profiles']
  call('agent_profile_save',{**payload,'sessionId':other},400)
  call('agent_profile_select',{'sessionId':other,'id':pid},400)
  with sqlite3.connect(Path(tmp)/'data.sql') as db:db.execute("UPDATE agent_profiles SET owner='guest' WHERE id=?",(pid,))
  proc.terminate();proc.wait(10);start();assert call('agent_session',{'sessionId':s})['agentProfile']['id']==pid
  assert call('agent_session',{'sessionId':s})['agentProfile']['icon']==''
  # Different local account cannot list or mutate the original owner's presets by session or ID.
  with sqlite3.connect(Path(tmp)/'data.sql') as db:db.execute("UPDATE agent_sessions SET owner_key='foreign' WHERE id=?",(s,))
  call('agent_profile_options',{'sessionId':s},400)
  call('agent_profile_save',{**payload,'sessionId':s},400)
  with sqlite3.connect(Path(tmp)/'data.sql') as db:db.execute("UPDATE agent_sessions SET owner_key='guest' WHERE id=?",(s,))
  call('agent_profile_select',{'sessionId':s,'id':''});call('agent_profile_delete',{'sessionId':s,'id':pid});assert not call('agent_profile_options',{'sessionId':s})['profiles']
  print('PASS profiles: CRUD, model/prompt injection, skill/MCP allowlist, frozen snapshot/regeneration, persistence and owner guards')
 finally:
  hold.set()
  if proc and proc.poll() is None:proc.terminate();proc.wait(10)
  api.shutdown()
