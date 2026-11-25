#!/usr/bin/env python3
"""Real local Map MCP + isolated Keeper + deterministic model; no real model key used.
Requires a running upgraded Map with published zhengzhou-cbd-osm and MAP_PUBLIC_BASE_URL.
"""
import http.server,json,os,socket,sqlite3,subprocess,tempfile,threading,time,urllib.request,urllib.error
from pathlib import Path
map_url=os.environ.get('TEST_MAP_URL','http://127.0.0.1:4310/').rstrip('/')
step=0;dataset=None
class Model(http.server.BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def do_POST(self):
  global step,dataset
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
  for m in body['messages']:
   if m['role']=='tool':
    try:
     v=json.loads(m['content']);v=json.loads(v['output']) if 'output' in v else v
     if v.get('datasets'):dataset=v['datasets'][0]
    except (KeyError,ValueError,TypeError):pass
  def mcp(name):return next(t['function']['name'] for t in body['tools'] if '/ '+name+':' in t['function']['description'])
  if step==0:name,args=mcp('search_3d_datasets'),{'query':'郑州'}
  elif step==1:name,args=mcp('get_3d_dataset'),{'datasetId':dataset['id']}
  elif step==2:name,args='earth_add_tileset',{'url':dataset['tilesetUrl'],'name':dataset['name']}
  elif step==3:
   b=dataset['bounds'];name,args='earth_fly_to',{'longitude':(b[0]+b[2])/2,'latitude':(b[1]+b[3])/2,'height':6000}
  elif step==4:name,args='earth_get_view',{}
  else:name,args=None,None
  delta={'tool_calls':[{'index':0,'id':f'call-{step}','type':'function','function':{'name':name,'arguments':json.dumps(args)}}]} if name else {'content':'已保存郑州建筑场景，等待地球页面渲染确认。'}
  step+=1;self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers();self.wfile.write(('data: '+json.dumps({'choices':[{'delta':delta,'finish_reason':'tool_calls' if name else 'stop'}]})+'\n\ndata: [DONE]\n\n').encode())
model=http.server.ThreadingHTTPServer(('127.0.0.1',0),Model);threading.Thread(target=model.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-map-mcp-') as tmp:
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}';proc=None;log=open(Path(tmp)/'log','w')
 def call(name,args={}):
  request=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args).encode(),headers={'Content-Type':'application/json','Origin':base})
  with urllib.request.urlopen(request,timeout=15) as r:return json.load(r)['result']
 def start():
  global proc
  proc=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',tmp,'--no-open'],env={**os.environ,'TMPDIR':tmp,'KEEPER_MAP_MCP_URL':map_url+'/mcp'},stdout=log,stderr=log)
  for _ in range(100):
   try:call('agent_bootstrap');return
   except Exception:time.sleep(.1)
  raise AssertionError('startup')
 def stop():proc.terminate();proc.wait(10)
 try:
  start();servers=call('agent_mcp_list');assert len(servers)==1,servers
  server=servers[0];assert server['enabled'] and server['defaultService'] and len(server['tools'])==2,server
  for _ in range(3):call('agent_bootstrap')
  assert len(call('agent_mcp_list'))==1
  call('agent_model_save',{'enabled':True,'baseUrl':f'http://127.0.0.1:{model.server_port}','model':'fixture','apiKey':'fixture-only'})
  session=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':session,'content':'显示郑州建筑并定位'})
  approved=set()
  for _ in range(400):
   detail=call('agent_session',{'sessionId':session})
   for run in detail['runs']:
    if run['status']=='pending_approval' and run['id'] not in approved:
     assert run['review']['kind']=='mcp';approved.add(run['id']);call('agent_review_tool',{'sessionId':session,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True})
   if not detail['modelPending']:break
   time.sleep(.05)
  assert len(approved)==2 and len(detail['runs'])==5 and all(r['status']=='succeeded' for r in detail['runs']),detail['runs']
  scene=call('agent_earth_get',{'projectId':'general','accountKey':'guest'});assert scene['tilesets'][0]['url']==dataset['tilesetUrl'] and scene['renderedRevision'] is None
  assert 113<scene['camera']['longitude']<114
  # User changes and deletion survive bootstrap and service restart.
  call('agent_mcp_save',{**server,'name':'我的地图','enabled':False});call('agent_bootstrap');assert not call('agent_mcp_list')[0]['enabled']
  stop();start();assert call('agent_mcp_list')[0]['name']=='我的地图' and not call('agent_mcp_list')[0]['enabled']
  call('agent_mcp_delete',{'id':server['id']});stop();start();assert call('agent_mcp_list')==[]
  with sqlite3.connect(Path(tmp)/'data.sql') as db:
   for k,v in [('login_token','fixture'),('login_username','another'),('server_url','http://localhost')]:db.execute('INSERT OR REPLACE INTO settings(key,value) VALUES (?,?)',(k,v))
  call('agent_bootstrap');other=call('agent_mcp_list');assert len(other)==1 and other[0]['id']!=server['id'] and other[0]['enabled']
  assert call('agent_earth_get',{'projectId':'general'})['revision']==0
  print('PASS real Map MCP: default discovery, two approved MCP calls, Earth load/fly/status, restart, user edits/deletion, account isolation')
 finally:
  if proc and proc.poll() is None:stop()
  log.close();model.shutdown();model.server_close()
