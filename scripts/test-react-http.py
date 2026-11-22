#!/usr/bin/env python3
"""Isolated read-only agent loop fixture. All model requests stay on localhost."""
import asyncio,http.server,json,os,socket,sqlite3,subprocess,sys,tempfile,threading,time,urllib.error,urllib.request
from pathlib import Path
import websockets
release=threading.Event();requests=[];preview='--preview' in sys.argv
class API(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body)
  assert {t['function']['name'] for t in body['tools']}=={'list_directory','read_file','search_files','write_file','apply_patch','run_command','load_skill','update_plan'}
  assert self.headers['Authorization']=='Bearer fixture-only-key'
  user=next(m['content'] for m in reversed(body['messages']) if m['role']=='user')
  results=[m for m in body['messages'] if m['role']=='tool']
  for result in results:
   summary=json.loads(result['content'])['keeperResult'];assert summary['version']==1 and summary['automaticRetry'] is False
   assert summary['outcome'] in ['success','failed','truncated','timeout','stopped','rejected']
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
  def event(delta,reason=None):
   packet=('data: '+json.dumps({'model':'fixture-agent','choices':[{'delta':delta,'finish_reason':reason}]},ensure_ascii=False)+'\r\n\r\n').encode()
   for i in range(0,len(packet),5):self.wfile.write(packet[i:i+5]);self.wfile.flush()
  def tools(calls,broken=False):
   event({'content':'我先检查项目文件。'})
   for index,(name,args) in enumerate(calls):
    arg=json.dumps(args,ensure_ascii=False);split=max(1,len(arg)//2)
    event({'tool_calls':[{'index':index,'id':'call-'+str(len(results))+'-'+str(index),'type':'function','function':{'name':name,'arguments':arg[:split]}}]})
    if broken:release.wait(10);return
    event({'tool_calls':[{'index':index,'function':{'arguments':arg[split:]}}]})
   event({},'tool_calls');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush()
  try:
   if 'correct args fixture' in user:
    if not results:tools([('write_file',{'path':'must-not-exist.txt'}),('read_file',{'path':'README.md','lineCount':'10'})]);return
    if len(results)==2:
     assert all(json.loads(r['content'])['keeperResult']['outcome']=='failed' for r in results)
     assert 'content' in json.loads(results[0]['content'])['error']
     assert 'lineCount' in json.loads(results[1]['content'])['error']
     tools([('read_file',{'path':'README.md','lineCount':10})]);return
    event({'content':'Corrected arguments succeeded'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'invalid plan fixture' in user:
    if not results:tools([('update_plan',{'plan':[{'step':'a','status':'in_progress'},{'step':'b','status':'in_progress'}]})]);return
    assert json.loads(results[-1]['content'])['keeperResult']['outcome']=='failed'
    event({'content':'Invalid plan rejected'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'plan fixture' in user:
    if len(results)==0:tools([('update_plan',{'plan':[{'step':'Read README','status':'in_progress'},{'step':'Summarize','status':'pending'}]})]);return
    if len(results)==1:tools([('read_file',{'path':'README.md'})]);return
    if len(results)==2:tools([('update_plan',{'plan':[{'step':'Read README','status':'completed'},{'step':'Summarize','status':'completed'}],'explanation':'Read results verified'})]);return
    event({'content':'Plan complete'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'broken' in user:tools([('read_file',{'path':'README.md'})],True);return
   if 'escape' in user and not results:
    tools([('read_file',{'path':'../secret.txt'}),('not_registered',{'command':'touch unsafe'}),('search_files',{'query':''})]);return
   if 'budget' in user:tools([('list_directory',{'path':'./'*len(results)+'.'})]);return
   if 'loop' in user:tools([('list_directory',{})]);return
   if not results:tools([('list_directory',{'path':'.'})]);return
   if 'escape' not in user and len(results)==1:
    assert any(e['name']=='README.md' for e in json.loads(results[0]['content'])['entries'])
    tools([('read_file',{'path':'README.md','startLine':1,'lineCount':10}),('search_files',{'query':'needle'})]);return
   if 'escape' in user:
    assert all('error' in json.loads(m['content']) for m in results)
   else:
    assert json.loads(results[1]['content'])['lines'][0]['text']=='# Fixture project'
    assert len(json.loads(results[2]['content'])['matches'])>=2
   event({'content':'已读取项目资料，正在整理总结。'})
   if 'escape' not in user:release.wait(300 if preview else 30)
   event({'content':'\n\n项目包含 README.md 和 src/main.rs；检索到了 needle。' if 'escape' not in user else '已拒绝越界和未开放操作。'},'stop')
   self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush()
  except (BrokenPipeError,ConnectionResetError):pass
api=http.server.ThreadingHTTPServer(('127.0.0.1',0),API);threading.Thread(target=api.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-react-http-') as folder:
 with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
 base=f'http://127.0.0.1:{port}';process=None;log=open(Path(folder)/'server.log','wb')
 def call(name,args=None,expected=200):
  request=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base})
  try:
   with urllib.request.urlopen(request,timeout=10) as r:status=r.status;data=json.load(r)
  except urllib.error.HTTPError as e:status=e.code;data=json.load(e)
  assert status==expected,(name,status,data)
  return data.get('result') if status==200 else data
 def start():
  global process
  process=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'TMPDIR':folder},stdout=log,stderr=log)
  for _ in range(100):
   try:call('agent_bootstrap');return
   except Exception:time.sleep(.05)
  raise RuntimeError('Keeper startup failed')
 def wait_for(session,predicate):
  for _ in range(200):
   detail=call('agent_session',{'sessionId':session})
   if predicate(detail):return detail
   time.sleep(.025)
  raise AssertionError('Session did not reach expected state')
 def session(prompt):
  s=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':s,'content':prompt});return s
 events=[];untrusted_events=[];ready=threading.Event();untrusted_ready=threading.Event()
 async def collect(trusted=True):
  async with websockets.connect(base.replace('http:','ws:')+'/api/events',origin=base if trusted else 'http://untrusted.invalid') as ws:
   (ready if trusted else untrusted_ready).set()
   try:
    async for raw in ws:(events if trusted else untrusted_events).append(json.loads(raw))
   except websockets.ConnectionClosed:pass
 try:
  start();process.terminate();process.wait(timeout=10);root=Path(folder)/'project';root.mkdir();
  with sqlite3.connect(Path(folder)/'data.sql') as db:db.execute("UPDATE agent_projects SET directory=? WHERE id='general'",(str(root),))
  start();(root/'src').mkdir();(root/'README.md').write_text('# Fixture project\nproject needle\n');(root/'src/main.rs').write_text('fn main() { /* needle */ }\n');(root.parent/'secret.txt').write_text('never-expose-outside-project')
  call('agent_model_save',{'enabled':True,'baseUrl':f'http://127.0.0.1:{api.server_port}','model':'fixture','apiKey':'fixture-only-key'})
  threading.Thread(target=lambda:asyncio.run(collect()),daemon=True).start();assert ready.wait(5)
  threading.Thread(target=lambda:asyncio.run(collect(False)),daemon=True).start();assert untrusted_ready.wait(5)
  normal=session('阅读项目并总结 normal')
  detail=wait_for(normal,lambda d:len(d['runs'])==3 and all(r['status']=='succeeded' for r in d['runs']) and '正在整理' in d['messages'][-1]['content'])
  assert detail['modelPending'] and detail['tasks'][-1]['status']=='model'
  assert len({r['toolCallId'] for r in detail['runs']})==3
  assert all(r['responseId']==detail['streamingMessageId'] for r in detail['runs'])
  fresh=call('agent_session',{'sessionId':normal});assert fresh['runs']==detail['runs']
  assert any(e['event']=='agent-tool-update' and e['payload']['status']=='running' for e in events)
  assert any(e['event']=='agent-tool-update' and e['payload']['status']=='succeeded' for e in events)
  assert not any(e['event'].startswith('agent-') for e in untrusted_events),'cross-origin WebSocket must not receive file contents'
  if preview:
   print(json.dumps({'url':base,'session':normal,'directory':str(root)}),flush=True);input();release.set();wait_for(normal,lambda d:not d['modelPending']);print('Completed.',flush=True);input()
  else:
   # Disconnect only the browser event channel; generation must continue and a
   # fresh snapshot after reconnect must not replay model/tool operations.
   async def reconnect():
    url=base.replace('http:','ws:')+'/api/events'
    async with websockets.connect(url,origin=base):pass
    before=len(requests)
    async with websockets.connect(url,origin=base):
     restored=call('agent_session',{'sessionId':normal})
     assert restored['modelPending'] and restored['runs']==detail['runs']
     assert restored['tasks']==fresh['tasks']
     await asyncio.sleep(.1)
     assert len(requests)==before,'reconnect must not replay model requests'
   asyncio.run(reconnect())
   call('agent_stop_generation',{'sessionId':normal,'messageId':detail['streamingMessageId']})
   stopped=wait_for(normal,lambda d:not d['modelPending']);assert stopped['tasks'][-1]['status']=='stopped';assert '正在整理' in stopped['messages'][-1]['content'];release.set()
   corrected=session('correct args fixture');corrected_detail=wait_for(corrected,lambda d:not d['modelPending'])
   assert [r['status'] for r in corrected_detail['runs']]==['failed','failed','succeeded']
   assert not (root/'must-not-exist.txt').exists()
   assert all(not r.get('review') for r in corrected_detail['runs'])
   invalid_plan=session('invalid plan fixture');invalid_detail=wait_for(invalid_plan,lambda d:not d['modelPending']);assert len(invalid_detail['runs'])==1 and invalid_detail['runs'][0]['status']=='failed'
   planned=session('plan fixture');plan_detail=wait_for(planned,lambda d:not d['modelPending'])
   assert [r['tool'] for r in plan_detail['runs']]==['update_plan','read_file','update_plan']
   assert all(r['status']=='succeeded' for r in plan_detail['runs'])
   assert json.loads(plan_detail['runs'][-1]['output'])['plan'][1]['status']=='completed'
   assert call('agent_session',{'sessionId':planned})['runs']==plan_detail['runs']
   complete=session('阅读项目并总结 complete');complete_detail=wait_for(complete,lambda d:not d['modelPending']);assert complete_detail['tasks'][-1]['status']=='complete';assert len(complete_detail['runs'])==3
   # A regenerated answer owns a separate tool history and selects the new version.
   source=complete_detail['messages'][-1]['id'];call('agent_regenerate',{'sessionId':complete,'messageId':source});regen=wait_for(complete,lambda d:not d['modelPending']);assert len(regen['runs'])==6;assert len({r['responseId'] for r in regen['runs']})==2
   escaped=session('escape');escaped=wait_for(escaped,lambda d:not d['modelPending']);assert len(escaped['runs'])==3 and all(r['status']=='failed' for r in escaped['runs']);assert len([r for r in requests if any(m.get('role')=='user' and m.get('content')=='escape' for m in r['messages'])])==2;assert not (root/'unsafe').exists();assert 'never-expose' not in json.dumps(escaped)
   looping=session('loop');looped=wait_for(looping,lambda d:not d['modelPending']);assert looped['tasks'][-1]['status']=='failed' and len(looped['runs'])==3
   assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='no_progress' for m in looped['messages'] if m['role']=='assistant')
   assert '无进展' in json.dumps(looped,ensure_ascii=False)
   assert '无进展' in json.dumps(call('agent_session',{'sessionId':looping}),ensure_ascii=False)
   bounded=session('budget');bounded=wait_for(bounded,lambda d:not d['modelPending']);assert len(bounded['runs'])==8 and '8 轮' in json.dumps(bounded,ensure_ascii=False)
   assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='round_limit' for m in bounded['messages'])
   assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='complete' for m in complete_detail['messages'])
   assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='user_stopped' for m in stopped['messages'])
   release.clear();broken=session('broken');partial=wait_for(broken,lambda d:bool(d['messages'][-1]['content']));call('agent_stop_generation',{'sessionId':broken,'messageId':partial['streamingMessageId']});assert wait_for(broken,lambda d:not d['modelPending'])['runs']==[]
   interrupted=session('阅读项目并总结 restart');persisted=wait_for(interrupted,lambda d:len(d['runs'])==3 and '正在整理' in d['messages'][-1]['content']);count=len(requests)
   process.terminate();process.wait(timeout=10);start();restored=call('agent_session',{'sessionId':interrupted});assert not restored['modelPending'] and restored['tasks'][-1]['status']=='interrupted';assert len(restored['runs'])==3
   assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='interrupted' for m in restored['messages'])
   time.sleep(.1);assert len(requests)==count,'restart must not replay tools or model calls'
   with sqlite3.connect(Path(folder)/'data.sql') as db:assert db.execute('SELECT count(*) FROM agent_events WHERE response_id=?',(persisted['streamingMessageId'],)).fetchone()[0]>5
   print('ReAct checks passed: fragmented native tool calls, multi-round results, realtime events, reload snapshots, stop, regeneration isolation, denied paths/tools, round limit and restart without replay.')
 finally:
  release.set()
  if process and process.poll() is None:process.terminate();process.wait(timeout=10)
  log.close();api.shutdown();api.server_close()
