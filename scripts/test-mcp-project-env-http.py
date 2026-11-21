#!/usr/bin/env python3
"""Private stdio environment + project-scoped MCP through disposable local HTTP.
Uses a mock model, temporary SQLite/projects and TMPDIR; never reads real keys.
"""
import http.server,json,os,socket,sqlite3,subprocess,sys,tempfile,threading,time,urllib.request,urllib.error,uuid
from pathlib import Path

SECRET='PRIVATE_MCP_ENV_FIXTURE_NOT_FOR_MODEL'
SECRET2='PRIVATE_MCP_ENV_ROTATED_NOT_FOR_MODEL'
HOST_SECRET='HOST_ENV_FIXTURE_MUST_NOT_BE_INHERITED'
requests=[];model_errors=[];remote_calls=[]
class Model(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  try:
   body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body)
   for secret in (SECRET,SECRET2,HOST_SECRET):assert secret not in json.dumps(body),'private environment leaked to model'
   last=max(i for i,m in enumerate(body['messages']) if m['role']=='user');command=json.loads(body['messages'][last]['content']);results=[m for m in body['messages'][last+1:] if m['role']=='tool']
   names=[t['function']['name'] for t in body.get('tools',[]) if t['function']['name'].startswith('mcp_')]
   if command.get('action')=='inspect' or results:delta={'content':'Fixture completed; available MCP tools: '+json.dumps(names)};reason='stop'
   else:
    name=command.get('forcedName') or next(n for n in names if n.startswith('mcp_'+command['serverId']+'_'))
    delta={'tool_calls':[{'index':0,'id':'scope-'+uuid.uuid4().hex[:12],'type':'function','function':{'name':name,'arguments':json.dumps({'mode':command.get('mode','probe')})}}]};reason='tool_calls'
   raw=('data: '+json.dumps({'choices':[{'delta':delta,'finish_reason':reason}]})+'\n\ndata: [DONE]\n\n').encode();self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(raw)));self.end_headers();self.wfile.write(raw)
  except Exception as error:model_errors.append(repr(error));self.send_response(500);self.end_headers()
class Remote(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  message=json.loads(self.rfile.read(int(self.headers['Content-Length'])));method=message.get('method');remote_calls.append(message)
  if 'id' not in message:self.send_response(202);self.send_header('Content-Length','0');self.end_headers();return
  result={'protocolVersion':'2025-06-18','capabilities':{'tools':{}},'serverInfo':{'name':'Remote fixture','version':'1'}} if method=='initialize' else {'tools':[{'name':'probe','inputSchema':{'type':'object'}}]}
  raw=json.dumps({'jsonrpc':'2.0','id':message['id'],'result':result}).encode();self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(raw)));self.end_headers();self.wfile.write(raw)
model=http.server.ThreadingHTTPServer(('127.0.0.1',0),Model);remote=http.server.ThreadingHTTPServer(('127.0.0.1',0),Remote)
for server in (model,remote):server.daemon_threads=True;threading.Thread(target=server.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-mcp-project-env-') as folder:
 root=Path(folder);project_a=root/'project-a';project_b=root/'project-b';project_a.mkdir();project_b.mkdir();script=root/'mcp.py';logfile=root/'calls.jsonl';gate=root/'delay-list';waiting=root/'list-waiting'
 script.write_text('''import json,os,sys,time\nfrom pathlib import Path\nlog=Path(sys.argv[1]);gate=Path(sys.argv[2]);waiting=Path(sys.argv[3]);counter=0\ndef record(value):\n value.update(pid=os.getpid(),time=time.monotonic_ns())\n with log.open('a') as f:f.write(json.dumps(value)+'\\n')\nrecord({'event':'launch','environment':{key:os.environ.get(key) for key in ['KEEPER_TEST_TOKEN','EMPTY','DROP_ME','KEEPER_HOST_ONLY_SECRET','TMPDIR']}})\nfor line in sys.stdin:\n r=json.loads(line);method=r.get('method');ident=r.get('id')\n if ident is None:continue\n record({'event':'rpc','method':method,'id':ident})\n if method=='initialize':result={'protocolVersion':'2025-06-18','capabilities':{'tools':{}},'serverInfo':{'name':'Scope environment fixture','version':'1'}}\n elif method=='tools/list':\n  if gate.exists():\n   waiting.write_text(str(os.getpid()))\n   while gate.exists():time.sleep(.02)\n  result={'tools':[{'name':'probe','description':'Return a harmless counter, never environment values','inputSchema':{'type':'object','properties':{'mode':{'type':'string'}}}}]}\n elif method=='tools/call':\n  counter+=1;value={'pid':os.getpid(),'count':counter,'mode':r['params']['arguments']['mode']};record({'event':'call',**value});result={'content':[{'type':'text','text':json.dumps(value)}]}\n else:result={}\n print(json.dumps({'jsonrpc':'2.0','id':ident,'result':result}),flush=True)\n''')
 # Exercise real migration before Keeper opens the old MCP table.
 with sqlite3.connect(root/'data.sql') as conn:
  conn.execute("CREATE TABLE agent_mcp_servers(id TEXT PRIMARY KEY,owner_key TEXT NOT NULL,name TEXT NOT NULL,command TEXT NOT NULL,args TEXT NOT NULL,enabled INTEGER NOT NULL DEFAULT 0,tools TEXT NOT NULL DEFAULT '[]')")
  conn.execute('INSERT INTO agent_mcp_servers VALUES (?,?,?,?,?,?,?)',('legacy','guest','Legacy fixture',sys.executable,'[]',0,'[]'))
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}';proc=None;log=open(root/'keeper.log','wb')
 def db():return sqlite3.connect(root/'data.sql')
 def call(name,args=None,expected=200):
  req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base})
  try:
   with urllib.request.urlopen(req,timeout=20) as response:status=response.status;value=json.load(response)
  except urllib.error.HTTPError as error:status=error.code;value=json.load(error)
  assert status==expected,(name,status,value);result=value.get('result') if status==200 else value
  if name.startswith('agent_mcp'):
   for secret in (SECRET,SECRET2,HOST_SECRET):assert secret not in json.dumps(result),'private environment leaked to API'
  return result
 def start():
  global proc
  proc=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'TMPDIR':folder,'KEEPER_HOST_ONLY_SECRET':HOST_SECRET},stdout=log,stderr=log)
  for _ in range(150):
   if proc.poll() is not None:raise AssertionError(('Keeper exited',proc.returncode,(root/'keeper.log').read_text(errors='replace')[-4000:]))
   try:call('agent_bootstrap');return
   except Exception:time.sleep(.05)
  raise AssertionError('Keeper failed to start')
 def stop():
  if proc and proc.poll() is None:proc.terminate();proc.wait(10)
 def identity(user):
  with db() as conn:
   for key,value in {'server_url':f'http://127.0.0.1:{model.server_port}/ib','login_token':'dummy-local-login','login_user_id':str(user),'login_username':'user-'+str(user)}.items():conn.execute('INSERT OR REPLACE INTO settings(key,value) VALUES (?,?)',(key,value))
 def records(event=None):
  values=[json.loads(line) for line in logfile.read_text().splitlines()] if logfile.exists() else []
  return [value for value in values if event is None or value['event']==event]
 def listed(sid):return next(s for s in call('agent_mcp_list') if s['id']==sid)
 def update(server,**changes):
  config={key:server[key] for key in ('id','name','transport','command','args')};config.update(changes);result=call('agent_mcp_save',config);return next(s for s in result['servers'] if s['id']==server['id'])
 def refresh(sid):return next(s for s in call('agent_mcp_refresh',{'id':sid}) if s['id']==sid)
 def wait(session,predicate):
  for _ in range(500):
   detail=call('agent_session',{'sessionId':session})
   if predicate(detail):return detail
   time.sleep(.025)
  raise AssertionError(('timeout',detail))
 def send(session,sid=None,mode='probe',**extra):call('agent_send_message',{'sessionId':session,'content':json.dumps({'serverId':sid,'mode':mode,**extra})})
 def pending(session):return wait(session,lambda d:any(r['status']=='pending_approval' for r in d['runs']))
 def approve(session,detail):
  run=next(r for r in reversed(detail['runs']) if r['status']=='pending_approval');assert SECRET not in json.dumps(run) and SECRET2 not in json.dumps(run);call('agent_review_tool',{'sessionId':session,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True})
 def turn(session,sid,mode='probe'):
  send(session,sid,mode);detail=pending(session);approve(session,detail);return wait(session,lambda d:not d['modelPending'])
 def counter(detail):return json.loads(json.loads(detail['runs'][-1]['output'])['output'])
 def inspect(session,sid):
  start_index=len(requests);send(session,sid,action='inspect');wait(session,lambda d:not d['modelPending']);request=next(r for r in requests[start_index:] if json.loads(next(m['content'] for m in reversed(r['messages']) if m['role']=='user')).get('action')=='inspect');return [t['function']['name'] for t in request.get('tools',[]) if t['function']['name'].startswith('mcp_')]
 def dead(pid):
  for _ in range(100):
   try:os.kill(pid,0)
   except ProcessLookupError:return
   time.sleep(.02)
  raise AssertionError(('MCP child not terminated',pid))
 def race_refresh(server,**changes):
  gate.write_text('wait');waiting.unlink(missing_ok=True);outcome=[]
  thread=threading.Thread(target=lambda:outcome.append(call('agent_mcp_refresh',{'id':server['id']},400)));thread.start()
  for _ in range(150):
   if waiting.exists():break
   time.sleep(.02)
  assert waiting.exists(),'slow discovery never reached tools/list'
  changed=update(server,**changes);gate.unlink();thread.join(10);assert not thread.is_alive() and len(outcome)==1;return changed
 try:
  start();legacy=listed('legacy');assert legacy['transport']=='stdio' and legacy['environmentKeys']==[] and legacy['environmentRevision']=='' and legacy['projectIds'] is None;call('agent_mcp_delete',{'id':'legacy'})
  with db() as conn:
   columns={row[1] for row in conn.execute('PRAGMA table_info(agent_mcp_servers)')};assert {'environment','environment_revision','project_ids'}<=columns
   conn.execute("UPDATE agent_projects SET directory=? WHERE id='general'",(str(project_a),));conn.execute('INSERT INTO agent_projects VALUES (?,?,?,?)',('project-b','Project B',str(project_b),1))
  identity(1);call('agent_model_save',{'name':'Project environment fixture','enabled':True,'baseUrl':f'http://127.0.0.1:{model.server_port}','model':'fixture','apiKey':'dummy-local-key'})
  config={'name':'Scoped stdio fixture','transport':'stdio','command':sys.executable,'args':[str(script),str(logfile),str(gate),str(waiting)],'environmentChanges':{'KEEPER_TEST_TOKEN':SECRET,'EMPTY':'','DROP_ME':'remove later'},'projectIds':['general']}
  saved=call('agent_mcp_save',config);sid=saved['id'];server=listed(sid);revision=server['environmentRevision'];uuid.UUID(revision);assert server['environmentKeys']==['DROP_ME','EMPTY','KEEPER_TEST_TOKEN'] and server['projectIds']==['general'] and not server['enabled']
  server=refresh(sid);assert server['enabled'] and server['tools'];launch=records('launch')[-1];assert launch['environment']=={'KEEPER_TEST_TOKEN':SECRET,'EMPTY':'','DROP_ME':'remove later','KEEPER_HOST_ONLY_SECRET':None,'TMPDIR':folder}
  # Omitted patches, empty patches and identical values preserve private data/revision/discovery.
  original_tools=server['tools'];server=update(server,name='Renamed fixture');assert server['enabled'] and server['tools']==original_tools and server['environmentRevision']==revision and server['projectIds']==['general']
  for patch in ({},{'KEEPER_TEST_TOKEN':SECRET}):
   server=update(server,environmentChanges=patch);assert server['environmentRevision']==revision and server['enabled'] and server['tools']==original_tools
  a=call('agent_new_session',{'projectId':'general'})['id'];b=call('agent_new_session',{'projectId':'project-b'})['id'];names=inspect(a,sid);assert len(names)==1 and inspect(b,sid)==[];valid_name=names[0]
  before=len(records('call'));send(b,sid,forcedName=valid_name);denied=wait(b,lambda d:not d['modelPending']);assert denied['runs'][-1]['status']=='failed' and not denied['runs'][-1].get('review') and len(records('call'))==before,'valid tool hash outside project must fail before approval/RPC'
  first=counter(turn(a,sid));second=counter(turn(a,sid));assert first['pid']==second['pid'] and second['count']==first['count']+1
  # Account ownership applies equally to metadata, secret patches and session access.
  identity(2);assert call('agent_mcp_list')==[];call('agent_mcp_save',{**config,'id':sid},400);call('agent_mcp_refresh',{'id':sid},400);call('agent_session',{'sessionId':a},400);identity(1)
  # Env mutations invalidate waiting approvals and live pooled children; rediscovery is required.
  send(a,sid);approval=pending(a);before=len(records('call'));server=update(server,environmentChanges={'KEEPER_TEST_TOKEN':SECRET2,'DROP_ME':None});assert not server['enabled'] and server['tools']==[] and server['environmentRevision']!=revision and server['environmentKeys']==['EMPTY','KEEPER_TEST_TOKEN'];dead(first['pid']);approve(a,approval);invalid=wait(a,lambda d:not d['modelPending']);assert invalid['runs'][-1]['status']=='failed' and len(records('call'))==before
  server=refresh(sid);launch=records('launch')[-1]['environment'];assert launch['KEEPER_TEST_TOKEN']==SECRET2 and launch['EMPTY']=='' and launch['DROP_ME'] is None and launch['KEEPER_HOST_ONLY_SECRET'] is None
  fresh=counter(turn(a,sid));assert fresh['pid']!=first['pid'] and fresh['count']==1
  # Scope changes preserve selection/enabled while revoking old schema and approvals.
  send(a,sid);approval=pending(a);before=len(records('call'));tools=server['tools'];revision=server['environmentRevision'];server=update(server,projectIds=['project-b']);assert server['enabled'] and server['tools']==tools and server['environmentRevision']==revision;dead(fresh['pid']);approve(a,approval);invalid=wait(a,lambda d:not d['modelPending']);assert invalid['runs'][-1]['status']=='failed' and len(records('call'))==before;assert inspect(a,sid)==[] and len(inspect(b,sid))==1
  fresh_b=counter(turn(b,sid));server=update(server,projectIds=None);assert server['projectIds'] is None and server['enabled'];dead(fresh_b['pid']);assert len(inspect(a,sid))==1 and len(inspect(b,sid))==1
  # Refresh completion may not resurrect stale env/scope snapshots.
  server=race_refresh(server,environmentChanges={'KEEPER_TEST_TOKEN':SECRET});assert listed(sid)['environmentRevision']==server['environmentRevision'] and not listed(sid)['enabled'] and listed(sid)['tools']==[];server=refresh(sid)
  server=race_refresh(server,projectIds=['general']);assert listed(sid)['projectIds']==['general'] and listed(sid)['tools']==server['tools'];server=refresh(sid)
  # Validation errors are generic and mutations are atomic.
  baseline=listed(sid)
  for changes in ({'environmentChanges':None},{'environmentChanges':{'1BAD':SECRET}},{'environmentChanges':{'BAD-NAME':SECRET}},{'environmentChanges':{'TOKEN':True}},{'environmentChanges':{'TOKEN':'x'*8193}},{'environmentChanges':{'TOKEN':'nul\0'+SECRET}},{'environmentChanges':{'V_'+str(i):'x' for i in range(33)}},{'environmentChanges':{'BIG_'+str(i):'x'*8192 for i in range(5)}},{'projectIds':[]},{'projectIds':['general','general']},{'projectIds':['missing']},{'projectIds':['general']*129}):
   call('agent_mcp_save',{key:baseline[key] for key in ('id','name','transport','command','args')}|changes,400);assert listed(sid)==baseline
  # Remote transports never accept stdio env values, and switching clears them.
  remote_config={'id':sid,'name':'Remote replacement','transport':'http','command':f'http://127.0.0.1:{remote.server_port}/mcp','args':[]}
  call('agent_mcp_save',{**remote_config,'environmentChanges':{'TOKEN':SECRET}},400);assert listed(sid)==baseline and remote_calls==[]
  server=update(baseline,**{key:value for key,value in remote_config.items() if key!='id'});assert server['environmentKeys']==[] and server['tools']==[] and not server['enabled'];assert server['environmentRevision']!=baseline['environmentRevision'];server=refresh(sid);assert server['environmentKeys']==[] and server['enabled']
  # All public snapshots/reviews and backups remain value-free; SQLite owns private config.
  backup=call('agent_history_backup');serialized=json.dumps(backup)
  for secret in (SECRET,SECRET2,HOST_SECRET):assert secret not in serialized and secret not in json.dumps(call('agent_session',{'sessionId':a}))
  with db() as conn:assert json.loads(conn.execute('SELECT environment FROM agent_mcp_servers WHERE id=?',(sid,)).fetchone()[0])=={}
  stop();start();server=listed(sid);assert server['projectIds']==['general'] and server['environmentKeys']==[]
  assert not model_errors,model_errors
  print('PASS: legacy migration; private child environment and host isolation; patch/no-op/blank/delete semantics; redacted APIs/models/reviews/backups; project-scoped schemas and pre-approval enforcement; owner isolation; pool/approval invalidation; stale refresh guards; atomic validation; remote-env rejection/clear; restart persistence.',flush=True)
  if '--preview' in sys.argv:
   server=update(server,transport='stdio',command=sys.executable,args=config['args'],environmentChanges={'KEEPER_TEST_TOKEN':SECRET,'EMPTY':''},projectIds=['general']);server=refresh(sid);print('MCP project/environment preview:',base,'session:',a,flush=True);input('Press Enter after browser verification: ')
 finally:
  gate.unlink(missing_ok=True);stop();log.close()
  for server in (model,remote):server.shutdown();server.server_close()
