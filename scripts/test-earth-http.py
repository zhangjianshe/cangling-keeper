#!/usr/bin/env python3
"""Isolated Earth storage, account, capability-path and fake-model integration checks."""
import json,os,tempfile,subprocess,socket,sqlite3,time,urllib.request,urllib.error,threading,http.server
from pathlib import Path
calls=[]
action_index=0
fixture_layer=None
import hashlib
skill_text=Path('skills/earth-workbench/SKILL.md').read_text()
skill_sha=hashlib.sha256(skill_text.encode()).hexdigest();skill_steps=[];offset=0
while offset<len(skill_text):
 skill_steps.append(('load_skill',{'name':'system:earth-workbench','offset':offset,**({'expectedSha256':skill_sha} if offset else {})}))
 size=0;end=offset
 while end<len(skill_text) and size+len(skill_text[end].encode())<=2200:size+=len(skill_text[end].encode());end+=1
 offset=end

class Model(http.server.BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def do_GET(self):
  assert self.path.startswith('/api/3dtiles/discover'),self.path
  self.send_response(200);self.send_header('Content-Type','application/json');self.end_headers()
  self.wfile.write(json.dumps({'datasets':[{'id':'fixture-buildings','name':'测试建筑','version':'a'*64,'bounds':[113.7,34.7,113.8,34.8]}],'total':1,'truncated':False}).encode())
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));calls.append(body)
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
  global action_index,fixture_layer
  for message in body["messages"]:
   if message["role"]=="tool":
    try:
     content=json.loads(message["content"])
     for layer in content.get("tilesetLayers",[]):
      if layer.get("datasetId")=="fixture-buildings":fixture_layer=layer["id"]
    except (ValueError,AttributeError):pass
  if not body.get('tools'):
   self.wfile.write(('data: '+json.dumps({'choices':[{'delta':{'content':'测试场景执行中，继续下一步骤。'},'finish_reason':'stop'}]})+'\n\ndata: [DONE]\n\n').encode());return
  count=action_index;action_index+=1
  actions=skill_steps+[('earth_search_datasets',{'query':'测试建筑'}),('earth_load_dataset',{'datasetId':'fixture-buildings'}),('earth_load_dataset',{'datasetId':'fixture-buildings'}),('earth_set_layer_visibility',{'layer':fixture_layer or 'missing','visible':False}),('earth_remove_tileset',{'layer':fixture_layer or 'missing'}),('earth_load_dataset',{'datasetId':'fixture-buildings'}),('earth_add_tileset',{'url':'http://localhost:8080/3dtiles/agent/tileset.json'}),('earth_publish_model',{'path':'demo.glb'}),('earth_add_model',{'path':'demo.glb','longitude':120,'latitude':30}),('earth_publish_image',{'path':'overlay.png'}),('earth_add_image_layer',{'path':'overlay.png','polygon':[[120,30],[121,30],[121,31],[120,31]]}),('earth_fly_to',{'longitude':116.4,'latitude':39.9,'height':1000000}),('earth_add_geojson_layer',{'path':'demo.geojson'}),('earth_get_view',{}),('earth_get_selection',{}),('earth_set_layer_visibility',{'layer':'graticule','visible':True})]
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
  proc=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',tmp,'--no-open'],env={**os.environ,'TMPDIR':tmp,'KEEPER_EARTH_MAP_URL':f'http://127.0.0.1:{model.server_port}/'},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
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
  # Camera persistence never increments the layer revision; stale layer saves cannot rewind it.
  initial=call('agent_earth_get',args)
  cam={**initial['camera'],'longitude':118.}
  moved=call('agent_earth_camera_save',{**args,'expectedCameraCommandRevision':0,'camera':cam})
  assert moved['revision']==1 and moved['cameraRevision']==1 and moved['camera']==cam
  # SSE initial resync and live scoped metadata, with no scene body or polling.
  stream=urllib.request.urlopen(urllib.request.Request(base+'/api/earth/events?project=general&account=guest',headers={'Origin':base}),timeout=3)
  def event():
   while True:
    line=stream.readline().decode().strip()
    if line.startswith('data: '):return json.loads(line[6:])
  assert event()=={'resync':True}
  moved=call('agent_earth_camera_save',{**args,'expectedCameraCommandRevision':0,'camera':{**cam,'longitude':119.}})
  notice=event();assert notice=={'revision':1,'cameraRevision':2,'cameraCommandRevision':0}
  stream.close()
  try:urllib.request.urlopen(base+'/api/earth/events?project=general&account=other',timeout=3);raise AssertionError('scope')
  except urllib.error.HTTPError as e:assert e.code==403
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
  assert located['revision']==imported['revision'] and located['cameraCommandRevision']==1
  call('agent_earth_camera_save',{**args,'expectedCameraCommandRevision':0,'camera':cam},400)
  assert located['camera']['height']==imported['camera']['height'] and located['overlays']==imported['overlays']
  for extra in [{'latitude':91},{'longitude':181},{'height':0},{'accountKey':'stale'},{'expectedRevision':0}]:
   call('agent_earth_fly_to',{**args,'expectedRevision':located['revision'],'longitude':120,'latitude':30,**extra},400)
  assert call('agent_earth_get',args)==located
  imported=located
  # Published image snapshots are durable, bounded and account scoped.
  import base64
  png=base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAAAF0lEQVR4nGP8z0AaYCJR/aiGUQ1DSAMAQC4BH2bjRnMAAAAASUVORK5CYII=')
  (root/'overlay.png').write_bytes(png)
  asset=call('agent_earth_publish_image',{**args,'path':'overlay.png'})
  assert call('agent_earth_publish_image',{**args,'path':'overlay.png'})['id']==asset['id']
  with urllib.request.urlopen(base+asset['downloadUrl']) as r:
   assert r.read()==png and r.headers['Content-Type']=='image/png' and 'attachment' in r.headers['Content-Disposition']
  for path in ['../overlay.png','link.geojson','demo.geojson']:
   call('agent_earth_publish_image',{**args,'path':path},400)
  for polygon in [[[0,0],[1,1],[0,1],[1,0]],[[0,0],[1,1],[2,2]],[[179,0],[-179,0],[179,1]]]:
   call('agent_earth_add_image',{**args,'expectedRevision':imported['revision'],'path':'overlay.png','polygon':polygon},400)
  imported=call('agent_earth_add_image',{**args,'expectedRevision':imported['revision'],'path':'overlay.png','polygon':[[120,30],[121,30],[121,31],[120,31]]})
  assert imported['images'][0]['id']==asset['id'] and imported['renderedRevision'] is None
  stop();start();assert call('agent_earth_get',args)==imported
  with urllib.request.urlopen(base+asset['url']) as r:assert r.read()==png
  # Static GLB publication, placement, restoration and bounded project access.
  import struct
  raw=struct.pack('<9f',0,0,0,1,0,0,0,1,0)
  doc={'asset':{'version':'2.0'},'buffers':[{'byteLength':len(raw)}],'bufferViews':[{'buffer':0,'byteLength':len(raw)}],'accessors':[{'bufferView':0,'componentType':5126,'count':3,'type':'VEC3','min':[0,0,0],'max':[1,1,0]}],'meshes':[{'primitives':[{'attributes':{'POSITION':0}}]}],'nodes':[{'mesh':0}],'scenes':[{'nodes':[0]}],'scene':0}
  payload=json.dumps(doc).encode();payload+=b' '*((-len(payload))%4)
  glb=b'glTF'+struct.pack('<II',2,28+len(payload)+len(raw))+struct.pack('<I4s',len(payload),b'JSON')+payload+struct.pack('<I4s',len(raw),b'BIN\0')+raw
  (root/'demo.glb').write_bytes(glb)
  model_asset=call('agent_earth_publish_model',{**args,'path':'demo.glb'})
  with urllib.request.urlopen(base+model_asset['downloadUrl']) as r:
   assert r.read()==glb and r.headers['Content-Type']=='model/gltf-binary' and 'attachment' in r.headers['Content-Disposition']
  assert call('agent_earth_publish_model',{**args,'path':'demo.glb'})['id']==model_asset['id']
  current=call('agent_earth_get',args)
  placed=call('agent_earth_add_model',{**args,'path':'demo.glb','longitude':120,'latitude':30,'scale':10,'expectedRevision':current['revision']})
  assert placed['models'][0]['height']==0 and placed['models'][0]['altitudeMode']=='relativeToGround'
  for extra in [{'latitude':91},{'scale':0},{'altitudeMode':'invalid'},{'accountKey':'stale'},{'expectedRevision':0}]:
   call('agent_earth_add_model',{**args,'path':'demo.glb','longitude':120,'latitude':30,'expectedRevision':placed['revision'],**extra},400)
  (root/'broken.glb').write_bytes(b'glTFinvalid');(root/'link.glb').symlink_to(root/'demo.glb')
  for path in ['broken.glb','link.glb','../demo.glb','/etc/passwd']:
   call('agent_earth_publish_model',{**args,'path':path},400)
  stop();start();assert call('agent_earth_get',args)==placed;imported=placed
  # Tiles references/status persist without modifying scene revision; stale reports are rejected.
  placed=call('agent_earth_add_tileset',{**args,'expectedRevision':placed['revision'],'url':'http://localhost:8080/3dtiles/demo/tileset.json','name':'测试瓦片'})
  lid=placed['tilesets'][0]['id'];rev=placed['revision']
  for extra in [{'accountKey':'stale'},{'expectedRevision':0},{'url':'file:///tmp/tileset.json'}]:
   call('agent_earth_add_tileset',{**args,'expectedRevision':rev,'url':'http://localhost:8080/3dtiles/other/tileset.json',**extra},400)
  report=call('agent_earth_tiles_status',{**args,'revision':rev,'layers':[{'id':lid,'drawing':2,'pending':0,'errors':[]}]})
  assert call('agent_earth_get',args)['revision']==rev
  call('agent_earth_tiles_status',{**args,'revision':0,'layers':[]},400)
  call('agent_earth_tiles_status',{**args,'revision':rev,'layers':[{'id':'other'}]},400)
  imported=call('agent_earth_get',args);stop();start();assert call('agent_earth_get',args)==imported
  # Changing active identity cannot overwrite the previous account's scene.
  with sqlite3.connect(Path(tmp)/'data.sql') as db:
   for k,v in [('login_token','fixture'),('login_username','other'),('server_url','http://localhost')]:db.execute('INSERT OR REPLACE INTO settings(key,value) VALUES (?,?)',(k,v))
  try:urllib.request.urlopen(base+model_asset['url']);raise AssertionError('cross account model access')
  except urllib.error.HTTPError as e:assert e.code==404
  try:urllib.request.urlopen(base+asset['url']);raise AssertionError('cross account image access')
  except urllib.error.HTTPError as e:assert e.code==404
  assert call('agent_earth_get',{'projectId':'general'})['revision']==0
  call('agent_earth_save',{**args,'expectedRevision':0,'scene':scene},400)
  call('agent_earth_get',args,400)  # detached window pins the original account
  with sqlite3.connect(Path(tmp)/'data.sql') as db:db.execute("DELETE FROM settings WHERE key='login_token'")
  assert call('agent_earth_get',args)==imported
  mid=call('agent_model_save',{'name':'Earth Fixture','enabled':True,'baseUrl':f'http://127.0.0.1:{model.server_port}','model':'earth-test','apiKey':'fixture'})['savedServerId'];call('agent_model_select',{'id':mid})
  sid=call('agent_new_session')['id'];call('agent_execution_budget',{'sessionId':sid,'maxRounds':32});call('agent_send_message',{'sessionId':sid,'content':'展示测试点并打开经纬网'})
  for _ in range(300):
   detail=call('agent_session',{'sessionId':sid})
   if not detail['modelPending']:break
   time.sleep(.05)
  assert detail['tasks'][-1]['status']=='complete',detail['tasks']
  assert len(detail['runs'])==16+len(skill_steps) and all(r['status']=='succeeded' for r in detail['runs']),detail['runs']
  result=call('agent_earth_get',args);assert result['camera']['longitude']==116.4 and result['layers']['graticule'] and result['renderedRevision'] is None
  assert len([c for c in calls if c.get('tools')])==17+len(skill_steps)
  catalog_layers=[l for l in result['tilesets'] if l.get('datasetId')=='fixture-buildings'];assert len(catalog_layers)==1 and catalog_layers[0]['datasetVersion']=='a'*64
  stop();start();assert call('agent_earth_get',args)['tilesets']==result['tilesets']
  loaded=[json.loads(r['output']) for r in detail['runs'] if r['tool']=='load_skill'][0]
  assert loaded['scope']=='system' and loaded['instructions'] and loaded['nextOffset']>0
  pages=[json.loads(r['output']) for r in detail['runs'] if r['tool']=='load_skill']
  assert ''.join(p['instructions'] for p in sorted(pages,key=lambda p:p['offset']))==skill_text
  tool_text=[m['content'] for r in calls for m in r['messages'] if m['role']=='tool'];assert any('saved_not_rendered' in t for t in tool_text)
  with urllib.request.urlopen(base+'/earth/pkg/cangling_desktop_bg.wasm') as r:assert r.headers['Content-Type'].startswith('application/wasm') and r.read(4)==b'\x00asm'
  print('PASS Earth: restart persistence, revisions, account isolation, traversal/symlinks, invalid coordinates, catalog search/load/idempotence/visibility/removal through ReAct, 3D Tiles storage/status, GLB publication/placement/download, image publication/download, WASM MIME')
 finally:
  if proc and proc.poll() is None:stop()
  model.shutdown()
