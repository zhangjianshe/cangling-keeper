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
  call('agent_project_entry_create',{**args,'parent':'.','name':'AGENTS.md','kind':'file'})
  guide=call('agent_project_browse',{**args,'action':'edit','path':'AGENTS.md'})
  snapshot=call('agent_project_guidance_by_project',args)
  assert '## 操作边界' in snapshot['rules']['content']
  assert guide['editable'] and '## 操作边界' in guide['content'] and '提交消息使用中文' in guide['content']
  (root/'AGENTS.md').write_text('user custom guide')
  try:
   call('agent_project_entry_create',{**args,'parent':'.','name':'AGENTS.md','kind':'file'})
   raise AssertionError('must refuse overwriting guide')
  except urllib.error.HTTPError:pass
  assert (root/'AGENTS.md').read_text()=='user custom guide'
  (root/'.agents/skills/demo').mkdir(parents=True)
  for doc in ['AGENTS.md','.agents/skills/demo/SKILL.md']:
   (root/doc).write_text('original guide')
   document=call('agent_project_browse',{**args,'action':'edit','path':doc})
   assert document['editable'] and document['content']=='original guide'
   call('agent_project_file_save',{**args,'path':doc,'content':'updated guide','sha256':document['sha256']})
   assert (root/doc).read_text()=='updated guide'
  create={**args,'parent':'.','name':'新目录','kind':'directory'}
  assert call('agent_project_entry_create',create)['created'];assert (root/'新目录').is_dir()
  new={**args,'parent':'新目录','name':'hello.py','kind':'file'}
  assert call('agent_project_entry_create',new)['created'];assert (root/'新目录/hello.py').read_bytes()==b''
  (root/'新目录/hello.py').write_text('preserve')
  (root/'outside-dir').symlink_to(Path(tmp),target_is_directory=True)
  for invalid in [new,create,{**new,'name':'../bad'},{**new,'parent':'..'},{**new,'parent':'outside-dir'},{**new,'name':'.env'},{**new,'name':'x/y'},{**new,'name':'bad.'},{**new,'kind':'other'}]:
   try:call('agent_project_entry_create',invalid);raise AssertionError(invalid)
   except urllib.error.HTTPError as e:assert e.code==400
  assert (root/'新目录/hello.py').read_text()=='preserve' and not (Path(tmp)/'hello.py').exists()
  current=call('agent_project_browse',{**args,'action':'edit','path':'sub/readme.txt'});assert current['editable'] and len(current['content'].splitlines())==450
  save={**args,'path':'sub/readme.txt','content':'changed\r\n','sha256':current['sha256']}
  result=call('agent_project_file_save',save);assert result['saved'] and (root/'sub/readme.txt').read_bytes()==b'changed\r\n'
  try:call('agent_project_file_save',{**save,'content':'overwrite'});raise AssertionError('stale hash accepted')
  except urllib.error.HTTPError as e:assert e.code==400
  assert (root/'sub/readme.txt').read_bytes()==b'changed\r\n'
  for path in ['../outside','link','.env','binary']:
   try:call('agent_project_browse',{**args,'action':'edit','path':path});raise AssertionError(path)
   except urllib.error.HTTPError as e:assert e.code==400
  (root/'large.txt').write_text('a'*140000);assert call('agent_project_browse',{**args,'action':'edit','path':'large.txt'})['editable'] is False
  for more in [{'path':'../outside','action':'read'},{'path':'link','action':'read'},{'path':'.env','action':'read'},{'path':'binary','action':'read'},{'projectId':'missing'},{'action':'write'}]:
   try:call('agent_project_browse',{**args,**more});raise AssertionError(more)
   except urllib.error.HTTPError as e:assert e.code==400
  print('PASS directory browsing, text pagination, missing project, traversal/link/secret/binary/write rejection')
 finally:p.terminate();p.wait()
