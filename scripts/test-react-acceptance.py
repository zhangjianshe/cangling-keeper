#!/usr/bin/env python3
"""Local ReAct acceptance: temporary databases and mock providers only.
Build target/debug/cangling-keeper first; NODE_PATH must include jsdom.
"""
import json, os, subprocess, sys, tempfile, time
from pathlib import Path
root=Path(__file__).resolve().parents[1]
logs=Path(tempfile.mkdtemp(prefix='keeper-react-acceptance-'))
tests=[
 ['python3','scripts/test-react-http.py'],
 ['python3','scripts/test-account-sessions-http.py'],
 ['node','scripts/test-react-ui.cjs'],
 ['node','scripts/test-run-experience-ui.cjs'],
 ['node','scripts/test-transport-recovery.cjs'],
 ['node','scripts/test-account-sessions-ui.cjs'],
]
report={'scope':'local mock model; no real credentials or user database','tests':[]}
for index,command in enumerate(tests):
 log=logs/f'{index:02d}-{Path(command[1]).stem}.log';start=time.monotonic()
 try:
  with log.open('w') as out:
   result=subprocess.run(command,cwd=root,stdout=out,stderr=subprocess.STDOUT,timeout=120,env=os.environ.copy())
  status='passed' if result.returncode==0 else 'failed'
 except subprocess.TimeoutExpired:status='timeout'
 report['tests'].append({'command':command,'status':status,'seconds':round(time.monotonic()-start,2),'log':str(log)})
 print(status,command[1],flush=True)
(logs/'report.json').write_text(json.dumps(report,indent=2))
print('Report:',logs/'report.json',flush=True)
sys.exit(0 if all(t['status']=='passed' for t in report['tests']) else 1)
