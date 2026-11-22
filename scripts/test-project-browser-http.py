import json,os,socket,sqlite3,subprocess,tempfile,time,urllib.request,urllib.error
from pathlib import Path
from contextlib import closing
with tempfile.TemporaryDirectory(prefix='keeper-browser-') as tmp:
 root=Path(tmp)/'project';root.mkdir();(root/'sub').mkdir();(root/'sub/readme.txt').write_text('\n'.join('line '+str(i) for i in range(450)));(root/'.env').write_text('secret');(root/'binary').write_bytes(b'\0abc');(Path(tmp)/'outside').write_text('secret');(root/'link').symlink_to(Path(tmp)/'outside')
 with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
 base=f'http://127.0.0.1:{port}'
 def call(name,args={}):
  req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args).encode(),headers={'Content-Type':'application/json','Origin':base})
  return json.load(urllib.request.urlopen(req,timeout=10))['result']
 def start():
  p=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',tmp,'--no-open'],env={**os.environ,'TMPDIR':tmp},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  for _ in range(100):
   try:call('agent_bootstrap');return p
   except OSError:time.sleep(.05)
  p.terminate();p.wait();raise AssertionError('startup')
 p=start()
 try:
  p.terminate();p.wait()
  with closing(sqlite3.connect(Path(tmp)/'data.sql')) as db,db:db.execute("UPDATE agent_projects SET directory=? WHERE id='general'",(str(root),))
  p=start();args={'projectId':'general'}
  data=call('agent_project_browse',args);names=[e['name'] for e in data['entries']];assert 'sub' in names and '.env' not in names and 'link' not in names
  data=call('agent_project_browse',{**args,'action':'read','path':'sub/readme.txt','startLine':201});assert data['totalLines']==450 and data['lines'][0]['line']==201
  for more in [{'path':'../outside','action':'read'},{'path':'link','action':'read'},{'path':'.env','action':'read'},{'path':'binary','action':'read'},{'projectId':'missing'},{'action':'write'}]:
   try:call('agent_project_browse',{**args,**more});raise AssertionError(more)
   except urllib.error.HTTPError as e:assert e.code==400
  print('PASS directory browsing, text pagination, missing project, traversal/link/secret/binary/write rejection')
 finally:p.terminate();p.wait()
