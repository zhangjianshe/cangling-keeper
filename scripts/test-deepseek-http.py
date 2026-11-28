#!/usr/bin/env python3
"""Isolated DeepSeek-compatible fixture; never calls a paid/remote API."""
import http.server,json,os,subprocess,tempfile,threading,time,urllib.request,urllib.error,socket,stat
from pathlib import Path
from fixture_scope import respond_scope
received=[];release=threading.Event();mode={'fail':False}
class API(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def output(self,data,code=200):
  self.send_response(code);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(json.dumps(data).encode())
 def do_GET(self):
  assert self.path=='/models';assert self.headers['Authorization']=='Bearer '+self.server.expected_key;self.output({'data':[{'id':'deepseek-flash'}]})
 def do_POST(self):
  assert self.path=='/chat/completions';assert self.headers['Authorization']=='Bearer '+self.server.expected_key
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
  if respond_scope(self,body):return
  received.append(body);self.server.calls+=1
  if self.server.expected_key=='test-key-2':assert 'thinking' not in body
  assert body['stream'] is True
  if mode['fail']:
   self.output({'error':{'message':'test-key must not leak'}},401);return
  if body['messages'][-1]['content']=='等待首字测试':
   self.server.headers_waiting.set();release.wait(10)
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
  def event(text,reason=None):
   packet=('data: '+json.dumps({'model':'resolved-'+body['model'],'choices':[{'delta':{'content':text},'finish_reason':reason}]},ensure_ascii=False)+'\r\n\r\n').encode()
   # Deliberately split bytes, including UTF-8 characters and event delimiters.
   for offset in range(0,len(packet),3):self.wfile.write(packet[offset:offset+3]);self.wfile.flush()
  try:
   self.wfile.write(b': heartbeat\r\n\r\n');event('模拟')
   deadline=time.monotonic()+10
   while not release.wait(.02) and time.monotonic()<deadline:
    try:
     if self.connection.recv(1,socket.MSG_PEEK|socket.MSG_DONTWAIT)==b'':self.server.disconnects+=1;return
    except BlockingIOError:pass
   event('模型回复','stop');self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush()
  except (BrokenPipeError,ConnectionResetError):pass
api=http.server.ThreadingHTTPServer(('127.0.0.1',0),API);api.expected_key='test-key';api.calls=0;api.disconnects=0;api.headers_waiting=threading.Event()
api2=http.server.ThreadingHTTPServer(('127.0.0.1',0),API);api2.expected_key='test-key-2';api2.calls=0;api2.disconnects=0;api2.headers_waiting=threading.Event();threading.Thread(target=api2.serve_forever,daemon=True).start()
threading.Thread(target=api.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-deepseek-http-') as folder:
 with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
 base=f'http://127.0.0.1:{port}';proc=None
 log=open(Path(folder)/'server.log','wb')
 def start():
  global proc
  proc=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'TMPDIR':folder,'HOME':folder},stdin=subprocess.DEVNULL,stdout=log,stderr=log)
  for _ in range(100):
   try:urllib.request.urlopen(base+'/api/health',timeout=.3).close();return
   except Exception:time.sleep(.05)
  raise RuntimeError('Keeper failed to start')
 def call(name,args=None,expected=200):
  req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base})
  try:
   with urllib.request.urlopen(req,timeout=10) as r:code=r.status;out=json.load(r)
  except urllib.error.HTTPError as e:code=e.code;out=json.load(e)
  assert code==expected,(name,code,out)
  return out.get('result') if code==200 else out
 def finish(session):
  for _ in range(100):
   detail=call('agent_session',{'sessionId':session})
   if not detail['modelPending']:return detail
   time.sleep(.05)
  raise RuntimeError('Model reply timeout')
 try:
  start();assert not call('agent_model_config')['hasApiKey']
  config={'enabled':True,'baseUrl':f'http://127.0.0.1:{api.server_port}','model':'deepseek-flash','apiKey':'test-key'}
  saved=call('agent_model_save',config);assert saved['hasApiKey'];assert 'test-key' not in json.dumps(saved)
  assert call('agent_model_test')['models']==['deepseek-flash']
  session=call('agent_new_session')['id'];pending=call('agent_send_message',{'sessionId':session,'content':'第一轮'})
  assert pending['modelPending'];call('agent_session_model',{'sessionId':session,'id':saved['savedServerId']},expected=400);call('agent_send_message',{'sessionId':session,'content':'重复消息'},expected=400)
  for _ in range(100):
   snapshot=call('agent_session',{'sessionId':session})
   if snapshot['messages'][-1]['content']=='模拟':break
   time.sleep(.03)
  assert snapshot['modelPending'] and snapshot['messages'][-1]['content']=='模拟'
  assert snapshot['streamingMessageId']==snapshot['messages'][-1]['id']
  # A fresh browser can recover the same persisted partial text before completion.
  fresh=call('agent_session',{'sessionId':session});assert fresh['messages'][-1]['content']=='模拟'
  release.set();detail=finish(session);assert detail['messages'][-1]['content']=='模拟模型回复',detail
  original_reply=detail['messages'][-1]['id'];original_context=received[0]['messages']
  call('agent_send_message',{'sessionId':session,'content':'第二轮'});detail=finish(session)
  assert [m['role'] for m in received[-1]['messages']]==['system','user','assistant','user']
  assert 'test-key' not in json.dumps(received)
  mode['fail']=True;call('agent_send_message',{'sessionId':session,'content':'错误测试'});detail=finish(session)
  assert detail['messages'][-1]['role']=='error' and 'test-key' not in json.dumps(detail)
  proc.terminate();proc.wait(timeout=10);start();assert call('agent_model_config')['hasApiKey'];assert len(call('agent_session',{'sessionId':session})['messages'])==7
  assert stat.S_IMODE((Path(folder)/'data.sql').stat().st_mode)==0o600
  # Stop one stream without affecting a concurrent conversation.
  mode['fail']=False;release.clear();one=call('agent_new_session')['id'];two=call('agent_new_session')['id']
  first=call('agent_send_message',{'sessionId':one,'content':'停止测试 A'});second_stream=call('agent_send_message',{'sessionId':two,'content':'停止测试 B'})
  for _ in range(100):
   snapshot=call('agent_session',{'sessionId':one});other=call('agent_session',{'sessionId':two})
   if snapshot['messages'][-1]['content']=='模拟' and other['messages'][-1]['content']=='模拟':break
   time.sleep(.03)
  stop=call('agent_stop_generation',{'sessionId':one,'messageId':first['streamingMessageId']});assert stop['stopRequested']
  stopped=finish(one);assert stopped['messages'][-1]['content']=='模拟';assert stopped['messages'][-1]['modelMetadata']['generationStatus']=='stopped'
  call('agent_stop_generation',{'sessionId':one,'messageId':first['streamingMessageId']})
  assert call('agent_session',{'sessionId':two})['modelPending']
  for _ in range(100):
   if api.disconnects:break
   time.sleep(.02)
  assert api.disconnects>=1,'Stopping must close the provider HTTP stream'
  new=call('agent_send_message',{'sessionId':one,'content':'新回复不能被旧请求停止'})
  call('agent_stop_generation',{'sessionId':one,'messageId':first['streamingMessageId']},expected=400)
  assert call('agent_session',{'sessionId':one})['modelPending']
  call('agent_stop_generation',{'sessionId':one,'messageId':new['streamingMessageId']});finish(one)
  waiting=call('agent_new_session')['id'];wait=call('agent_send_message',{'sessionId':waiting,'content':'等待首字测试'});assert api.headers_waiting.wait(5)
  call('agent_stop_generation',{'sessionId':waiting,'messageId':wait['streamingMessageId']});before_first=finish(waiting);assert before_first['messages'][-1]['modelMetadata']['generationStatus']=='stopped'
  release.set();assert finish(two)['messages'][-1]['content']=='模拟模型回复'
  assert call('agent_session',{'sessionId':one})['messages'][1]['content']=='模拟'
  proc.terminate();proc.wait(timeout=10);start()
  assert call('agent_session',{'sessionId':one})['messages'][1]['modelMetadata']['generationStatus']=='stopped'
  # Preserve partial output and report interruption when Keeper itself restarts.
  mode['fail']=False;release.clear();interrupted=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':interrupted,'content':'重启测试'})
  for _ in range(100):
   snapshot=call('agent_session',{'sessionId':interrupted})
   if snapshot['messages'][-1]['content']=='模拟':break
   time.sleep(.03)
  assert snapshot['modelPending']
  proc.terminate();proc.wait(timeout=10);start();release.set()
  recovered=call('agent_session',{'sessionId':interrupted});assert not recovered['modelPending'];assert recovered['messages'][-2]['content']=='模拟';assert recovered['messages'][-1]['role']=='error'
  first_id=call('agent_model_config')['activeServerId']
  second={'name':'Second LLM','provider':'openai-compatible','enabled':True,'baseUrl':f'http://127.0.0.1:{api2.server_port}','model':'custom-model','apiKey':'test-key-2'}
  saved=call('agent_model_save',second);second_id=saved['savedServerId'];assert saved['activeServerId']==first_id;assert len(saved['servers'])==2;assert 'test-key-2' not in json.dumps(saved)
  edited=call('agent_model_save',{**second,'id':second_id,'name':'Second LLM edited','apiKey':''});assert next(s for s in edited['servers'] if s['id']==second_id)['hasApiKey']
  call('agent_model_test',{'id':second_id});assert call('agent_model_config')['activeServerId']==first_id
  chosen=call('agent_new_session')['id'];call('agent_session_model',{'sessionId':chosen,'id':second_id})
  assert call('agent_model_config')['activeServerId']==first_id
  assert call('agent_session',{'sessionId':session})['model']['activeServerId']==first_id
  assert call('agent_session',{'sessionId':chosen})['model']['activeServerId']==second_id
  first_calls=api.calls
  call('agent_send_message',{'sessionId':chosen,'content':'通过第二个服务回复'})
  call('agent_send_message',{'sessionId':session,'content':'此会话继续使用第一个服务'})
  finish(chosen);finish(session);assert api2.calls==1 and api.calls==first_calls+1
  old_replies=[m['modelMetadata'] for m in call('agent_session',{'sessionId':session})['messages'] if m['role']=='assistant']
  assert old_replies[0]['model']=='resolved-deepseek-flash' and old_replies[0]['requestedModel']=='deepseek-flash'
  call('agent_session_model',{'sessionId':session,'id':second_id});call('agent_send_message',{'sessionId':session,'content':'同一会话改用第二个模型'});mixed=finish(session)
  replies=[m['modelMetadata'] for m in mixed['messages'] if m['role']=='assistant']
  assert replies[:-1]==old_replies and replies[-1]['model']=='resolved-custom-model'
  assert replies[-1]['serverName']=='Second LLM edited'
  call('agent_model_save',{**second,'id':second_id,'apiKey':'','name':'Renamed after reply','model':'custom-new'})
  assert [m['modelMetadata'] for m in call('agent_session',{'sessionId':session})['messages'] if m['role']=='assistant']==replies
  # Restore this session's first server; none of its historical labels should change.
  call('agent_session_model',{'sessionId':session,'id':first_id})
  call('agent_model_select',{'id':second_id});newer=call('agent_new_session')['id']
  assert call('agent_session',{'sessionId':newer})['model']['activeServerId']==second_id
  assert call('agent_session',{'sessionId':session})['model']['activeServerId']==first_id
  proc.terminate();proc.wait(timeout=10);start()
  assert call('agent_session',{'sessionId':session})['model']['activeServerId']==first_id
  assert call('agent_session',{'sessionId':chosen})['model']['activeServerId']==second_id
  assert [m['modelMetadata'] for m in call('agent_session',{'sessionId':session})['messages'] if m['role']=='assistant']==replies
  # Regeneration must pin the old request model and context, not the current selection/config.
  call('agent_model_save',{**config,'id':first_id,'name':'Renamed primary','model':'new-default-model','apiKey':''})
  call('agent_session_model',{'sessionId':session,'id':second_id})
  source_before=next(m for m in call('agent_session',{'sessionId':session})['messages'] if m['id']==original_reply)
  release.clear();regenerated=call('agent_regenerate',{'sessionId':session,'messageId':original_reply})
  call('agent_regenerate',{'sessionId':session,'messageId':original_reply},expected=400)
  assert regenerated['model']['activeServerId']==second_id
  for _ in range(100):
   snapshot=call('agent_session',{'sessionId':session})
   if snapshot['messages'][-1]['content']=='模拟':break
   time.sleep(.03)
  release.set();new_detail=finish(session);new_reply=new_detail['messages'][-1]
  assert new_reply['modelMetadata']['regenerateOf']==original_reply and new_reply['modelMetadata']['requestedModel']=='deepseek-flash'
  assert new_reply['modelMetadata']['serverName']=='DeepSeek'
  assert received[-1]['model']=='deepseek-flash' and received[-1]['messages'][1:]==original_context[1:]
  assert next(m for m in new_detail['messages'] if m['id']==original_reply)==source_before
  call('agent_regenerate',{'sessionId':chosen,'messageId':original_reply},expected=400)
  # Stopped responses can regenerate, can be stopped again, and can then complete.
  release.clear();stopped_regen=call('agent_regenerate',{'sessionId':one,'messageId':first['streamingMessageId']})
  call('agent_stop_generation',{'sessionId':one,'messageId':stopped_regen['streamingMessageId']});assert finish(one)['messages'][-1]['modelMetadata']['generationStatus']=='stopped'
  release.set();call('agent_regenerate',{'sessionId':one,'messageId':first['streamingMessageId']});assert finish(one)['messages'][-1]['modelMetadata']['generationStatus']=='complete'
  # Alternative replies stay in one slot; selection persists and controls future context.
  group=next(g for g in new_detail['replyGroups'] if g['rootId']==original_reply)
  assert group['versionIds']==[original_reply,new_reply['id']] and group['selectedId']==new_reply['id']
  call('agent_reply_version',{'sessionId':session,'rootId':original_reply,'messageId':newer},expected=400)
  call('agent_reply_version',{'sessionId':chosen,'rootId':original_reply,'messageId':original_reply},expected=400)
  call('agent_reply_version',{'sessionId':session,'rootId':original_reply,'messageId':original_reply})
  proc.terminate();proc.wait(timeout=10);start()
  assert next(g for g in call('agent_session',{'sessionId':session})['replyGroups'] if g['rootId']==original_reply)['selectedId']==original_reply
  selected_detail=call('agent_session',{'sessionId':session})
  selected_ids={g['selectedId'] for g in selected_detail['replyGroups']}
  expected_assistants=sum(m['role']=='assistant' and m['id'] in selected_ids and bool(m['content']) for m in selected_detail['messages'])
  call('agent_send_message',{'sessionId':session,'content':'只使用选中的历史版本'})
  finish(session)
  # The original first-turn assistant appears once, at its original position, not again at the end.
  assert received[-1]['messages'][2]['content']==source_before['content']
  assert sum(m['role']=='assistant' for m in received[-1]['messages'])==expected_assistants
  release.clear();again=call('agent_regenerate',{'sessionId':session,'messageId':new_reply['id']})
  call('agent_reply_version',{'sessionId':session,'rootId':original_reply,'messageId':original_reply},expected=400)
  release.set();again=finish(session)
  group=next(g for g in again['replyGroups'] if g['rootId']==original_reply)
  assert len(group['versionIds'])==3 and group['selectedId']==again['messages'][-1]['id']
  # Editing a user message preserves both prompt and response revisions at the same slots.
  before_edit=call('agent_session',{'sessionId':session})
  user_message=next(m for m in before_edit['messages'] if m['role']=='user')
  call('agent_edit_message',{'sessionId':chosen,'messageId':user_message['id'],'content':'跨会话'},expected=400)
  call('agent_edit_message',{'sessionId':session,'messageId':original_reply,'content':'不能编辑助手消息'},expected=400)
  call('agent_edit_message',{'sessionId':session,'messageId':user_message['id'],'content':'   '},expected=400)
  release.clear();editing=call('agent_edit_message',{'sessionId':session,'messageId':user_message['id'],'content':'编辑后的第一轮问题'})
  call('agent_edit_message',{'sessionId':session,'messageId':user_message['id'],'content':'生成期间不能重复编辑'},expected=400)
  release.set();edited=finish(session)
  edited_user=next(m for m in edited['messages'] if m['modelMetadata'].get('editOf')==user_message['id'])
  edited_reply=edited['messages'][-1]
  assert edited_reply['modelMetadata']['promptMessageId']==edited_user['id']
  assert received[-1]['messages'][-1]['content']=='编辑后的第一轮问题' and len(received[-1]['messages'])==len(original_context)
  assert next(m for m in edited['messages'] if m['id']==user_message['id'])==user_message
  assert next(m for m in edited['messages'] if m['id']==original_reply)==source_before
  prompt_group=next(g for g in edited['replyGroups'] if g['rootId']==user_message['id'])
  assert prompt_group['versionIds']==[user_message['id'],edited_user['id']] and prompt_group['selectedId']==edited_user['id']
  assert next(g for g in edited['replyGroups'] if g['rootId']==original_reply)['versionIds']==[edited_reply['id']]
  call('agent_reply_version',{'sessionId':session,'rootId':user_message['id'],'messageId':user_message['id']})
  restored=call('agent_session',{'sessionId':session})
  assert edited_reply['id'] not in next(g for g in restored['replyGroups'] if g['rootId']==original_reply)['versionIds']
  call('agent_reply_version',{'sessionId':session,'rootId':user_message['id'],'messageId':edited_user['id']})
  proc.terminate();proc.wait(timeout=10);start()
  restored=call('agent_session',{'sessionId':session})
  assert next(g for g in restored['replyGroups'] if g['rootId']==user_message['id'])['selectedId']==edited_user['id']
  call('agent_send_message',{'sessionId':session,'content':'编辑后继续对话'});finish(session)
  context=received[-1]['messages']
  assert context[1]['content']=='编辑后的第一轮问题'
  assert not any(m['content']==user_message['content'] for m in context)
  assert context[2]['content']==edited_reply['content']
  # Editing the edited prompt makes a third version; regenerate retains that version's context.
  call('agent_edit_message',{'sessionId':session,'messageId':edited_user['id'],'content':'第三版第一轮问题'});third=finish(session)
  third_reply=third['messages'][-1]
  call('agent_regenerate',{'sessionId':session,'messageId':third_reply['id']});finish(session)
  assert received[-1]['messages'][-1]['content']=='第三版第一轮问题'
  call('agent_model_save',{**second,'id':second_id,'apiKey':'','enabled':False,'clearApiKey':True});assert not call('agent_model_config')['hasApiKey'];assert next(s for s in call('agent_model_config')['servers'] if s['id']==first_id)['hasApiKey']
  call('agent_send_message',{'sessionId':chosen,'content':'不可用时不能改用其他模型'},expected=400)
  assert call('agent_session',{'sessionId':chosen})['model']['activeServerId']==second_id
  unavailable=next(m['id'] for m in call('agent_session',{'sessionId':chosen})['messages'] if m['role']=='assistant')
  call('agent_regenerate',{'sessionId':chosen,'messageId':unavailable},expected=400)
  print('DeepSeek HTTP checks passed: config redaction, models, multi-turn context, SSE UTF-8 fragmentation, partial/reload, restart interruption, duplicate guard, safe errors, SQLite restart persistence key deletion, multiple server routing, independent edits, per-session model routing, new-session defaults, restart restore, no silent fallback, immutable reply model labels, per-request cancellation, socket close, early stop, stale-stop protection, original-model/context regeneration, stopped-reply regeneration, edited prompt/response version pairing, restart restore, selected-only context and OpenAI-compatible requests.')
 finally:
  release.set()
  if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=10)
  log.close();api.shutdown();api.server_close();api2.shutdown();api2.server_close()
