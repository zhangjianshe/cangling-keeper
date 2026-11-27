#!/usr/bin/env python3
"""Resume a retained long-task evaluation with a concrete acceptance failure."""
import argparse,hashlib,json,os,socket,sqlite3,subprocess,time,urllib.request,re
from pathlib import Path
from eval_long_task_fixture import fixture

def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--execute',action='store_true');parser.add_argument('--source-data',type=Path,required=True);parser.add_argument('--batch',type=Path,required=True);args=parser.parse_args()
 if not args.execute:parser.error('--execute required; consumes live model usage')
 out=args.batch.resolve();data=out/'private-state';previous=json.loads((out/'long_task-detail.json').read_text());session=previous['sessionId']
 original_prompt=next(m['content'] for m in reversed(previous['messages']) if m['role']=='user' and 'Complete this standard-library-only' in m['content']);marker=re.search(r'"marker":"([^"]+)"',original_prompt).group(1)
 files,_,checks=fixture(marker)
 with sqlite3.connect(data/'data.sql') as db:root=Path(db.execute('SELECT directory FROM agent_projects WHERE id=?',(previous['session']['projectId'],)).fetchone()[0])
 prompt=('Independent acceptance found a real contract violation in your previous implementation, despite the 17 supplied tests passing. '
  'The original specification requires canonically equivalent aggregated items and discount rates to be idempotent. '
  'A request for A quantity 2 and B quantity 3 with discount=".1", retried with the same request_id, reordered/aggregated identical items and discount="0.10", '
  'raises ValueError("Discount mismatch for reused request_id") instead of returning the same receipt without deducting stock again. '
  'Inspect the existing implementation, reproduce this in a NEW unittest regression file, and fix the implementation. '
  'Use Decimal-normalized semantic values; do not compare a requested rate against the monetary discount amount. '
  'Preserve test_fulfillment.py, sample.csv, verification.json and all public APIs and signatures. Do not weaken assertions or bypass validation. '
  'Check canonical retry, discount rounding, insufficient-stock atomic rollback, repeat cancellation, empty reports and invalid discounts. '
  'Run run_tests(directory=".") after the fix. Keep all operations in this existing project; do not install packages or use arbitrary shell commands. '
  'Finish with actual results. This is a targeted repair after a FAILED independent check, not a replacement for the original result.')
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 base=f'http://127.0.0.1:{port}';opener=urllib.request.build_opener(urllib.request.ProxyHandler({}));process=None;log=(out/'repair-server.log').open('wb');started=time.monotonic()
 def call(name,payload=None):
  req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(payload or {}).encode(),headers={'Content-Type':'application/json','Origin':base})
  with opener.open(req,timeout=30) as response:return json.load(response)['result']
 def save(name,value):
  target=out/name;temp=target.with_suffix('.tmp');temp.write_text(json.dumps(value,ensure_ascii=False,indent=2));temp.replace(target)
 try:
  with sqlite3.connect('file:'+str(args.source_data.resolve()/'data.sql')+'?mode=ro',uri=True) as source,sqlite3.connect(data/'data.sql') as target:
   token=source.execute("SELECT value FROM settings WHERE key='login_token'").fetchone();assert token
   target.execute("INSERT OR REPLACE INTO settings VALUES ('login_token',?)",token)
  binary=Path(__file__).resolve().parents[1]/'target/debug/cangling-keeper';binary_sha=hashlib.sha256(binary.read_bytes()).hexdigest()
  process=subprocess.Popen([str(binary),'--listen',f'127.0.0.1:{port}','--data-dir',str(data),'--no-open','--no-https'],env={**os.environ,'HOME':str(data/'home'),'TMPDIR':str(data)},stdout=log,stderr=log)
  for _ in range(100):
   try:call('agent_bootstrap');break
   except Exception:time.sleep(.1)
  detail=call('agent_session',{'sessionId':session});assert detail['executionBudget']=={'maxRounds':128,'maxToolCalls':128,'activeSeconds':1800}
  call('agent_send_message',{'sessionId':session,'content':prompt});print('START targeted independent-check repair',flush=True)
  while True:
   detail=call('agent_session',{'sessionId':session});save('repair-detail.json',detail)
   for run in detail['runs']:
    if run['status']!='pending_approval':continue
    review=run['review'];relative=Path(review.get('path','/'))
    allowed=review.get('kind')=='file' and not relative.is_absolute() and '..' not in relative.parts
    allowed|=run['tool']=='run_tests' and review.get('testRunner')=='unittest' and Path(review.get('directory','/')).resolve()==root.resolve()
    call('agent_review_tool',{'sessionId':session,'runId':run['id'],'reviewHash':review['hash'],'approved':bool(allowed)})
   if not detail['modelPending']:break
   if time.monotonic()-started>1980:
    call('agent_stop_generation',{'sessionId':session,'messageId':detail['streamingMessageId']});raise RuntimeError('repair wall budget exceeded')
   time.sleep(.5)
  scored=subprocess.run(['python3','-c',checks],cwd=root,capture_output=True,text=True,timeout=15)
  answer=next(m for m in reversed(detail['messages']) if m['role']=='assistant');meta=answer['modelMetadata'];runs=[r for r in detail['runs'] if r['responseId']==answer['id']]
  preserved=all((root/name).read_text()==text for name,text in files.items() if name.startswith('test_') or name in ['sample.csv','repeat.txt'])
  record={'stage':'targeted-repair-64k','sessionId':session,'seconds':round(time.monotonic()-started,2),'taskStatus':detail['tasks'][-1]['status'],'rounds':detail['tasks'][-1]['step'],'toolCalls':len(runs),'summaryRequests':meta.get('contextSummaryRequests',0),'readStallRecoveries':meta.get('readStallRecoveries',0),'taskBudget':meta.get('taskBudget'),'context':meta.get('context'),'usage':meta.get('usage'),'reason':meta.get('taskReasonCode'),'inputsPreserved':preserved,'independentCheckPassed':scored.returncode==0,'checkOutput':scored.stdout+scored.stderr,'binarySha256':binary_sha,'promptSha256':hashlib.sha256(prompt.encode()).hexdigest(),'newRegressionFiles':[p.name for p in root.glob('test*.py') if p.name not in files]}
  record['passed']=preserved and scored.returncode==0 and record['taskStatus']=='complete' and bool(record['newRegressionFiles']);save('repair-report.json',record);print('DONE',json.dumps(record,ensure_ascii=False),flush=True)
  return 0 if record['passed'] else 1
 finally:
  if process and process.poll() is None:process.terminate();process.wait(timeout=15)
  with sqlite3.connect(data/'data.sql') as db:
   db.execute('PRAGMA secure_delete=ON');db.execute("DELETE FROM settings WHERE key='login_token'");db.commit();db.execute('VACUUM')
  log.close()
if __name__=='__main__':raise SystemExit(main())
