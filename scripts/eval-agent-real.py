#!/usr/bin/env python3
"""Real-model evaluation; isolated projects, guarded approvals, post-run scoring and undo."""
import json,urllib.request,urllib.error,time,pathlib,subprocess,sys,re,hashlib
import argparse
parser=argparse.ArgumentParser(description='Use the current localhost Keeper model for three real coding tasks (incurs model usage).')
parser.add_argument('--output',type=pathlib.Path,required=True)
parser.add_argument('--execute',action='store_true',help='Explicitly authorize real model requests and temporary evaluation projects')
parser.add_argument('--budget',type=int,choices=[65536,131072])
a=parser.parse_args()
if not a.execute:parser.error('--execute is required: this consumes model tokens')
repo=pathlib.Path(__file__).resolve().parents[1];out=a.output;out.mkdir(parents=True,exist_ok=False);base='http://127.0.0.1:4301'

def call(n,a={}):
 r=urllib.request.Request(base+'/api/invoke/'+n,data=json.dumps(a).encode(),headers={'Content-Type':'application/json','Origin':base})
 return json.load(urllib.request.urlopen(r,timeout=30))['result']
model=call('agent_bootstrap')['model'];cases=json.loads((repo/'evals/agent-tasks/cases.json').read_text());results=[]
for c in cases:
 p=call('agent_new_project',{'name':'真实评测 '+c['id']});root=pathlib.Path(p['directory'])
 for name,text in c['files'].items():(root/name).write_text(text)
 s=call('agent_new_session',{'projectId':p['id']})['id'];call('agent_session_model',{'sessionId':s,'id':model['id']})
 if a.budget:call('agent_context_budget',{'sessionId':s,'workingBudget':a.budget})
 start=time.monotonic();call('agent_send_message',{'sessionId':s,'content':c['prompt']+'\n仅操作当前项目。使用 python3 -m unittest discover -v 运行测试。'});approved=set();denied=[]
 print('START',c['id'],s,flush=True)
 while True:
  d=call('agent_session',{'sessionId':s});(out/(c['id']+'-detail.json')).write_text(json.dumps(d,ensure_ascii=False,indent=2))
  for r in d['runs']:
   if r['status']=='pending_approval' and r['id'] not in approved:
    v=r['review'];cmd=v.get('command','');path=v.get('path','');ok=(v.get('kind')=='file' and not pathlib.Path(path).is_absolute() and '..' not in pathlib.Path(path).parts) or (v.get('kind')=='command' and re.fullmatch(r'python3? -m (unittest|py_compile)[a-zA-Z0-9_ ./\-]*',cmd) is not None and '..' not in cmd and pathlib.Path(v.get('directory','')).resolve()==root.resolve())
    print('APPROVAL',c['id'],ok,path or cmd,flush=True);call('agent_review_tool',{'sessionId':s,'runId':r['id'],'reviewHash':v['hash'],'approved':ok});approved.add(r['id'])
    if not ok:denied.append(cmd or path)
  if not d['modelPending']:break
  if time.monotonic()-start>600:call('agent_stop_generation',{'sessionId':s});raise RuntimeError('task timeout')
  time.sleep(1)
 elapsed=time.monotonic()-start
 score=subprocess.run([sys.executable,'-c',c['checks']+'\nunittest.main()'],cwd=root,text=True,capture_output=True,timeout=30)
 preserved=all(t in (root/n).read_text() for n,t in c['preserve'].items());m=[m for m in d['messages'] if m['role']=='assistant'][-1];files=[r for r in d['runs'] if r['status']=='succeeded' and (r.get('review') or {}).get('kind')=='file' and json.loads(r.get('output') or '{}').get('applied')]
 record={'id':c['id'],'projectId':p['id'],'directory':str(root),'sessionId':s,'model':model['model'],'seconds':round(elapsed,2),'passed':score.returncode==0 and preserved,'preserved':preserved,'usage':m.get('modelMetadata',{}).get('usage'), 'taskStatus':d['tasks'][-1]['status'],'rounds':d['tasks'][-1]['step'],'taskBudget':m.get('modelMetadata',{}).get('taskBudget'),'errors':[m['content'] for m in d['messages'] if m['role']=='error'],'workingBudget':d.get('contextPolicy',{}).get('workingBudget'),'modelLimit':d.get('contextPolicy',{}).get('modelLimit'),'summaryRequests':m.get('modelMetadata',{}).get('contextSummaryRequests',0),'context':m.get('modelMetadata',{}).get('context'),'toolCalls':len(d['runs']),'fileChanges':len(files),'denied':denied,'testOutput':score.stdout+score.stderr}
 (out/(c['id']+'-completed.json')).write_text(json.dumps(record,ensure_ascii=False,indent=2));results.append(record);(out/'report.json').write_text(json.dumps(results,ensure_ascii=False,indent=2));print('DONE',c['id'],record['passed'],record['taskStatus'],record['seconds'],record['usage'],flush=True)
 # Verify later user changes prevent undo, then undo all tracked changes in reverse order.
 if files:
  last=files[-1];f=root/last['review']['path'];after=f.read_bytes();f.write_bytes(after+b'\n# user later edit\n')
  try:call('agent_undo_file_change',{'runId':last['id']});raise AssertionError('overwrote later edit')
  except urllib.error.HTTPError as e:assert e.code==400
  assert f.read_bytes()==after+b'\n# user later edit\n';f.write_bytes(after)
  for r in reversed(files):call('agent_undo_file_change',{'runId':r['id']})
  for n,t in c['files'].items():assert (root/n).read_text()==t
  for r in files:
   if r['review'].get('newFile'):assert not (root/r['review']['path']).exists()
  record['undoVerified']=True
  print('UNDO PASS',c['id'],flush=True)
 (out/'report.json').write_text(json.dumps(results,ensure_ascii=False,indent=2))

sys.exit(0 if all(r["passed"] and r["taskStatus"]=="complete" and r.get("undoVerified") for r in results) else 1)
