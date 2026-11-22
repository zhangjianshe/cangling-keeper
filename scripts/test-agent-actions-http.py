#!/usr/bin/env python3
"""Reviewed agent actions against an isolated localhost model and project."""
import hashlib,sqlite3,http.server,json,os,socket,subprocess,tempfile,threading,time,urllib.request,urllib.error,sys
from pathlib import Path
if os.name!='posix':raise SystemExit('This isolated HTTP command fixture runs on POSIX; file tools also have portable Rust unit tests.')
requests=[];preview='--preview' in sys.argv
class API(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body)
  assert self.headers['Authorization']=='Bearer fixture-only-key'
  prompt=next(m['content'] for m in reversed(body['messages']) if m['role']=='user')
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
  def event(delta,reason=None):
   data=('data: '+json.dumps({'model':'review-fixture','choices':[{'delta':delta,'finish_reason':reason}]},ensure_ascii=False)+'\n\n').encode();self.wfile.write(data);self.wfile.flush()
  try:
   if 'repeat reject' in prompt and body['messages'][-1]['role']=='tool':
    event({'tool_calls':[{'index':0,'id':'different-call-id','type':'function','function':{'name':'write_file','arguments':json.dumps({'content':'hello v1\n','path':'rejected.txt'})}}]},'tool_calls')
   elif body['messages'][-1]['role']=='tool':
    result=json.loads(body['messages'][-1]['content']);
    if 'large output' in prompt:assert result['exitCode']==7 and result['stderr'].endswith('FINAL ERROR') and result['stdout'].endswith('END\n')
    assert result['keeperResult']['automaticRetry'] is False;assert result['keeperResult']['outcome']==('rejected' if result.get('rejected') else 'timeout' if result.get('timedOut') else 'failed' if result.get('error') or result.get('exitCode') not in (None,0) else 'truncated' if result.get('outputTruncated') or result.get('contextTruncated') else 'success');event({'content':'操作结果已保存。'+('用户拒绝，未执行。' if result.get('rejected') else '')},'stop')
   else:
    if 'create only' in prompt:name,args='write_file',{'path':'create-only.txt','content':'created safely','createOnly':True}
    elif 'multi patch' in prompt:name,args='apply_patch',{'path':'multi.txt','edits':[{'oldText':'first','newText':'ONE'},{'oldText':'absent' if 'invalid' in prompt else 'second','newText':'TWO'}]}
    elif 'stale revision' in prompt:name,args='write_file',{'path':'version.txt','content':'overwrite','expectedSha256':hashlib.sha256(b'old').hexdigest()}
    elif 'valid revision' in prompt:name,args='write_file',{'path':'version.txt','content':'updated','expectedSha256':hashlib.sha256(b'current').hexdigest()}
    elif 'patch' in prompt:name,args='apply_patch',{'path':'hello.txt','oldText':'hello v1','newText':'hello v2'}
    elif 'large output' in prompt:name,args='run_command',{'command':"python3 -c \"import sys; print('BEGIN'+'x'*100000+'END'); sys.stderr.write('ERROR-BEGIN'+'e'*100000+'FINAL ERROR'); sys.exit(7)\"",'timeoutSeconds':10}
    elif 'long' in prompt:name,args='run_command',{'command':"printf 'starting\\n'; sleep 120 & child=$!; echo $child > child.pid; wait $child; printf bad > after-stop.txt",'timeoutSeconds':120}
    elif 'timeout' in prompt:name,args='run_command',{'command':'sleep 10; printf bad > timeout-result.txt','timeoutSeconds':1}
    elif 'command' in prompt:name,args='run_command',{'command':"printf 'stream-out\\n'; printf 'stream-err\\n' >&2; printf completed > command-result.txt",'timeoutSeconds':10}
    elif 'parent' in prompt:name,args='write_file',{'path':'sub/child.txt','content':'new child\n'}
    else:
     filename='rejected.txt' if 'reject' in prompt else 'cancelled.txt' if 'cancel' in prompt else 'restart.txt' if 'restart' in prompt else 'hello.txt'
     name,args='write_file',{'path':filename,'content':'hello v1\n'}
    event({'content':'我会先展示操作内容，等待你确认。'})
    raw=json.dumps(args,ensure_ascii=False);mid=len(raw)//2
    event({'tool_calls':[{'index':0,'id':'review-call','type':'function','function':{'name':name,'arguments':raw[:mid]}}]})
    event({'tool_calls':[{'index':0,'function':{'arguments':raw[mid:]}}]},'tool_calls')
   self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush()
  except (BrokenPipeError,ConnectionResetError):pass
api=http.server.ThreadingHTTPServer(('127.0.0.1',0),API);threading.Thread(target=api.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-action-http-') as folder:
 with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
 base=f'http://127.0.0.1:{port}';process=None;log=open(Path(folder)/'server.log','wb')
 def call(name,args=None,expected=200,origin=None):
  req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':origin or base})
  try:
   with urllib.request.urlopen(req,timeout=10) as r:status=r.status;result=json.load(r)
  except urllib.error.HTTPError as e:status=e.code;result=json.load(e)
  assert status==expected,(name,status,result)
  return result.get('result') if status==200 else result
 def start():
  global process
  process=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'TMPDIR':folder},stdout=log,stderr=log)
  for _ in range(100):
   try:call('agent_bootstrap');return
   except Exception:time.sleep(.05)
  raise RuntimeError('Keeper failed to start')
 def wait(s,predicate):
  for _ in range(300):
   d=call('agent_session',{'sessionId':s})
   if predicate(d):return d
   time.sleep(.025)
  raise AssertionError('Agent state timed out')
 def pending(prompt):
  s=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':s,'content':prompt});d=wait(s,lambda d:bool(d['runs']) and d['runs'][-1]['status']=='pending_approval' and d['tasks'][-1]['status']=='approval');return s,d['runs'][-1],d
 def approve(s,run,approved=True,expected=200,**extra):return call('agent_review_tool',{'sessionId':s,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':approved,**extra},expected)
 def done(s):return wait(s,lambda d:not d['modelPending'])
 def alive(pid):
  try:
   stat=Path(f'/proc/{pid}/stat').read_text().split();return stat[2]!='Z'
  except FileNotFoundError:return False
 try:
  start();process.terminate();process.wait(timeout=10);root=Path(folder)/'project';root.mkdir();
  with sqlite3.connect(Path(folder)/'data.sql') as db:db.execute("UPDATE agent_projects SET directory=? WHERE id='general'",(str(root),))
  start();(root/'sub').mkdir()
  call('agent_model_save',{'name':'Review fixture','provider':'openai-compatible','enabled':True,'baseUrl':f'http://127.0.0.1:{api.server_port}','model':'fixture','apiKey':'fixture-only-key'})
  s,run,d=pending('write new file')
  assert not (root/'hello.txt').exists();assert d['tasks'][-1]['status']=='approval';assert '+hello v1' in run['review']['diff']
  assert call('agent_session',{'sessionId':s})['runs'][-1]['review']==run['review'],'refresh must recover same immutable preview'
  if preview:
   print(json.dumps({'url':base,'session':s,'directory':str(root)}),flush=True);input()
  else:
   call('agent_decide_tool',{'runId':run['id'],'approved':True},400)
   approve(s,run,expected=400,reviewHash='wrong')
   wrong=call('agent_new_session')['id'];approve(wrong,run,expected=400)
   call('agent_review_tool',{'sessionId':s,'runId':run['id'],'reviewHash':run['review']['hash'],'approved':True},403,origin='http://untrusted.invalid')
   approve(s,run,content='not-reviewed',arguments={'path':'tampered.txt','content':'not-reviewed'});approve(s,run,expected=400);d=done(s);assert (root/'hello.txt').read_text()=='hello v1\n';assert d['runs'][-1]['status']=='succeeded'
   s,run,_=pending('create only');assert run['review']['createOnly'] and run['review']['newFile'];approve(s,run);d=done(s);assert d['runs'][0]['status']=='succeeded';assert (root/'create-only.txt').read_text()=='created safely'
   existing=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':existing,'content':'create only existing'});d=done(existing);assert d['runs'][0]['status']=='failed' and not d['runs'][0].get('review');assert (root/'create-only.txt').read_text()=='created safely'
   (root/'create-only.txt').unlink();s,run,_=pending('create only conflict');(root/'create-only.txt').write_text('concurrent content');approve(s,run);assert done(s)['runs'][0]['status']=='failed';assert (root/'create-only.txt').read_text()=='concurrent content'
   (root/'multi.txt').write_text('first\nsecond\n')
   invalid=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':invalid,'content':'invalid multi patch'});invalid_detail=done(invalid);assert invalid_detail['runs'][0]['status']=='failed';assert not invalid_detail['runs'][0].get('review');assert (root/'multi.txt').read_text()=='first\nsecond\n'
   s,run,_=pending('multi patch');assert '+ONE' in run['review']['diff'] and '+TWO' in run['review']['diff'];assert (root/'multi.txt').read_text()=='first\nsecond\n';approve(s,run);multi_detail=done(s);assert len(multi_detail['runs'])==1 and multi_detail['runs'][0]['status']=='succeeded';assert (root/'multi.txt').read_text()=='ONE\nTWO\n';assert run['review']['changeSummary']=={'addedLines':2,'removedLines':2};committed=json.loads(multi_detail['runs'][0]['output']);assert committed['changeSummary']==run['review']['changeSummary'];assert committed['previousSha256']==run['review']['beforeHash'];assert committed['sha256']==run['review']['afterHash'];assert committed['created'] is False
   (root/'version.txt').write_text('current')
   stale=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':stale,'content':'stale revision'});stale_detail=done(stale)
   assert stale_detail['runs'][0]['status']=='failed' and not stale_detail['runs'][0].get('review');assert (root/'version.txt').read_text()=='current'
   s,run,_=pending('valid revision');assert run['review']['sourceHashVerified'];approve(s,run);done(s);assert (root/'version.txt').read_text()=='updated'
   s,run,_=pending('apply patch');assert '-hello v1' in run['review']['diff'] and '+hello v2' in run['review']['diff'];approve(s,run);done(s);assert (root/'hello.txt').read_text()=='hello v2\n'
   (root/'hello.txt').write_text('hello v1\n');s,run,_=pending('conflict patch');(root/'hello.txt').write_text('user edit\n');approve(s,run);d=done(s);assert d['runs'][-1]['status']=='failed';assert (root/'hello.txt').read_text()=='user edit\n'
   s,run,_=pending('parent changed');(root/'sub').rename(root/'moved');(root/'sub').mkdir();approve(s,run);assert done(s)['runs'][-1]['status']=='failed';assert not (root/'sub/child.txt').exists() and not (root/'moved/child.txt').exists()
   s,run,_=pending('repeat reject write');approve(s,run,False);blocked=done(s)
   assert len(blocked['runs'])==1 and blocked['tasks'][-1]['status']=='failed'
   assert '已被你拒绝' in json.dumps(blocked,ensure_ascii=False)
   assert any(m.get('modelMetadata',{}).get('taskReasonCode')=='rejected_repeat' for m in blocked['messages'])
   assert not (root/'rejected.txt').exists()
   assert call('agent_session',{'sessionId':s})['tasks']==blocked['tasks']
   s,run,_=pending('reject write');approve(s,run,False);assert done(s)['runs'][-1]['status']=='rejected';assert not (root/'rejected.txt').exists()
   s,run,d=pending('cancel before approval');call('agent_stop_generation',{'sessionId':s,'messageId':d['streamingMessageId']});d=done(s);assert d['runs'][-1]['status']=='stopped';approve(s,run,expected=400);assert not (root/'cancelled.txt').exists()
   s,run,_=pending('command succeeds');assert 'printf' in run['review']['command'] and run['review']['directory']==str(root);assert not (root/'command-result.txt').exists();approve(s,run,command='printf tampered > replaced-command.txt');d=done(s);assert not (root/'replaced-command.txt').exists();result=json.loads(d['runs'][-1]['output']);assert result['exitCode']==0 and 'stream-out' in result['stdout'] and 'stream-err' in result['stderr'];assert (root/'command-result.txt').read_text()=='completed'
   s,run,_=pending('large output');approve(s,run);d=done(s);result=json.loads(d['runs'][-1]['output']);assert result['exitCode']==7 and result['outputTruncated'];assert result['stdout'].startswith('BEGIN') and result['stdout'].endswith('END\n');assert result['stderr'].startswith('ERROR-BEGIN') and result['stderr'].endswith('FINAL ERROR');assert result['stdoutOmittedBytes']>0 and result['stderrOmittedBytes']>0;assert len(json.dumps(result))<46000
   s,run,d=pending('long command stop');approve(s,run);wait(s,lambda d:'starting' in d['runs'][-1]['output'] and (root/'child.pid').exists());pid=int((root/'child.pid').read_text());assert alive(pid)
   call('agent_stop_generation',{'sessionId':s,'messageId':d['streamingMessageId']});d=done(s);assert d['runs'][-1]['status']=='stopped' and 'starting' in d['runs'][-1]['output'];assert not alive(pid) and not (root/'after-stop.txt').exists()
   s,run,_=pending('timeout command');approve(s,run);d=done(s);assert d['runs'][-1]['status']=='failed' and json.loads(d['runs'][-1]['output'])['timedOut'];assert not (root/'timeout-result.txt').exists()
   s,run,_=pending('restart pending approval');count=len(requests);process.terminate();process.wait(timeout=10);start();restored=call('agent_session',{'sessionId':s});assert not restored['modelPending'];assert restored['runs'][-1]['status'] in ('stopped','interrupted');assert restored['runs'][-1]['review']==run['review'];approve(s,run,expected=400);assert not (root/'restart.txt').exists();time.sleep(.1);assert len(requests)==count
   s,run,d=pending('long command restart');approve(s,run);wait(s,lambda d:'starting' in d['runs'][-1]['output'] and (root/'child.pid').exists());pid=int((root/'child.pid').read_text());process.terminate();process.wait(timeout=10);assert not alive(pid);start();restored=call('agent_session',{'sessionId':s});assert not restored['modelPending'];assert restored['runs'][-1]['status'] in ('stopped','interrupted');assert not (root/'after-stop.txt').exists()
   print('Reviewed action checks passed: preview before effects, refresh restore, approval hash/session/origin/replay guards, atomic write/patch, stale file/parent conflict, rejection, pending stop, command streams, process-tree stop, timeout and restart without replay.')
 finally:
  if process and process.poll() is None:process.terminate();process.wait(timeout=10)
  log.close();api.shutdown();api.server_close()
