#!/usr/bin/env python3
"""Session titles, validation and persistence in an isolated local server (no model calls)."""
import json,os,sys,socket,subprocess,tempfile,time,urllib.request,urllib.error
from pathlib import Path
with tempfile.TemporaryDirectory(prefix='keeper-title-') as folder:
 s=socket.socket();s.bind(('127.0.0.1',0));port=s.getsockname()[1];s.close();base=f'http://127.0.0.1:{port}';process=None
 def call(name,args=None,expected=200):
  try:
   with urllib.request.urlopen(urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base}),timeout=10) as r:status=r.status;v=json.load(r)
  except urllib.error.HTTPError as e:status=e.code;v=json.load(e)
  assert status==expected,(name,status,v)
  return v.get('result')
 def start():
  global process
  process=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'HOME':folder,'TMPDIR':folder},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  for _ in range(100):
   try:call('agent_bootstrap');return
   except Exception:time.sleep(.05)
  raise AssertionError('startup')
 def title(s):return next(v['title'] for v in call('agent_bootstrap')['sessions'] if v['id']==s)
 try:
  start();s=call('agent_new_session')['id'];assert title(s)=='新会话'
  call('agent_send_message',{'sessionId':s,'content':'第一条\n消息 '+('中'*150)});assert len(title(s))==128 and '\n' not in title(s)
  call('agent_rename_session',{'sessionId':s,'title':'😀'*128});assert title(s)=='😀'*128
  for bad in ['', ' '*5, 'x'*129, '😀'*129, 'line\nbreak']:
   call('agent_rename_session',{'sessionId':s,'title':bad},400);assert title(s)=='😀'*128
  call('agent_send_message',{'sessionId':s,'content':'后续问题'});assert title(s)=='😀'*128
  other=call('agent_new_session')['id'];call('agent_rename_session',{'sessionId':other,'title':'新会话'});call('agent_send_message',{'sessionId':other,'content':'不要覆盖用户标题'});assert title(other)=='新会话'
  call('agent_rename_session',{'sessionId':'missing','title':'test'},400)
  process.terminate();process.wait(10);start();assert title(s)=='😀'*128 and title(other)=='新会话'
  if '--preview' in sys.argv:
   preview=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':preview,'content':'优化项目的登录页面'});print('Title preview:',base,flush=True);input('Press Enter after browser verification: ')
  print('PASS: first-message automatic title, whitespace normalization, 128 Unicode characters, invalid/empty/control/missing guards, manual title retention, restart persistence')
 finally:
  if process and process.poll() is None:process.terminate();process.wait(10)
