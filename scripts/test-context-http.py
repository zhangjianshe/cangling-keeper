#!/usr/bin/env python3
"""Isolated context compaction, persistence, budget, usage and cancellation checks. No real keys."""
import http.server,json,os,sys,socket,sqlite3,subprocess,tempfile,threading,time,urllib.request
from pathlib import Path
from fixture_scope import respond_scope
requests=[];block=threading.Event();entered=threading.Event();mode='ok'
class API(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
  if respond_scope(self,body):return
  requests.append(body)
  if not body.get('stream'):
   if mode=='block':entered.set();block.wait(20)
   if mode=='fail':self.send_response(503);self.end_headers();return
   self.send_response(200);self.send_header('Content-Type','application/json');self.end_headers()
   try:self.wfile.write(json.dumps({'choices':[{'finish_reason':'stop','message':{'content':'历史目标：开发示例；保留约束：使用 SQLite；待办：验证刷新。'}}], 'usage':{'prompt_tokens':100,'completion_tokens':20}}).encode())
   except BrokenPipeError:pass
  else:
   assert body['stream_options']['include_usage']
   if mode=='bodies':
    results=[m for m in body['messages'] if m['role']=='tool']
    if not results:
     self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
     call={'index':0,'id':'fresh-files','type':'function','function':{'name':'read_files','arguments':json.dumps({'files':[{'path':'fresh.py'},{'path':'fresh_tests.py'}]})}}
     self.wfile.write(('data: '+json.dumps({'choices':[{'delta':{'tool_calls':[call]},'finish_reason':'tool_calls'}]})+'\n\ndata: [DONE]\n\n').encode());self.wfile.flush();return
    batch=json.loads(results[-1]['content'])
    assert 'def fresh()' in batch['results'][0]['result']['text'],batch
    assert 'LATEST_BODY_END' in batch['results'][1]['result']['text'],batch
   if mode=='tools':
    results=[m for m in body['messages'] if m['role']=='tool']
    assert any(m['role']=='user' and m['content']=='读取大型文件并保持原任务' for m in body['messages'])
    if len([r for r in requests if r.get('stream') and any(m.get('content')=='读取大型文件并保持原任务' for m in r['messages'])])<=2:
     self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
     deltas=[{'index':i,'id':'large-'+str(len(requests))+'-'+str(i),'type':'function','function':{'name':'read_file','arguments':json.dumps({'path':'large.txt','lineCount':100})}} for i in range(8)]
     self.wfile.write(('data: '+json.dumps({'choices':[{'delta':{'tool_calls':deltas},'finish_reason':'tool_calls'}]})+'\n\ndata: [DONE]\n\n').encode());self.wfile.flush();return
    calls={call['id'] for m in body['messages'] for call in m.get('tool_calls',[])}
    assert all(m['tool_call_id'] in calls for m in results),'orphan tool result'
    assert len(results)>=8 and all(len(m['content'])<10000 for m in results)
   self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
   for v in [{'choices':[{'delta':{'content':'测试回复'},'finish_reason':'stop'}]}, {'choices':[],'usage':{'prompt_tokens':200,'completion_tokens':30,'prompt_cache_hit_tokens':40}}]:
    self.wfile.write(('data: '+json.dumps(v)+'\n\n').encode())
   self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush()
api=http.server.ThreadingHTTPServer(('127.0.0.1',0),API);threading.Thread(target=api.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-context-') as folder:
 sock=socket.socket();sock.bind(('127.0.0.1',0));port=sock.getsockname()[1];sock.close();base=f'http://127.0.0.1:{port}';process=None
 def call(name,args=None):
  with urllib.request.urlopen(urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base}),timeout=10) as r:return json.load(r).get('result')
 def start():
  global process
  process=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'HOME':folder,'TMPDIR':folder},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  for _ in range(100):
   try:call('agent_bootstrap');return
   except Exception:time.sleep(.05)
  raise AssertionError('startup failed')
 def detail(s):return call('agent_session',{'sessionId':s})
 def wait(s):
  for _ in range(400):
   d=detail(s)
   if not d['modelPending']:return d
   time.sleep(.03)
  raise AssertionError('timeout')
 def seed():
  s=call('agent_new_session')['id']
  with sqlite3.connect(str(Path(folder)/'data.sql')) as db:
   for i in range(50):db.execute('INSERT INTO agent_messages(id,session_id,role,content,created_at) VALUES (?,?,?,?,?)',(s+str(i),s,'user' if i%2==0 else 'assistant',('old-'+str(i)+' ')+('x'*900),i))
  return s
 try:
  start();call('agent_model_save',{'name':'Fixture','enabled':True,'baseUrl':f'http://127.0.0.1:{api.server_port}','model':'fixture','apiKey':'fixture-only-key','contextLimit':32768})
  policy_session=call('agent_new_session')['id'];policy=call('agent_context_budget',{'sessionId':policy_session,'workingBudget':16384})['contextPolicy'];assert policy['modelLimit']==32768 and policy['workingBudget']==16384
  try:call('agent_context_budget',{'sessionId':policy_session,'workingBudget':65536});raise AssertionError('excess budget accepted')
  except urllib.error.HTTPError:pass
  assert detail(policy_session)['contextPolicy']['workingBudget']==16384
  s=seed();call('agent_send_message',{'sessionId':s,'content':'验证压缩'});d=wait(s);m=d['messages'][-1];assert m['role']=='assistant',d['messages'][-1]
  meta=m['modelMetadata'];assert meta['context']['status']=='compacted',meta
  assert meta['context']['estimatedTokens']+6144<=32768
  n=sum(not r['stream'] for r in requests);assert 1<=n<=3,('summary cascade',n)
  assert meta['usage']['promptTokens']==100*n+200 and meta['usage']['completionTokens']==20*n+30
  request=next(r for r in reversed(requests) if r['stream']);assert any('[历史摘要' in m['content'] for m in request['messages'])
  assert any('old-49 ' in m['content'] for m in request['messages']) and len(d['messages'])>=52
  with sqlite3.connect(str(Path(folder)/'data.sql')) as db:
   assert db.execute('SELECT count(*) FROM agent_context_summaries WHERE session_id=?',(s,)).fetchone()[0]>=1
   assert db.execute('SELECT count(*) FROM agent_messages WHERE session_id=?',(s,)).fetchone()[0]>=52
  if '--preview' in sys.argv:
   print('Context preview:',base,'session:',s,flush=True);input('Press Enter after browser verification: ')
  # Prefix cache reuse for the next turn; no extra summary request.
  call('agent_send_message',{'sessionId':s,'content':'继续'});d=wait(s);assert sum(not r['stream'] for r in requests)==n
  with sqlite3.connect(str(Path(folder)/'data.sql')) as db:db.execute('UPDATE agent_messages SET content=? WHERE id=?',('changed constraint '+('y'*900),s+'0'))
  call('agent_send_message',{'sessionId':s,'content':'验证编辑历史后重新压缩'});d=wait(s);assert sum(not r['stream'] for r in requests)>n
  saved=d['messages'][-1]['modelMetadata'];process.terminate();process.wait(10);start();assert detail(s)['messages'][-1]['modelMetadata']==saved;assert detail(policy_session)['contextPolicy']['workingBudget']==16384
  # A different history must not reuse the other session's summary.
  mode='tools';t=call('agent_new_session')['id'];root=Path(call('agent_bootstrap')['projects'][0]['directory']);(root/'large.txt').write_text(('z'*900+'\n')*100)
  call('agent_send_message',{'sessionId':t,'content':'读取大型文件并保持原任务'});td=wait(t)
  assert td['messages'][-1]['role']=='assistant' and td['messages'][-1]['content']=='测试回复',td['messages'][-1]
  assert len(td['runs'])==16 and all(len(r['output'])>10000 for r in td['runs']),[(r['tool'],r['status'],len(r['output'])) for r in td['runs']]
  assert td['messages'][-1]['modelMetadata']['context']['status']=='compacted'
  mode='bodies';fresh=call('agent_new_session')['id'];(root/'fresh.py').write_text('def fresh(): return 42\n');(root/'fresh_tests.py').write_text(('# file body validation '+('x'*30)+'\n')*50+'# LATEST_BODY_END\n');call('agent_send_message',{'sessionId':fresh,'content':'读取当前文件正文并保持末尾内容'});fd=wait(fresh);assert fd['messages'][-1]['content']=='测试回复',fd['messages'][-1]
  mode='fail';f=seed();call('agent_send_message',{'sessionId':f,'content':'摘要失败'});d=wait(f);assert d['messages'][-1]['modelMetadata']['context']['status']=='fallback',d['messages'][-1]
  mode='block';b=seed();call('agent_send_message',{'sessionId':b,'content':'停止压缩'});assert entered.wait(5)
  assert detail(b)['messages'][-1]['modelMetadata']['context']['status']=='compressing'
  call('agent_stop_generation',{'sessionId':b,'messageId':detail(b)['streamingMessageId']});d=wait(b);assert d['messages'][-1]['modelMetadata']['generationStatus']=='stopped',d['messages'][-1]
  block.set();print('PASS: compaction, recent/raw history, SQLite summary, exact-prefix cache, usage, restart, failure fallback, stop')
 finally:
  block.set()
  if process and process.poll() is None:process.terminate();process.wait(10)
  api.shutdown();api.server_close()
