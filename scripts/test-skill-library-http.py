#!/usr/bin/env python3
"""Agent presets: real HTTP/SSE loop with local fake model; never calls paid providers."""
import http.server,json,os,socket,sqlite3,subprocess,tempfile,threading,time,urllib.request,urllib.error
from pathlib import Path
requests=[];hold=threading.Event();started=threading.Event();tool_calls=[]
class Model(http.server.BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body)
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
  def emit(delta,reason=None):self.wfile.write(('data: '+json.dumps({'choices':[{'delta':delta,'finish_reason':reason}]})+'\n\n').encode());self.wfile.flush()
  try:
   if not any(m['role']=='tool' for m in body['messages']):
    started.set();hold.wait(10)
    calls=list(tool_calls)
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
  start();proc.terminate();proc.wait(10);root=Path(tmp)/'project';root.mkdir();root2=Path(tmp)/'project2';root2.mkdir()
  for folder in (root,root2):
   skill=folder/'.agents/skills/same';skill.mkdir(parents=True);(skill/'SKILL.md').write_text('---\nname: same\ndescription: Project guide\n---\nPROJECT-'+folder.name)
  with sqlite3.connect(Path(tmp)/'data.sql') as db:
   db.execute("UPDATE agent_projects SET directory=? WHERE id='general'",(str(root),))
   db.execute("INSERT INTO agent_projects VALUES ('second','Second',?,0)",(str(root2),))
  start();s=call('agent_new_session')['id'];other=call('agent_new_session',{'projectId':'second'})['id']
  model=call('agent_model_save',{'name':'Skills fixture','enabled':True,'baseUrl':f'http://127.0.0.1:{api.server_port}','model':'skills-fixture','apiKey':'fixture'})['savedServerId']
  preset={'sessionId':s,'name':'跨项目智能体','systemPrompt':'Use associated skills','modelId':model,'skills':['same'],'mcpTools':[]}
  p=call('agent_profile_save',preset);preset['id']=p['id']
  with sqlite3.connect(Path(tmp)/'data.sql') as db:
   legacy={**p,'skills':['same']};db.execute('UPDATE agent_profiles SET config=? WHERE id=?',(json.dumps(legacy),p['id']))
  assert call('agent_profile_options',{'sessionId':other})['profiles'][0]['skills']==['project:general:same']
  p2=call('agent_profile_save',{**preset,'id':'','name':'Other agent','skills':[]})
  def skill_payload(scope='account',body='VERSION-ONE'):
   return {'sessionId':s,'scope':scope,'profileId':p['id'],'name':'same','description':'Shared guide','directories':['references/empty/nested'],'files':{'SKILL.md':'---\nname: same\ndescription: Shared guide\n---\n'+body,'references/example.md':'RESOURCE-ONE','scripts/demo.py':'print("demo")'}}
  shared=call('agent_library_skill_save',skill_payload());private=call('agent_library_skill_save',skill_payload('agent','PRIVATE'))
  assert shared['ref']!=private['ref']
  read=call('agent_library_skill_read',{'sessionId':s,'id':shared['id']});assert set(read['directories'])=={'scripts','references','assets','references/empty','references/empty/nested'}
  for paths in [['references/../escape'],['scripts/demo.py'],['SKILL.md'],['/tmp/x']]:
   call('agent_library_skill_save',{**skill_payload(),'directories':paths},400)
  call('agent_library_skill_save',{**skill_payload(),'files':{'SKILL.md':'invalid'}},400)
  call('agent_library_skill_save',{**skill_payload(),'files':{**skill_payload()['files'],'references/../../escape':'x'}},400)
  call('agent_library_skill_save',{**skill_payload('agent'),'profileId':'foreign'},400)
  call('agent_profile_save',{**preset,'id':p2['id'],'skills':[private['ref']]},400)
  refs=[shared['ref'],private['ref'],'project:general:same'];preset['skills']=refs
  call('agent_profile_save',preset)
  assert len(call('agent_profile_options',{'sessionId':other})['profiles'])==2
  call('agent_profile_save',{**preset,'sessionId':other}) # Retain foreign-project association without rebinding.
  call('agent_profile_select',{'sessionId':s,'id':p['id']})
  tool_calls[:]=[('load_skill',{'name':shared['ref']}),('read_skill_file',{'name':shared['ref'],'path':'references/example.md'}),('load_skill',{'name':private['ref']}),('load_skill',{'name':'same'}),('load_skill',{'name':'project:general:same'}),('read_skill_file',{'name':shared['ref'],'path':'../secret'})]
  call('agent_send_message',{'sessionId':s,'content':'Use skills'});assert started.wait(5)
  newer=call('agent_library_skill_save',{**skill_payload(body='VERSION-TWO'),'id':shared['id'],'expectedRevision':shared['revision']})
  call('agent_library_skill_save',{**skill_payload(),'id':shared['id'],'expectedRevision':shared['revision']},400)
  hold.set();d=wait(s);assert d['tasks'][-1]['status']=='complete',d['tasks']
  assert [r['status'] for r in d['runs']]==['succeeded','succeeded','succeeded','failed','succeeded','failed'],d['runs']
  assert 'VERSION-ONE' in d['runs'][0]['output'] and 'VERSION-TWO' not in d['runs'][0]['output']
  assert 'RESOURCE-ONE' in d['runs'][1]['output'] and 'PRIVATE' in d['runs'][2]['output']
  answer=next(m for m in d['messages'] if m['role']=='assistant');bindings=answer['modelMetadata']['skillBindings'];assert len(bindings)==3
  assert any(t['function']['name']=='read_skill_file' for t in requests[0]['tools'])
  assert shared['ref'] in requests[0]['messages'][0]['content'] and 'VERSION-ONE' not in requests[0]['messages'][0]['content']
  call('agent_regenerate',{'sessionId':s,'messageId':answer['id']});d=wait(s)
  assert sum('VERSION-ONE' in r['output'] for r in d['runs'])==2
  # Same account, another project: shared/private travel, first project's skill is not exposed.
  call('agent_profile_select',{'sessionId':other,'id':p['id']})
  tool_calls[:]=[('load_skill',{'name':shared['ref']}),('load_skill',{'name':private['ref']}),('load_skill',{'name':'project:general:same'}),('load_skill',{'name':'project:second:same'})]
  call('agent_send_message',{'sessionId':other,'content':'Across projects'});d=wait(other)
  assert [r['status'] for r in d['runs']]==['succeeded','succeeded','failed','failed'],d['runs']
  assert 'VERSION-TWO' in d['runs'][0]['output']
  meta=next(m for m in d['messages'] if m['role']=='assistant')['modelMetadata'];assert len(meta['skillBindings'])==2 and meta['projectGuidance']['warnings']
  # IDs do not bypass account ownership, even with a valid local session.
  with sqlite3.connect(Path(tmp)/'data.sql') as db:db.execute("UPDATE agent_skill_library SET owner='foreign' WHERE id=?",(shared['id'],))
  call('agent_library_skill_read',{'sessionId':s,'id':shared['id']},400)
  call('agent_library_skill_save',{**skill_payload(),'id':shared['id'],'expectedRevision':newer['revision']},400)
  assert not any(x.get('id')==shared['id'] for x in call('agent_profile_options',{'sessionId':s})['skills'])
  with sqlite3.connect(Path(tmp)/'data.sql') as db:db.execute("UPDATE agent_skill_library SET owner='guest' WHERE id=?",(shared['id'],))
  general=call('agent_new_session')['id'];call('agent_session_model',{'sessionId':general,'id':model})
  tool_calls[:]=[('load_skill',{'name':shared['ref']})]
  call('agent_send_message',{'sessionId':general,'content':'General assistant snapshot'});gd=wait(general)
  ga=next(m for m in gd['messages'] if m['role']=='assistant');assert len(ga['modelMetadata']['skillBindings'])==2
  extra=call('agent_library_skill_save',{**skill_payload(),'name':'extra','files':{'SKILL.md':'---\nname: extra\ndescription: Shared guide\n---\nNew skill'}})
  tool_calls[:]=[('load_skill',{'name':extra['ref']})]
  call('agent_regenerate',{'sessionId':general,'messageId':ga['id']});gd=wait(general)
  assert gd['runs'][-1]['status']=='failed'
  proc.terminate();proc.wait(10);start()
  read=call('agent_library_skill_read',{'sessionId':other,'id':shared['id']});assert read['revision']==newer['revision'] and 'VERSION-TWO' in read['files']['SKILL.md']
  assert 'references/empty/nested' in read['directories']
  assert call('agent_session',{'sessionId':other})['agentProfile']['id']==p['id']
  print('PASS skill scopes: migration, cross-project profiles, shared/private/project isolation, immutable generation/regeneration, lazy resources, conflicts, path guards and restart')
 finally:
  hold.set()
  if proc and proc.poll() is None:proc.terminate();proc.wait(10)
  api.shutdown()
