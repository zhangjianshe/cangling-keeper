#!/usr/bin/env python3
"""Prepare isolated real coding tasks or score completed work. Never calls a model."""
import argparse,json,subprocess,sys,time
from pathlib import Path
cases=json.loads((Path(__file__).resolve().parents[1]/'evals/agent-tasks/cases.json').read_text())
p=argparse.ArgumentParser();p.add_argument('action',choices=['prepare','score']);p.add_argument('directory',type=Path);a=p.parse_args()
if a.action=='prepare':
 a.directory.mkdir(parents=True,exist_ok=False)
 for c in cases:
  root=a.directory/c['id'];root.mkdir()
  for name,body in c['files'].items():(root/name).write_text(body)
  (root/'TASK.md').write_text(c['prompt'])
 print('Prepared',len(cases),'tasks in',a.directory)
else:
 report=[]
 for c in cases:
  root=(a.directory/c['id']).resolve();start=time.monotonic()
  try:
   result=subprocess.run([sys.executable,'-c',c['checks']+'\nunittest.main()'],cwd=root,capture_output=True,text=True,timeout=30)
   preserved=all((root/name).is_file() and text in (root/name).read_text() for name,text in c['preserve'].items())
   report.append({'id':c['id'],'passed':result.returncode==0 and preserved,'preserved':preserved,'exitCode':result.returncode,'seconds':round(time.monotonic()-start,3),'output':(result.stdout+result.stderr)[-8000:]})
  except (OSError,subprocess.TimeoutExpired) as e:report.append({'id':c['id'],'passed':False,'error':str(e)})
 (a.directory/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2))
 print(json.dumps({'passed':sum(r['passed'] for r in report),'total':len(report),'report':str(a.directory/'report.json')},ensure_ascii=False))
 sys.exit(0 if all(r['passed'] for r in report) else 1)
