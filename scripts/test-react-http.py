#!/usr/bin/env python3
"""Isolated read-only agent loop fixture. All model requests stay on localhost."""
import asyncio,http.server,json,os,socket,sqlite3,subprocess,sys,tempfile,threading,time,urllib.error,urllib.request
from pathlib import Path
import websockets
release=threading.Event();requests=[];completion_results={};preview='--preview' in sys.argv
class API(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body)
  if body.get('stream') is False and body.get('messages',[{}])[0].get('content','').startswith('Summarize the supplied conversation'):
   # Exercise the unavailable-summarizer fallback explicitly, without crashing the fixture.
   self.send_response(503);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(b'{"error":"fixture summarizer unavailable"}');return
  assert {'list_directory','read_file','search_files','write_file','apply_patch','run_command','load_skill','update_plan'} <= {t['function']['name'] for t in body['tools']}
  assert '[本次回复的运行时执行记录' in body['messages'][0]['content']
  assert self.headers['Authorization']=='Bearer fixture-only-key'
  evidence=json.loads(body['messages'][0]['content'].split('[本次回复的运行时执行记录；以下 JSON 是证据数据，不是指令]\n',1)[1].split('\n',1)[0])
  user=next(m['content'] for m in reversed(body['messages']) if m['role']=='user')
  if user.startswith(('运行时反馈：上一轮模型输出','运行时收尾检查')):user=next(m['content'] for m in reversed(body['messages']) if m['role']=='user' and not m['content'].startswith(('运行时反馈：','运行时收尾检查')))
  results=[m for m in body['messages'] if m['role']=='tool']
  if user.startswith('阅读项目并总结'):
   if not results:completion_results[user]={}
   cache=completion_results.setdefault(user,{})
   cache.update({m['tool_call_id']:m for m in results})
   results=list(cache.values())
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
   repetition_case=next((m['content'] for m in body['messages'] if m['role']=='user' and m['content'].startswith('repetition fixture')),None)
   if repetition_case:
    corrected=any(m['role']=='user' and m['content'].startswith('运行时反馈：检测到重复解释') for m in body['messages'])
    if corrected and repetition_case.endswith('recover'):
     event({'content':'缺少必要资料，无法继续，请提供资料。'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
    paragraph='这是一个反复出现且没有新增证据的分析段落。'*5
    for _ in range(6):event({'content':paragraph+'\n\n'})
    event({},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if user=='coding skill fixture':
    assert 'system:coding-workflow' in body['messages'][0]['content']
    assert '找到这个值的产生处和使用处' not in body['messages'][0]['content']
    if not results:tools([('load_skill',{'name':'system:coding-workflow'})]);return
    assert '找到这个值的产生处和使用处' in results[-1]['content']
    event({'content':'已按需读取技能。'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if user=='plan retention fixture':
    if len(results)==0:tools([('update_plan',{'plan':[{'step':'实现','status':'in_progress'}],'acceptance':[{'requirement':'功能','check':'测试','status':'completed'}]})]);return
    if len(results)==1:tools([('update_plan',{'plan':[{'step':'实现','status':'completed'}]})]);return
    assert evidence['latestPlan']['acceptance'][0]['status']=='unverified'
    event({'content':'计划已记录，验收尚未验证。'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'inheritance fixture' in user:
    if user=='seed inheritance fixture' and not results:
     tools([('update_plan',{'plan':[{'step':'等待必要资料','status':'pending'}]})]);return
    if user.startswith('继续'):
     assert evidence['planInherited'] is True and len(evidence['remainingSteps'])==1
    if user.startswith('new'):
     assert evidence['latestPlan'] is None
    event({'content':'缺少必要资料，无法继续，请提供资料。'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if user=='structured test fixture':
    if len(results)<2:tools([('run_command',{'command':"printf 'Ran 3 tests in 0.1s\\nFAILED (failures=1)\\n'"})]);return
    event({'content':'测试报告仍有失败，未完成验收。'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if user=='oversized event fixture':
    event({'content':'Before oversized event'})
    event({'tool_calls':[{'index':0,'id':'not-executed','type':'function','function':{'name':'write_file','arguments':json.dumps({'path':'oversized-event.txt','content':'must not write'})}}]})
    for _ in range(40):self.wfile.write(b'data: '+b' '*32768+b'\n');self.wfile.flush()
    self.wfile.write(b'\n');self.wfile.flush();return
   if user in ['missing finish fixture','unknown finish fixture']:
    event({'content':'Incomplete answer preserved'},None if user=='missing finish fixture' else 'unexpected')
    self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if user=='multiline event fixture':
    packet={'model':'fixture-agent','choices':[{'delta':{'content':'多行事件正常'},'finish_reason':'stop'}]}
    for line in json.dumps(packet,ensure_ascii=False,indent=2).splitlines():self.wfile.write(('data: '+line+'\r\n').encode());self.wfile.flush()
    self.wfile.write(b'\r\ndata: [DONE]\r\n\r\n');self.wfile.flush();return
   if user in ['malformed batch fixture','array batch fixture','duplicate batch fixture']:
    event({'content':'Batch preview preserved'})
    event({'tool_calls':[{'index':0,'id':'first','type':'function','function':{'name':'write_file','arguments':json.dumps({'path':'bad-batch.txt','content':'must not write'})}}]})
    event({'tool_calls':[{'index':1,'id':'first' if user=='duplicate batch fixture' else 'second','type':'function','function':{'name':'read_file','arguments':'{' if user=='malformed batch fixture' else '[]' if user=='array batch fixture' else '{}'}}]},'tool_calls')
    self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if user=='recover cutoff fixture':
    if any(m.get('content','').startswith('运行时反馈：上一轮模型输出') for m in body['messages'] if isinstance(m.get('content'),str)):
     if not results:tools([('list_directory',{})]);return
     event({'content':'恢复成功，未执行截断写入。'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
    event({'tool_calls':[{'index':0,'id':'partial','type':'function','function':{'name':'write_file','arguments':'{"path":"cutoff.txt","content":"'}}]},'length');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if user in ['output cutoff fixture','filtered output fixture','tool cutoff fixture']:
    event({'content':'Partial answer preserved'})
    if user=='tool cutoff fixture':event({'tool_calls':[{'index':0,'id':'cutoff','type':'function','function':{'name':'write_file','arguments':json.dumps({'path':'cutoff.txt','content':'must not write'})}}]})
    event({},'content_filter' if user=='filtered output fixture' else 'length');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'ordered flow fixture' in user:
    if len(results)==0:event({'content':'先检查🙂。'});tools([('list_directory',{})]);return
    if len(results)==1:event({'content':'再读取文件。'});tools([('read_file',{'path':'README.md'})]);return
    event({'content':'总结：读取结果已核对。'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'batch read fixture' in user:
    if not results:tools([('read_files',{'files':[{'path':'README.md','lineCount':1},{'path':'missing.txt'},{'path':'context.txt'}]})]);return
    value=json.loads(results[-1]['content']);assert value['completedFiles']==3 and value['failedFiles']==1
    assert value['results'][0]['status']=='succeeded' and value['results'][1]['status']=='failed'
    first=value['results'][0]['result'];assert first['hasMore'] is True and first['truncated'] is False and first['requestedRangeComplete'] is True
    assert '不必' in first['readWindow'] and first['nextRead']['startLine']==2
    assert value['results'][2]['result']['text'].splitlines()[0]=='中文前文'
    event({'content':'Batch results verified; missing file reported separately'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'context search fixture' in user:
    if not results:tools([('search_files',{'query':'needle','path':'context.txt','mode':'content','contextLines':1})]);return
    if len(results)==1:
     hit=json.loads(results[-1]['content'])['matches'][0];assert [r['line'] for r in hit['context']]==[1,2,3];assert hit['context'][0]['text']=='中文前文';assert hit['context'][1]['isMatch'] is True
     tools([('read_file',hit['contextReadArgs'])]);return
    assert json.loads(results[-1]['content'])['text'].splitlines()==['中文前文','needle here','after']
    event({'content':'Search context verified'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'located search fixture' in user:
    if not results:tools([('search_files',{'query':'target-word','path':'late.txt','mode':'content'})]);return
    if len(results)==1:
     hit=json.loads(results[-1]['content'])['matches'][0];assert 'target-word' in hit['text'];assert hit['column']>1000
     tools([('read_file',hit['readArgs'])]);return
    assert 'target-word' in json.loads(results[-1]['content'])['text']
    event({'content':'Located and read match'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'limited search fixture' in user:
    if not results:tools([('search_files',{'query':'needle','path':'limited.txt','mode':'content','maxResults':1})]);return
    result=json.loads(results[-1]['content']);assert len(result['matches'])==1 and result['truncated'] is True;assert result['truncationReasons']==['result_limit']
    event({'content':'Search limited explicitly'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'filtered search fixture' in user:
    if not results:tools([('search_files',{'query':'needle','mode':'content','extensions':['rs']})]);return
    matches=json.loads(results[-1]['content'])['matches'];assert len(matches)==1 and matches[0]['path'].endswith('main.rs')
    event({'content':'Filtered search complete'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'paged directory fixture' in user:
    if not results:tools([('list_directory',{'path':'many'})]);return
    last=json.loads(results[-1]['content'])
    if last.get('nextList'):tools([('list_directory',last['nextList'])]);return
    assert last.get('nextList') is None
    event({'content':'Listed entire directory'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'continued read fixture' in user:
    if not results:tools([('read_file',{'path':'long.txt'})]);return
    last=json.loads(results[-1]['content'])
    if last.get('nextRead'):tools([('read_file',last['nextRead'])]);return
    # Context compaction may remove earlier calls from the current request.
    # Verify all uniquely observed fragments, not an unbounded wire history.
    observed={r['tool_call_id']:json.loads(r['content']) for request in requests for r in request.get('messages',[]) if r.get('role')=='tool' and json.loads(r['content']).get('path')=='long.txt'}
    fragments=sorted(observed.values(),key=lambda r:r['startColumn'])
    assert ''.join(r['text'] for r in fragments)=='中文🙂'*250
    event({'content':'Read all fragments'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'correct args fixture' in user:
    if not results:tools([('write_file',{'path':'must-not-exist.txt'}),('read_file',{'path':'README.md','lineCount':'10'})]);return
    if len(results)==2:
     assert all(json.loads(r['content'])['keeperResult']['outcome']=='failed' for r in results)
     assert 'content' in json.loads(results[0]['content'])['error']
     assert 'lineCount' in json.loads(results[1]['content'])['error']
     tools([('read_file',{'path':'README.md','lineCount':10})]);return
    event({'content':'Corrected arguments succeeded'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'blocked steps fixture' in user:
    if not results:tools([('update_plan',{'plan':[{'step':'Read','status':'blocked','reason':'等待文件'},{'step':'Publish','status':'skipped','reason':'本轮不发布'}]})]);return
    if len(results)==1:
     assert json.loads(results[-1]['content'])['saved'] is True
     assert evidence['latestPlan']['plan'][0]['reason']=='等待文件'
     tools([('update_plan',{'plan':[{'step':'Read','status':'blocked'}]})]);return
    if len(results)==2:
     assert json.loads(results[-1]['content'])['keeperResult']['outcome']=='failed'
     assert evidence['latestPlan']['plan'][0]['status']=='blocked'
     assert len(evidence['remainingSteps'])==2
     tools([('update_plan',{'plan':[{'step':'Read','status':'pending'},{'step':'Publish','status':'skipped','reason':'本轮不发布'}]})]);return
    event({'content':'Blocked plan corrected; no actions executed'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'invalid plan fixture' in user:
    if not results:tools([('update_plan',{'plan':[{'step':'a','status':'in_progress'},{'step':'b','status':'in_progress'}]})]);return
    assert json.loads(results[-1]['content'])['keeperResult']['outcome']=='failed'
    event({'content':'Invalid plan rejected'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'plan fixture' in user:
    if len(results)==0:tools([('update_plan',{'plan':[{'step':'Read README','status':'in_progress'},{'step':'Summarize','status':'pending'}]})]);return
    if len(results)==1:tools([('read_file',{'path':'README.md'})]);return
    if len(results)==2:tools([('update_plan',{'plan':[{'step':'Read README','status':'completed'},{'step':'Summarize','status':'completed'}],'explanation':'Read results verified'})]);return
    assert evidence['remainingSteps']==[] and 'modelReported' not in evidence['latestPlan']
    assert evidence['goalStatus']=='not_assessed'
    event({'content':'Plan complete'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'unified test fixture' in user:
    if not results:tools([('run_tests',{'directory':'checks','timeoutSeconds':10})]);return
    summary=json.loads(results[-1]['content'])['testSummary'];assert summary['verdict']=='passed' and summary['total']==1
    event({'content':'测试结果已记录。'},'stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush();return
   if 'broken' in user:tools([('read_file',{'path':'README.md'})],True);return
   if 'escape' in user and not results:
    tools([('read_file',{'path':'../secret.txt'}),('not_registered',{'command':'touch unsafe'}),('search_files',{'query':''})]);return
   if 'budget' in user:tools([('list_directory',{'path':'./'*evidence['callCount']+'.'})]);return
   if 'alternating cycle' in user:tools([('read_file',{'path':'README.md' if len(results)%2==0 else 'src/main.rs'})]);return
   if 'loop' in user:tools([('list_directory',{})]);return
   if not results:tools([('list_directory',{'path':'.'})]);return
   if 'escape' not in user and len(results)==1:
    assert any(e['name']=='README.md' for e in json.loads(results[0]['content'])['entries'])
    tools([('read_file',{'path':'README.md','startLine':1,'lineCount':10}),('search_files',{'query':'needle'})]);return
   if 'escape' in user:
    assert all('error' in json.loads(m['content']) for m in results)
   else:
    assert json.loads(results[1]['content'])['text'].splitlines()[0]=='# Fixture project'
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
  call('agent_model_save',{'enabled':True,'baseUrl':f'http://127.0.0.1:{api.server_port}','model':'fixture','apiKey':'fixture-only-key','contextLimit':131072})
  threading.Thread(target=lambda:asyncio.run(collect()),daemon=True).start();assert ready.wait(5)
  threading.Thread(target=lambda:asyncio.run(collect(False)),daemon=True).start();assert untrusted_ready.wait(5)
  for mode in ['recover','repeat']:
   repetition=session('repetition fixture '+mode);rd=wait_for(repetition,lambda d:not d['modelPending'])
   rm=next(m['modelMetadata'] for m in reversed(rd['messages']) if m['role']=='assistant')
   assert rm['repetitionInterrupted'] is True and rm['usageIncomplete'] is True
   assert not rd['runs'], 'interrupted prose must never execute tools'
   assert rd['tasks'][-1]['step']==2, rd['tasks']
   if mode=='repeat':assert rm['taskReasonCode']=='no_progress',rm
  sk=session('coding skill fixture');sd=wait_for(sk,lambda d:not d['modelPending']);assert len(sd['runs'])==1 and sd['runs'][0]['status']=='succeeded',sd
  inherited_session=session('seed inheritance fixture');seed=wait_for(inherited_session,lambda d:not d['modelPending'])
  for prompt in ['继续 inheritance fixture 1','继续 inheritance fixture 2']:
   call('agent_send_message',{'sessionId':inherited_session,'content':prompt});continued=wait_for(inherited_session,lambda d:not d['modelPending'])
   meta=continued['messages'][-1]['modelMetadata'];assert meta['executionEvidence']['planInherited'] is True, {'prompt':prompt,'metadata':meta,'seed':seed['messages'][-1]}
   assert meta['taskOutcome']['remainingSteps']==1 and meta['taskOutcome']['status']=='incomplete'
   assert meta['taskGoal']=='seed inheritance fixture'
  source=continued['messages'][-1]['id']
  call('agent_send_message',{'sessionId':inherited_session,'content':'new inheritance fixture'});new=wait_for(inherited_session,lambda d:not d['modelPending']);assert new['messages'][-1]['modelMetadata']['executionEvidence']['latestPlan'] is None;assert new['messages'][-1]['modelMetadata']['taskGoal']=='new inheritance fixture'
  call('agent_regenerate',{'sessionId':inherited_session,'messageId':source});regenerated=wait_for(inherited_session,lambda d:not d['modelPending']);assert regenerated['messages'][-1]['modelMetadata']['taskOutcome']['remainingSteps']==1;assert regenerated['messages'][-1]['modelMetadata']['taskGoal']=='seed inheritance fixture'
  normal=session('阅读项目并总结 normal')
  detail=wait_for(normal,lambda d:len(d['runs'])==3 and all(r['status']=='succeeded' for r in d['runs']) and '正在整理' in d['messages'][-1]['content'])
  call('agent_execution_budget',{'sessionId':normal,'maxRounds':24},expected=400)
  call('agent_execution_budget',{'sessionId':normal,'maxToolCalls':64,'activeSeconds':600},expected=400)
  assert detail['modelPending'] and detail['tasks'][-1]['status']=='model'
  assert detail['messages'][-1]['modelMetadata']['taskBudget']=={'maxRounds':128,'maxToolCalls':128,'activeSeconds':1800}
  assert len({r['toolCallId'] for r in detail['runs']})==3
  assert all(r['responseId']==detail['streamingMessageId'] for r in detail['runs'])
  fresh=call('agent_session',{'sessionId':normal});assert fresh['runs']==detail['runs'];assert fresh['messages'][-1]['modelMetadata']['presentation']==detail['messages'][-1]['modelMetadata']['presentation'];assert fresh['messages'][-1]['modelMetadata']['taskBudget']==detail['messages'][-1]['modelMetadata']['taskBudget']
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
   stopped=wait_for(normal,lambda d:not d['modelPending']);assert stopped['tasks'][-1]['status']=='stopped';assert '正在整理' in stopped['messages'][-1]['content'];assert stopped['messages'][-1]['modelMetadata']['presentation']['segments'][-1]['kind']=='commentary';release.set()
   ordered=session('ordered flow fixture');ordered_detail=wait_for(ordered,lambda d:not d['modelPending'])
   answer=next(m for m in ordered_detail['messages'] if m['role']=='assistant');layout=answer['modelMetadata']['presentation'];assert layout['version']==1
   assert [x['kind'] for x in layout['segments']]==['commentary','commentary','final']
   assert [r['step'] for r in ordered_detail['runs']]==[1,2]
   raw=answer['content'].encode('utf-16-le');parts=[raw[x['start']*2:(layout['segments'][i+1]['start']*2 if i+1<len(layout['segments']) else len(raw))].decode('utf-16-le').strip() for i,x in enumerate(layout['segments'])]
   assert parts==['先检查🙂。我先检查项目文件。','再读取文件。我先检查项目文件。','总结：读取结果已核对。'],parts
   restored=call('agent_session',{'sessionId':ordered});assert next(m for m in restored['messages'] if m['id']==answer['id'])['modelMetadata']['presentation']==layout
   (root/'context.txt').write_text('中文前文\nneedle here\nafter')
   batch=session('batch read fixture');batch_detail=wait_for(batch,lambda d:not d['modelPending']);assert batch_detail['tasks'][-1]['status']=='complete' and len(batch_detail['runs'])==1
   assert call('agent_session',{'sessionId':batch})['runs']==batch_detail['runs']
   context=session('context search fixture');context_detail=wait_for(context,lambda d:not d['modelPending']);assert context_detail['tasks'][-1]['status']=='complete' and len(context_detail['runs'])==2
   assert call('agent_session',{'sessionId':context})['runs']==context_detail['runs']
   (root/'late.txt').write_text('中'*1200+'target-word'+'后'*100)
   located=session('located search fixture');located_detail=wait_for(located,lambda d:not d['modelPending']);assert located_detail['tasks'][-1]['status']=='complete';assert len(located_detail['runs'])==2
   (root/'limited.txt').write_text('needle\nneedle\n')
   limited=session('limited search fixture');limited_detail=wait_for(limited,lambda d:not d['modelPending']);assert limited_detail['tasks'][-1]['status']=='complete';assert len(limited_detail['runs'])==1
   assert call('agent_session',{'sessionId':limited})['runs']==limited_detail['runs']
   filtered=session('filtered search fixture');filtered_detail=wait_for(filtered,lambda d:not d['modelPending']);assert filtered_detail['tasks'][-1]['status']=='complete';assert len(filtered_detail['runs'])==1
   (root/'many').mkdir()
   for index in range(205):(root/'many'/f'{index:03}.txt').write_text('')
   paged=session('paged directory fixture');paged_detail=wait_for(paged,lambda d:not d['modelPending'])
   assert paged_detail['tasks'][-1]['status']=='complete' and len(paged_detail['runs'])==2
   listed=[e['name'] for run in paged_detail['runs'] for e in json.loads(run['output'])['entries']]
   assert len(listed)==205 and len(set(listed))==205
   (root/'long.txt').write_text('中文🙂'*250)
   continued=session('continued read fixture');continued_detail=wait_for(continued,lambda d:not d['modelPending'])
   assert continued_detail['tasks'][-1]['status']=='complete' and len(continued_detail['runs'])==3
   assert all(r['status']=='succeeded' for r in continued_detail['runs'])
   corrected=session('correct args fixture');corrected_detail=wait_for(corrected,lambda d:not d['modelPending'])
   assert [r['status'] for r in corrected_detail['runs']]==['failed','failed','succeeded']
   assert not (root/'must-not-exist.txt').exists()
   assert all(not r.get('review') for r in corrected_detail['runs'])
   blocked=session('blocked steps fixture');blocked_detail=wait_for(blocked,lambda d:not d['modelPending']);assert blocked_detail['tasks'][-1]['status']=='complete'
   assert [r['status'] for r in blocked_detail['runs']]==['succeeded','failed','succeeded']
   assert all(r['tool']=='update_plan' and not r.get('review') for r in blocked_detail['runs'])
   assert json.loads(blocked_detail['runs'][0]['output'])['plan'][0]['status']=='blocked'
   assert json.loads(blocked_detail['runs'][-1]['output'])['plan'][1]['reason']=='本轮不发布'
   assert call('agent_session',{'sessionId':blocked})['runs']==blocked_detail['runs']
   invalid_plan=session('invalid plan fixture');invalid_detail=wait_for(invalid_plan,lambda d:not d['modelPending']);assert len(invalid_detail['runs'])==1 and invalid_detail['runs'][0]['status']=='failed'
   planned=session('plan fixture');plan_detail=wait_for(planned,lambda d:not d['modelPending'])
   assert [r['tool'] for r in plan_detail['runs']]==['update_plan','read_file','update_plan']
   assert all(r['status']=='succeeded' for r in plan_detail['runs'])
   assert json.loads(plan_detail['runs'][-1]['output'])['plan'][1]['status']=='completed'
   assert call('agent_session',{'sessionId':planned})['runs']==plan_detail['runs']
   complete=session('阅读项目并总结 complete');complete_detail=wait_for(complete,lambda d:not d['modelPending']);assert complete_detail['tasks'][-1]['status']=='complete';assert len(complete_detail['runs'])==3
   # A regenerated answer owns a separate tool history and selects the new version.
   evidence=complete_detail['messages'][-1]['modelMetadata']['executionEvidence'];assert evidence['callCount']==3 and evidence['counts']['succeeded']==3 and evidence['goalStatus']=='not_assessed', evidence
   source=complete_detail['messages'][-1]['id'];call('agent_regenerate',{'sessionId':complete,'messageId':source});regen=wait_for(complete,lambda d:not d['modelPending']);assert len(regen['runs'])==6;assert len({r['responseId'] for r in regen['runs']})==2
   escaped=session('escape');escaped=wait_for(escaped,lambda d:not d['modelPending']);assert len(escaped['runs'])==3 and all(r['status']=='failed' for r in escaped['runs']);assert len([r for r in requests if any(m.get('role')=='user' and m.get('content')=='escape' for m in r['messages'])])==2;assert not (root/'unsafe').exists();assert 'never-expose' not in json.dumps(escaped)
   multiline=session('multiline event fixture');multi=wait_for(multiline,lambda d:not d['modelPending']);assert multi['tasks'][-1]['status']=='complete';assert multi['messages'][-1]['content']=='多行事件正常';assert multi['messages'][-1]['modelMetadata']['executionEvidence']['callCount']==0
   for prompt in ['missing finish fixture','unknown finish fixture']:
    incomplete=session(prompt);detail=wait_for(incomplete,lambda d:not d['modelPending'])
    assert detail['tasks'][-1]['status']=='failed' and not detail['runs']
    answer=next(m for m in reversed(detail['messages']) if m['role']=='assistant')
    assert 'Incomplete answer preserved' in answer['content']
    assert answer['modelMetadata']['taskReasonCode']=='incomplete_response'
    assert all(segment['kind']!='final' for segment in answer['modelMetadata']['presentation']['segments'])
    saved=call('agent_session',{'sessionId':incomplete})
    assert next(m for m in reversed(saved['messages']) if m['role']=='assistant')['modelMetadata']['taskReasonCode']=='incomplete_response'
    assert len([r for r in requests if any(m.get('role')=='user' and m.get('content')==prompt for m in r['messages'])])==1
   oversized=session('oversized event fixture');over=wait_for(oversized,lambda d:not d['modelPending']);assert over['tasks'][-1]['status']=='failed' and not over['runs'];assert not (root/'oversized-event.txt').exists()
   answer=next(m for m in reversed(over['messages']) if m['role']=='assistant');assert 'Before oversized event' in answer['content'];assert answer['modelMetadata']['taskReasonCode']=='stream_limit'
   assert next(m for m in reversed(call('agent_session',{'sessionId':oversized})['messages']) if m['role']=='assistant')['modelMetadata']['taskReasonCode']=='stream_limit'
   assert len([r for r in requests if any(m.get('role')=='user' and m.get('content')=='oversized event fixture' for m in r['messages'])])==1
   for prompt in ['malformed batch fixture','array batch fixture','duplicate batch fixture']:
    bad=session(prompt);bad_detail=wait_for(bad,lambda d:not d['modelPending']);assert bad_detail['tasks'][-1]['status']=='failed'
    assert not bad_detail['runs'] and not (root/'bad-batch.txt').exists()
    answer=next(m for m in reversed(bad_detail['messages']) if m['role']=='assistant');assert 'Batch preview preserved' in answer['content'];assert answer['modelMetadata']['taskReasonCode']=='invalid_tool_call'
    assert next(m for m in reversed(call('agent_session',{'sessionId':bad})['messages']) if m['role']=='assistant')['modelMetadata']['taskReasonCode']=='invalid_tool_call'
    assert len([r for r in requests if any(m.get('role')=='user' and m.get('content')==prompt for m in r['messages'])])==1
   for prompt,reason in [('output cutoff fixture','output_limit'),('filtered output fixture','model_filtered'),('tool cutoff fixture','output_limit')]:
    cutoff=session(prompt);cutoff_detail=wait_for(cutoff,lambda d:not d['modelPending']);assert cutoff_detail['tasks'][-1]['status']=='failed'
    assert not cutoff_detail['runs'] and not (root/'cutoff.txt').exists()
    answer=next(m for m in reversed(cutoff_detail['messages']) if m['role']=='assistant');assert 'Partial answer preserved' in answer['content'];assert answer['modelMetadata']['taskReasonCode']==reason
    assert next(m for m in reversed(call('agent_session',{'sessionId':cutoff})['messages']) if m['role']=='assistant')['modelMetadata']['taskReasonCode']==reason
    assert len([r for r in requests if any(m.get('role')=='user' and m.get('content')==prompt for m in r['messages'])])==(3 if prompt=='tool cutoff fixture' else 1)
   (root/'checks').mkdir();(root/'checks/test_ok.py').write_text('import unittest\nclass T(unittest.TestCase):\n def test_ok(self): self.assertEqual(2+2,4)\n')
   retained=session('plan retention fixture');rd=wait_for(retained,lambda d:not d['modelPending']);assert len(rd['runs'])==2 and all(r['status']=='succeeded' for r in rd['runs']);assert json.loads(rd['runs'][-1]['output'])['acceptance'][0]['status']=='unverified'
   unified=session('unified test fixture');ud=wait_for(unified,lambda d:any(r['status']=='pending_approval' for r in d['runs']));ur=ud['runs'][-1]
   assert ur['tool']=='run_tests' and ur['review']['testRunner']=='unittest'
   assert not list(root.glob('keeper-test-*.log'))
   call('agent_review_tool',{'sessionId':unified,'runId':ur['id'],'reviewHash':ur['review']['hash'],'approved':True})
   ud=wait_for(unified,lambda d:not d['modelPending']);assert ud['messages'][-1]['modelMetadata']['taskOutcome']['status']=='tests_passed'
   assert len(list(root.glob('keeper-test-*.log')))==2
   assert call('agent_session',{'sessionId':unified})['messages'][-1]['modelMetadata']['taskOutcome']==ud['messages'][-1]['modelMetadata']['taskOutcome']
   recovered=session('recover cutoff fixture');rd=wait_for(recovered,lambda d:not d['modelPending']);assert rd['tasks'][-1]['status'] not in ['failed','stopped'],rd['tasks']
   assert len(rd['runs'])==1 and rd['runs'][0]['status']=='succeeded' and not (root/'cutoff.txt').exists()
   tested=session('structured test fixture')
   for _ in range(2):
    td=wait_for(tested,lambda d:any(r['status']=='pending_approval' for r in d['runs']))
    run=next(r for r in td['runs'] if r['status']=='pending_approval')
    call('agent_review_tool',{'sessionId':tested,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True})
   td=wait_for(tested,lambda d:not d['modelPending'])
   parsed=[json.loads(r['output']) for r in td['runs']]
   assert all(v['testSummary']['verdict']=='failed' and v['exitCode']==0 for v in parsed)
   assert parsed[-1]['repeatTestHint']['previousFailures']==1
   alternating=session('alternating cycle');alternated=wait_for(alternating,lambda d:not d['modelPending']);assert alternated['tasks'][-1]['status']=='failed' and len(alternated['runs'])==6
   assert all(r['status']=='succeeded' for r in alternated['runs'])
   assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='no_progress' for m in alternated['messages'])
   assert '双步骤' in json.dumps(alternated,ensure_ascii=False)
   assert call('agent_session',{'sessionId':alternating})['runs']==alternated['runs']
   looping=session('loop');looped=wait_for(looping,lambda d:not d['modelPending']);assert looped['tasks'][-1]['status']=='failed' and len(looped['runs'])==3
   assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='no_progress' for m in looped['messages'] if m['role']=='assistant')
   assert '无进展' in json.dumps(looped,ensure_ascii=False)
   assert '无进展' in json.dumps(call('agent_session',{'sessionId':looping}),ensure_ascii=False)
   custom=call('agent_new_session')['id']
   for invalid in [0,129,1.5,'16']:
    call('agent_execution_budget',{'sessionId':custom,'maxRounds':invalid},expected=400)
   assert call('agent_execution_budget',{'sessionId':custom,'maxRounds':2})['executionBudget']['maxRounds']==2
   budget=call('agent_execution_budget',{'sessionId':custom,'maxToolCalls':64,'activeSeconds':600})['executionBudget']
   assert budget=={'maxRounds':2,'maxToolCalls':64,'activeSeconds':600}
   call('agent_execution_budget',{'sessionId':custom,'maxRounds':8,'activeSeconds':1801},expected=400)
   assert call('agent_session',{'sessionId':custom})['executionBudget']==budget
   call('agent_send_message',{'sessionId':custom,'content':'budget custom'})
   limited=wait_for(custom,lambda d:not d['modelPending']);assert len(limited['runs'])==2
   assert next(m for m in limited['messages'] if m['role']=='assistant')['modelMetadata']['taskBudget']==budget
   tool_limited=call('agent_new_session')['id']
   call('agent_execution_budget',{'sessionId':tool_limited,'maxRounds':16,'maxToolCalls':8,'activeSeconds':600})
   call('agent_send_message',{'sessionId':tool_limited,'content':'budget tool cap'})
   capped=wait_for(tool_limited,lambda d:not d['modelPending'])
   assert len(capped['runs'])==8
   assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='tool_limit' for m in capped['messages'])
   bounded=call('agent_new_session')['id'];call('agent_execution_budget',{'sessionId':bounded,'maxRounds':16});call('agent_send_message',{'sessionId':bounded,'content':'budget'});bounded=wait_for(bounded,lambda d:not d['modelPending']);assert len(bounded['runs'])==16 and '16 轮' in json.dumps(bounded,ensure_ascii=False)
   assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='round_limit' for m in bounded['messages'])
   assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='complete' for m in complete_detail['messages'])
   assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='user_stopped' for m in stopped['messages'])
   release.clear();broken=session('broken');partial=wait_for(broken,lambda d:bool(d['messages'][-1]['content']));call('agent_stop_generation',{'sessionId':broken,'messageId':partial['streamingMessageId']});assert wait_for(broken,lambda d:not d['modelPending'])['runs']==[]
   interrupted=session('阅读项目并总结 restart');persisted=wait_for(interrupted,lambda d:len(d['runs'])==3 and '正在整理' in d['messages'][-1]['content']);count=len(requests)
   process.terminate();process.wait(timeout=10);start();restored=call('agent_session',{'sessionId':interrupted});assert not restored['modelPending'] and restored['tasks'][-1]['status']=='interrupted';assert len(restored['runs'])==3;assert next(m for m in restored['messages'] if m['id']==persisted['messages'][-1]['id'])['modelMetadata']['presentation']==persisted['messages'][-1]['modelMetadata']['presentation']
   assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='interrupted' for m in restored['messages'])
   assert call('agent_session',{'sessionId':custom})['executionBudget']==budget
   time.sleep(.1);assert len(requests)==count,'restart must not replay tools or model calls'
   with sqlite3.connect(Path(folder)/'data.sql') as db:assert db.execute('SELECT count(*) FROM agent_events WHERE response_id=?',(persisted['streamingMessageId'],)).fetchone()[0]>5
   print('ReAct checks passed: fragmented native tool calls, multi-round results, realtime events, reload snapshots, stop, regeneration isolation, denied paths/tools, round limit and restart without replay.')
 finally:
  release.set()
  if process and process.poll() is None:process.terminate();process.wait(timeout=10)
  log.close();api.shutdown();api.server_close()
