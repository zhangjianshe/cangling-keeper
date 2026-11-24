#!/usr/bin/env python3
"""Isolated Earth storage, account, capability-path and fake-model integration checks."""
import json,os,tempfile,subprocess,socket,sqlite3,time,urllib.request,urllib.error,threading,http.server
from pathlib import Path
calls=[]
class Model(http.server.BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));calls.append(body)
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
  count=sum(m['role']=='tool' for m in body['messages'])
  actions=[('earth_fly_to',{'longitude':116.4,'latitude':39.9,'height':1000000}),('earth_add_geojson_layer',{'path':'demo.geojson'}),('earth_get_view',{}),('earth_get_selection',{}),('earth_set_layer_visibility',{'layer':'graticule','visible':True})]
  if count<len(actions):
   name,args=actions[count];delta={'tool_calls':[{'index':0,'id':'earth'+str(count),'type':'function','function':{'name':name,'arguments':json.dumps(args)}}]};reason='tool_calls'
  else:delta={'content':'场景已保存，等待浏览器渲染确认。'};reason='stop'
  self.wfile.write(('data: '+json.dumps({'choices':[{'delta':delta,'finish_reason':reason}]})+'\n\ndata: [DONE]\n\n').encode())
model=http.server.ThreadingHTTPServer(('127.0.0.1',0),Model);threading.Thread(target=model.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-earth-') as tmp:
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}';proc=None;root=Path(tmp)/'project';root.mkdir()
 def call(name,args={},code=200):
  req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args).encode(),headers={'Content-Type':'application/json','Origin':base})
  try:
   with urllib.request.urlopen(req,timeout=10) as r:status=r.status;v=json.load(r)
  except urllib.error.HTTPError as e:status=e.code;v=json.load(e)
  assert status==code,(name,status,v);return v.get('result',v)
 def start():
  global proc
  proc=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',tmp,'--no-open'],env={**os.environ,'TMPDIR':tmp},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  for _ in range(100):
   try:call('agent_bootstrap');return
   except Exception:time.sleep(.05)
  raise AssertionError('startup')
 def stop():
  proc.terminate();proc.wait(10)
 try:
  start();stop()
  with sqlite3.connect(Path(tmp)/'data.sql') as db:db.execute("UPDATE agent_projects SET directory=? WHERE id='general'",(str(root),))
  (root/'demo.geojson').write_text(json.dumps({'type':'Feature','properties':{'name':'测试点','html':'<script>alert(1)</script>'},'geometry':{'type':'Point','coordinates':[116.4,39.9]}}))
  (Path(tmp)/'secret.geojson').write_text((root/'demo.geojson').read_text());(root/'link.geojson').symlink_to(Path(tmp)/'secret.geojson')
  start();args={'projectId':'general','accountKey':'guest'};scene=call('agent_earth_get',args);assert scene['revision']==0
  scene['camera']['longitude']=121.;saved=call('agent_earth_save',{**args,'expectedRevision':0,'scene':scene});assert saved['renderedRevision'] is None
  call('agent_earth_save',{**args,'expectedRevision':0,'scene':scene},400)
  call('agent_earth_ack',{**args,'revision':0},400)
  assert call('agent_earth_ack',{**args,'revision':1})['renderedRevision']==1
  for path in ['../secret.geojson','link.geojson','/etc/passwd']:
   call('agent_earth_import',{**args,'expectedRevision':1,'path':path},400)
  invalid=json.loads(json.dumps(scene));invalid['camera']['latitude']=91
  call('agent_earth_save',{**args,'expectedRevision':1,'scene':invalid},400)
  (root/'bad.geojson').write_text(json.dumps({'type':'Point','coordinates':[181,90]}))
  call('agent_earth_import',{**args,'expectedRevision':1,'path':'bad.geojson'},400)
  imported=call('agent_earth_import',{**args,'expectedRevision':1,'path':'demo.geojson'});assert len(imported['overlays'])==1
  stop();start();assert call('agent_earth_get',args)==imported
  # Public positioning API preserves scene and height, validates coordinates and revisions.
  located=call('agent_earth_fly_to',{**args,'expectedRevision':imported['revision'],'longitude':120,'latitude':30})
  assert located['camera']['height']==imported['camera']['height'] and located['overlays']==imported['overlays']
  for extra in [{'latitude':91},{'longitude':181},{'height':0},{'accountKey':'stale'},{'expectedRevision':0}]:
   call('agent_earth_fly_to',{**args,'expectedRevision':located['revision'],'longitude':120,'latitude':30,**extra},400)
  assert call('agent_earth_get',args)==located
  imported=located
  # Changing active identity cannot overwrite the previous account's scene.
  with sqlite3.connect(Path(tmp)/'data.sql') as db:
   for k,v in [('login_token','fixture'),('login_username','other'),('server_url','http://localhost')]:db.execute('INSERT OR REPLACE INTO settings(key,value) VALUES (?,?)',(k,v))
  assert call('agent_earth_get',args)['revision']==0
  call('agent_earth_save',{**args,'expectedRevision':0,'scene':scene},400)
  with sqlite3.connect(Path(tmp)/'data.sql') as db:db.execute("DELETE FROM settings WHERE key='login_token'")
  assert call('agent_earth_get',args)==imported
  mid=call('agent_model_save',{'name':'Earth Fixture','enabled':True,'baseUrl':f'http://127.0.0.1:{model.server_port}','model':'earth-test','apiKey':'fixture'})['savedServerId'];call('agent_model_select',{'id':mid})
  sid=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':sid,'content':'展示测试点并打开经纬网'})
  for _ in range(300):
   detail=call('agent_session',{'sessionId':sid})
   if not detail['modelPending']:break
   time.sleep(.05)
  assert detail['tasks'][-1]['status']=='complete',detail['tasks']
  assert len(detail['runs'])==5 and all(r['status']=='succeeded' for r in detail['runs']),detail['runs']
  result=call('agent_earth_get',args);assert result['camera']['longitude']==116.4 and result['layers']['graticule'] and result['renderedRevision'] is None
  assert len(calls)==6
  tool_text=[m['content'] for r in calls for m in r['messages'] if m['role']=='tool'];assert any('saved_not_rendered' in t for t in tool_text)
  with urllib.request.urlopen(base+'/earth/pkg/cangling_desktop_bg.wasm') as r:assert r.headers['Content-Type'].startswith('application/wasm') and r.read(4)==b'\x00asm'
  print('PASS Earth: restart persistence, revisions, account isolation, traversal/symlinks, invalid coordinates, five tools through ReAct, WASM MIME')
 finally:
  if proc and proc.poll() is None:stop()
  model.shutdown()
