#!/usr/bin/env python3
"""Bounded local release regression. No publish, real model or user database access."""
import json,os,subprocess,sys,tempfile,time
from pathlib import Path
root=Path(__file__).resolve().parents[1]
logs=Path(tempfile.mkdtemp(prefix='keeper-release-regression-'))
tests=[
 ['node','scripts/test-transport-recovery.cjs'],
 ['python3','scripts/test-linux-self-update.py','target/debug/cangling-keeper'],
 ['python3','scripts/test-release-migration-http.py'],
 ['python3','scripts/test-history-backup-http.py'],
 ['python3','scripts/test-account-sessions-http.py'],
 ['python3','scripts/test-mcp-sessions-http.py'],
 ['python3','scripts/test-mcp-images-http.py'],
 ['python3','scripts/test-plugins-http.py'],
 ['node','scripts/test-react-ui.cjs'],
 ['node','scripts/test-account-sessions-ui.cjs'],
 ['node','scripts/test-history-backup-ui.cjs'],
 ['node','scripts/test-drafts-ui.cjs'],
 ['python3','scripts/test-install-socket-activation.py'],
 ['python3','scripts/test-project-management-http.py'],
 ['python3','scripts/test-project-purge-http.py'],
 ['python3','scripts/test-context-http.py'],
 ['python3','scripts/test-plan-policy-http.py'],
 ['node','scripts/test-plan-progress-ui.cjs'],
 ['python3','scripts/test-react-http.py'],
 ['node','scripts/test-project-browser-ui.cjs'],
 ['node','scripts/test-project-management-ui.cjs'],
 ['node','scripts/test-project-trash-ui.cjs'],
 ['node','scripts/test-new-project-session-ui.cjs'],
 ['node','scripts/test-session-selection-ui.cjs'],
 ['node','scripts/test-sidebar-nav-ui.cjs'],
 ['node','scripts/test-notice-timeout-ui.cjs'],
 ['node','scripts/test-context-budget-ui.cjs'],
 ['node','scripts/test-conversation-options-ui.cjs'],
]
report={'platform':sys.platform,'tests':[],'notVerified':['Windows native upgrade/runtime','ARM native upgrade/runtime','macOS runtime','Production CIS migration','Live provider connectivity']}
try:
 for index,command in enumerate(tests):
  log=logs/f'{index:02d}-{Path(command[1]).stem}.log';start=time.monotonic()
  try:
   with log.open('w') as output:result=subprocess.run(command,cwd=root,stdout=output,stderr=subprocess.STDOUT,timeout=240,env=os.environ.copy())
   status='passed' if result.returncode==0 else 'failed'
  except subprocess.TimeoutExpired:status='timeout'
  report['tests'].append({'command':command,'status':status,'seconds':round(time.monotonic()-start,2),'log':str(log)})
  print(status,command[1],flush=True)
finally:
 (logs/'report.json').write_text(json.dumps(report,indent=2));print('Report:',logs/'report.json',flush=True)
sys.exit(0 if all(t['status']=='passed' for t in report['tests']) else 1)
