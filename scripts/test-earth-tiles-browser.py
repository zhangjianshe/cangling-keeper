#!/usr/bin/env python3
"""Create disposable Map publication + Keeper scene for native browser/GPU LOD QA.
Run from Keeper root with --hold; enter to clean up after browser validation.
Far (height=300000): red parent; near (height=12000): green/blue children.
"""
import json,os,sqlite3,socket,subprocess,tempfile,time,struct,sys
from pathlib import Path
from urllib.request import Request,urlopen
def port():
 with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
def req(base,path,body=None,method=None):
 with urlopen(Request(base+path,data=None if body is None else json.dumps(body).encode(),method=method,headers={'Content-Type':'application/json','Origin':base}),timeout=10) as r:return json.load(r)
def glb(left,right,color):
 points=[left,0,-3000,right,0,-3000,right,0,3000,left,0,-3000,right,0,3000,left,0,3000];binary=struct.pack('<18f',*points)
 d={'asset':{'version':'2.0'},'buffers':[{'byteLength':len(binary)}],'bufferViews':[{'buffer':0,'byteLength':len(binary)}],'accessors':[{'bufferView':0,'componentType':5126,'count':6,'type':'VEC3','min':[left,0,-3000],'max':[right,0,3000]}],'meshes':[{'primitives':[{'attributes':{'POSITION':0},'material':0}]}],'materials':[{'doubleSided':True,'pbrMetallicRoughness':{'baseColorFactor':color}}],'nodes':[{'mesh':0}],'scenes':[{'nodes':[0]}],'scene':0}
 raw=json.dumps(d).encode();raw+=b' '*(-len(raw)%4);return b'glTF'+struct.pack('<II',2,28+len(raw)+len(binary))+struct.pack('<I4s',len(raw),b'JSON')+raw+struct.pack('<I4s',len(binary),b'BIN\0')+binary
with tempfile.TemporaryDirectory(prefix='native-tiles-') as temp:
 root=Path(temp);source=root/'data/demo/source';source.mkdir(parents=True);nested=source/'nested';nested.mkdir()
 for name,left,right,color in [('parent',-3000,3000,[1,0,0,1]),('left',-3000,0,[0,1,0,1]),('right',0,3000,[0,0,1,1])]:
  (source/(name+'.glb')).write_bytes(glb(left,right,color))
 # One child is b3dm inside an external tileset to exercise both formats.
 f=b'{"BATCH_LENGTH":0,"RTC_CENTER":[0,0,0]}';f+=b' '*(-(28+len(f))%8);g=(source/'right.glb').read_bytes();(nested/'right.b3dm').write_bytes(b'b3dm'+struct.pack('<6I',1,28+len(f)+len(g),len(f),0,0,0)+f+g)
 tile=lambda uri,error=0:{'boundingVolume':{'sphere':[0,0,0,5000]},'geometricError':error,'refine':'REPLACE','content':{'uri':uri}}
 (nested/'tileset.json').write_text(json.dumps({'asset':{'version':'1.0'},'geometricError':0,'root':tile('right.b3dm')}))
 t=tile('parent.glb',1000);t['transform']=[0,1,0,0,0,0,1,0,1,0,0,0,6378637,0,0,1];t['children']=[tile('left.glb'),tile('nested/tileset.json')]
 (source/'tileset.json').write_text(json.dumps({'asset':{'version':'1.1'},'geometricError':1000,'root':t}))
 with sqlite3.connect(root/'places.db') as db:db.execute('CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT)');db.execute("INSERT INTO metadata VALUES('schema_version','1')")
 mp,kp=port(),port();mb,kb=f'http://127.0.0.1:{mp}',f'http://127.0.0.1:{kp}'
 env={**os.environ,'MAP_SERVER_ADDRESS':f'127.0.0.1:{mp}','CATALOG_DATABASE_URL':f'sqlite://{root}/catalog.db?mode=rwc','IMAGERY_DATABASE_URL':f'sqlite://{root}/imagery.db?mode=rwc','IMAGERY_INDEX_DATABASE_URL':f'sqlite://{root}/index.db?mode=rwc','PLACENAMES_DATABASE_URL':f'sqlite://{root}/places.db?mode=ro','WMTS_CACHE_BACKEND':'filesystem','WMTS_CACHE_PATH':str(root/'cache'),'TILES3D_DATA_ROOT':str(root/'data')}
 processes=[];log=open(root/'run.log','w')
 try:
  processes.append(subprocess.Popen([str(Path('../cangling-map/target/debug/map-server').resolve())],cwd=root,env=env,stdout=log,stderr=log))
  processes.append(subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{kp}','--data-dir',str(root/'keeper'),'--no-open'],env={**os.environ,'TMPDIR':temp},stdout=log,stderr=log))
  for base,path in [(mb,'/health'),(kb,'/api/invoke/agent_bootstrap')]:
   for _ in range(100):
    try:
     if base==kb:req(base,path,{})
     else:urlopen(base+path,timeout=2).close()
     break
    except Exception:time.sleep(.1)
   else:raise AssertionError((root/'run.log').read_text())
  req(mb,'/api/3dtiles/demo',{'title':'原生LOD测试'},'PUT');req(mb,'/api/3dtiles/demo/publish',{},'POST')
  def call(name,args):return req(kb,'/api/invoke/'+name,{'projectId':'general','accountKey':'guest',**args})['result']
  scene=call('agent_earth_add_tileset',{'expectedRevision':0,'url':mb+'/3dtiles/demo/tileset.json','name':'原生LOD：远红近绿蓝'})
  call('agent_earth_fly_to',{'expectedRevision':scene['revision'],'longitude':0,'latitude':0,'height':300000})
  state={'keeper':kb,'map':mb,'projectId':'general','layerId':scene['tilesets'][0]['id']}
  Path('/tmp/keeper-tiles-browser.json').write_text(json.dumps(state));print(json.dumps(state),flush=True)
  print('BROWSER_URL='+kb+'/earth-window.html?project=general',flush=True)
  if '--hold' in sys.argv:input('Enter after browser QA to clean up: ')
  print('PASS native tiles fixture publication and Keeper scene integration')
 finally:
  for p in processes:p.terminate()
  for p in processes:p.wait(15)
  log.close()
