#!/usr/bin/env python3
"""Isolated deterministic model: evidence correction, bounded failure, Q&A and real tool recovery."""
import http.server,json,os,socket,subprocess,tempfile,threading,time,urllib.request
from pathlib import Path
from fixture_scope import respond_scope
mode='';requests=0
class Model(http.server.BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def do_POST(self):
  global requests
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
  if respond_scope(self,body):return
  requests+=1
  corrected=any(m.get('role')=='system' and '运行时校验发现' in str(m.get('content')) for m in body['messages'])
  tools=any(m.get('role')=='tool' for m in body['messages'])
  if mode=='tool' and corrected and not tools:
   delta={'tool_calls':[{'index':0,'id':'read-proof','type':'function','function':{'name':'list_directory','arguments':'{"path":"."}'}}]}
  else:
   text='你好，我是助手。' if mode=='qa' else '我尚未查询，无法确认数据集是否存在。' if mode=='correct' and corrected else '已查询项目目录。' if tools else '两个查询都返回空结果。检索了郑州，均无结果。'
   delta={'content':text}
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
  self.wfile.write(('data: '+json.dumps({'choices':[{'delta':delta,'finish_reason':'tool_calls' if 'tool_calls' in delta else 'stop'}]})+'\n\ndata: [DONE]\n\n').encode())
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Model);threading.Thread(target=server.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-evidence-') as tmp:
 with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
 base=f'http://127.0.0.1:{port}'
 def call(name,args={}):
  req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args).encode(),headers={'Content-Type':'application/json','Origin':base})
  with urllib.request.urlopen(req,timeout=10) as r:return json.load(r)['result']
 with open(Path(tmp)/'server.log','w') as log:
  proc=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',tmp,'--no-open'],env={**os.environ,'TMPDIR':tmp,'KEEPER_MAP_MCP_URL':''},stdout=log,stderr=log)
  try:
   for _ in range(100):
    try:call('agent_bootstrap');break
    except Exception:time.sleep(.1)
   call('agent_model_save',{'enabled':True,'baseUrl':f'http://127.0.0.1:{server.server_port}','model':'fixture','apiKey':'fixture'})
   for mode in ['correct','blocked','qa','tool']:
    requests=0;session=call('agent_new_session')['id'];call('agent_send_message',{'sessionId':session,'content':'请查询项目目录'})
    for _ in range(300):
     detail=call('agent_session',{'sessionId':session})
     if not detail['modelPending']:break
     time.sleep(.05)
    assert not detail['modelPending'],mode
    detail=call('agent_session',{'sessionId':session}) # reload persisted result
    msg=next(m for m in detail['messages'] if m['role']=='assistant')
    meta=msg.get('modelMetadata',msg.get('model_metadata',{}));meta=json.loads(meta) if isinstance(meta,str) else meta
    assert '两个查询都返回空' not in msg['content'],(mode,msg)
    if mode=='qa':assert requests==1 and meta['generationStatus']=='complete',meta
    elif mode=='blocked':assert requests==2 and meta['generationStatus']=='failed' and meta['evidenceValidation']['status']=='blocked',meta
    elif mode=='correct':assert requests==2 and meta['evidenceValidation']['status']=='corrected' and not detail['runs'],meta
    else:assert requests==3 and len(detail['runs'])==1 and detail['runs'][0]['status']=='succeeded',detail
    print('PASS',mode,'model requests',requests)
  finally:proc.terminate();proc.wait(10)
server.shutdown();server.server_close()
