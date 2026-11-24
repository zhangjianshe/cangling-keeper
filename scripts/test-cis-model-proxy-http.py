#!/usr/bin/env python3
"""CIS model catalog and proxy routing; all identities and keys are disposable."""
import base64,http.server,json,os,socket,sqlite3,subprocess,tempfile,threading,time,urllib.request,urllib.error
from pathlib import Path
seen=[];admin_writes=[];models=[{'id':'shared','name':'CIS Shared','provider':'deepseek','model':'central-v1','enabled':True,'contextLimit':32768,'revision':'revision-1','supportsImages':True,'baseUrl':'http://provider.invalid','hasApiKey':True}]
class CIS(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def payload(self,value):
  self.send_response(200);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(json.dumps({'success':True,'code':200,'data':value}).encode())
 def do_GET(self):
  if self.path.endswith('/keeper/models/admin/usage'):
   if self.headers.get('API-TOKEN')!='alice-token':self.send_response(403);self.end_headers();return
   self.payload({'rows':[{'userId':1,'userName':'alice','model':'central-v1','calls':2,'completed':2,'failed':0,'disconnected':0,'inProgress':0,'missingUsage':0,'promptTokens':30,'completionTokens':5,'totalTokens':35}]})
  elif self.path.endswith('/keeper/models/usage'):
   self.payload({'rows':[{'userName':'mine','calls':2,'totalTokens':35}]})
  elif self.path.endswith('/keeper/models/admin'):
   if self.headers.get('API-TOKEN')!='alice-token':self.send_response(403);self.end_headers();return
   self.payload({'models':models})
  elif self.path.endswith('/keeper/models'):
   assert self.headers.get('API-TOKEN') in ('alice-token','bob-token');self.payload({'models':[{k:v for k,v in m.items() if k not in ('baseUrl','hasApiKey')} for m in models if m['enabled']]})
  else:self.payload([])
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers.get('Content-Length',0))) or b'{}')
  if self.path.endswith('/keeper/models/admin/save'):
   assert self.headers.get('API-TOKEN')=='alice-token';admin_writes.append(body);models.append({'id':'added','name':body['name'],'provider':body['provider'],'model':body['model'],'enabled':body['enabled'],'contextLimit':body['contextLimit'],'revision':'revision-2','baseUrl':body['baseUrl'],'hasApiKey':True});self.payload({'id':'added'});return
  if self.path.endswith('/keeper/models/admin/added/delete'):
   assert self.headers.get('API-TOKEN')=='alice-token';models[:]=[m for m in models if m['id']!='added'];self.payload({});return
  if self.path.endswith('/keeper/login'):
   u=body['userName'];self.payload({'token':u+'-token','userName':u,'nickName':u,'userId':1 if u=='alice' else 2});return
  if self.path.endswith('/keeper/models/shared/chat/completions'):
   if body.get('cis_model_revision')!=models[0]['revision']:
    self.send_response(409);self.end_headers();return
   seen.append({'body':body,'apiToken':self.headers.get('API-TOKEN'),'authorization':self.headers.get('Authorization')})
   if body.get('stream'):
    self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers();self.wfile.write(('data: '+json.dumps({'model':'central-v1','choices':[{'delta':{'content':'CIS reply'},'finish_reason':'stop'}]})+'\n\ndata: [DONE]\n\n').encode());self.wfile.flush()
   else:
    self.send_response(200);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(json.dumps({'choices':[{'finish_reason':'stop','message':{'content':'summary'}}]}).encode())
  else:self.payload([])
api=http.server.ThreadingHTTPServer(('127.0.0.1',0),CIS);threading.Thread(target=api.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-cis-model-') as folder:
 sock=socket.socket();sock.bind(('127.0.0.1',0));port=sock.getsockname()[1];sock.close();base=f'http://127.0.0.1:{port}';process=None
 def call(name,args=None,expected=200):
  req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base})
  try:
   with urllib.request.urlopen(req,timeout=12) as r:status=r.status;value=json.load(r)
  except urllib.error.HTTPError as e:status=e.code;value=json.load(e)
  assert status==expected,(name,status,value);return value.get('result')
 def login(name):call('login',{'serverUrl':f'http://127.0.0.1:{api.server_port}/ib','username':name,'password':'dummy'})
 def done(s):
  for _ in range(300):
   d=call('agent_session',{'sessionId':s})
   if not d['modelPending']:return d
   time.sleep(.03)
  raise AssertionError('timeout')
 try:
  process=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'TMPDIR':folder},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  for _ in range(100):
   try:call('agent_bootstrap');break
   except Exception:time.sleep(.05)
  local=call('agent_model_save',{'name':'Local','enabled':True,'baseUrl':'http://127.0.0.1:9999','model':'local','apiKey':'local-private-fixture'})['savedServerId']
  login('alice');config=call('agent_cis_models_refresh');remote=next(s for s in config['servers'] if s.get('source')=='cis');assert any(s['id']==local for s in config['servers']);assert 'local-private-fixture' not in json.dumps(config)
  assert call('agent_cis_models_admin')['models'][0]['id']=='shared'
  assert call('agent_cis_usage',{'all':False})['rows'][0]['totalTokens']==35
  assert call('agent_cis_usage',{'all':True})['rows'][0]['userName']=='alice'
  saved=call('agent_cis_model_save',{'name':'Managed','provider':'deepseek','model':'m2','baseUrl':'https://provider.example','apiKey':'server-fixture-only','enabled':True,'contextLimit':32768});assert saved['id']=='added' and saved['refreshError'] is None
  assert admin_writes[-1]['apiKey']=='server-fixture-only' and 'local-private-fixture' not in json.dumps(admin_writes)
  assert any(x.get('remoteId')=='added' for x in call('agent_model_config')['servers']);call('agent_cis_model_delete',{'id':'added'});assert not any(x.get('remoteId')=='added' for x in call('agent_model_config')['servers'])
  s=call('agent_new_session')['id'];call('agent_session_model',{'sessionId':s,'id':remote['id']});call('agent_send_message',{'sessionId':s,'content':'hello'});d=done(s);assert d['messages'][-1]['content']=='CIS reply',d['messages'][-1]
  assert seen[-1]['apiToken']=='alice-token' and seen[-1]['authorization'] is None and seen[-1]['body']['cis_model_revision']=='revision-1';assert 'local-private-fixture' not in json.dumps(seen)
  assert d['messages'][-1]['modelMetadata']['model']=='central-v1'
  assert remote['supportsImages'] is True
  with sqlite3.connect(str(Path(folder)/'data.sql')) as db:assert db.execute('SELECT supports_images FROM agent_cis_models WHERE id=?',('shared',)).fetchone()[0]==1
  image_session=call('agent_new_session')['id'];call('agent_session_model',{'sessionId':image_session,'id':remote['id']})
  # Generate valid PNG using stdlib, matching the attachment validator.
  import zlib,struct
  def chunk(k,b):return struct.pack('>I',len(b))+k+b+struct.pack('>I',zlib.crc32(k+b)&0xffffffff)
  png=base64.b64encode(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',1,1,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(b'\0\x20\x80\xc0'))+chunk(b'IEND',b'')).decode()
  req=urllib.request.Request(base+'/api/attachments/upload',data=json.dumps({'sessionId':image_session,'name':'image.png','mimeType':'image/png','data':png}).encode(),headers={'Content-Type':'application/json','Origin':base})
  image=json.load(urllib.request.urlopen(req))['result']
  call('agent_send_message',{'sessionId':image_session,'content':'describe','attachmentIds':[image['id']]});assert done(image_session)['messages'][-1]['content']=='CIS reply'
  assert any(part.get('type')=='image_url' for part in seen[-1]['body']['messages'][-1]['content'])
  original=d['messages'][-1]['id'];before_regenerate=len(seen)
  call('agent_regenerate',{'sessionId':s,'messageId':original});regenerated=done(s)
  assert len(seen)==before_regenerate+1 and regenerated['messages'][-1]['content']=='CIS reply'
  assert regenerated['messages'][-1]['modelMetadata']['regenerateOf']==original
  before=len(seen);models[0]['revision']='revision-changed'
  call('agent_send_message',{'sessionId':s,'content':'stale revision'});stale=done(s);assert len(seen)==before and '409' in json.dumps(stale,ensure_ascii=False)
  call('agent_cis_models_refresh');assert next(x for x in call('agent_model_config')['servers'] if x.get('source')=='cis')['revision']=='revision-changed'
  call('agent_regenerate',{'sessionId':s,'messageId':original},expected=400);assert len(seen)==before
  long=call('agent_new_session')['id'];call('agent_session_model',{'sessionId':long,'id':remote['id']})
  with sqlite3.connect(str(Path(folder)/'data.sql')) as db:
   for i in range(50):db.execute('INSERT INTO agent_messages(id,session_id,role,content,created_at) VALUES (?,?,?,?,?)',(long+str(i),long,'user' if i%2==0 else 'assistant','old '+('x'*900),i))
  call('agent_send_message',{'sessionId':long,'content':'compact history'});compacted=done(long);assert compacted['messages'][-1]['modelMetadata']['context']['status']=='compacted',compacted['messages'][-1]
  assert any(not request['body'].get('stream') and request['apiToken']=='alice-token' and request['authorization'] is None and request['body']['cis_model_revision']=='revision-changed' for request in seen)
  with sqlite3.connect(str(Path(folder)/'data.sql')) as db:assert db.execute('SELECT api_key FROM agent_llm_servers WHERE id=?',(local,)).fetchone()[0]=='local-private-fixture';assert db.execute('SELECT count(*) FROM agent_llm_servers').fetchone()[0]==1
  call('agent_model_output_budget',{'id':remote['id'],'maxOutputTokens':8192});call('agent_model_output_budget',{'id':local,'maxOutputTokens':8192})
  assert next(x for x in call('agent_cis_models_refresh')['servers'] if x['id']==remote['id'])['maxOutputTokens']==8192
  login('bob');call('agent_cis_models_admin',expected=400);call('agent_cis_usage',{'all':True},expected=400);cfg=call('agent_cis_models_refresh');bob=next(x for x in cfg['servers'] if x.get('source')=='cis');assert bob['id']!=remote['id'];assert not any(x['id']==remote['id'] for x in cfg['servers']);call('agent_model_save',{'id':bob['id'],'name':'overwrite','apiKey':'local-private-fixture'},400)
  assert bob['maxOutputTokens']==4096 and next(x for x in cfg['servers'] if x['id']==local)['maxOutputTokens']==4096
  call('agent_model_output_budget',{'id':remote['id'],'maxOutputTokens':8192},expected=400)
  call('logout');assert all(x.get('source')!='cis' for x in call('agent_model_config')['servers'])
  print('PASS: CIS catalog, session selection, proxy authentication/stream, local secret isolation, account switch and logout')
 finally:
  if process and process.poll() is None:process.terminate();process.wait(10)
  api.shutdown();api.server_close()
