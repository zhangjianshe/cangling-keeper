#!/usr/bin/env python3
"""Exercise the local agent API using an isolated temporary data directory."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

binary = Path(sys.argv[1] if len(sys.argv)>1 else 'target/debug/cangling-keeper').resolve()
if os.name != 'posix':
    raise SystemExit('Run this isolated-instance HTTP test on Linux; backend unit tests are portable.')
with tempfile.TemporaryDirectory(prefix='keeper-agent-http-') as folder:
    data = Path(folder)
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0)); port = listener.getsockname()[1]
    base = f'http://127.0.0.1:{port}'
    log = open(data/'server.log', 'wb')
    process = None
    def start():
        global process
        process = subprocess.Popen([str(binary), '--listen', f'127.0.0.1:{port}', '--data-dir',folder,'--no-open'],env={**os.environ,'TMPDIR':folder,'HOME':folder},stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT)
        for _ in range(100):
            if process.poll() is not None: raise RuntimeError('Test Keeper exited')
            try:
                with urllib.request.urlopen(base+'/api/health',timeout=.3): return
            except Exception: time.sleep(.05)
        raise RuntimeError('Keeper health timeout')
    def call(command, args=None, origin=None, expected=200):
        request = urllib.request.Request(base+'/api/invoke/'+command,data=json.dumps(args or {}).encode(),headers={'Content-Type':'application/json','Origin':base if origin is None else origin})
        try:
            with urllib.request.urlopen(request,timeout=5) as response: code=response.status; value=json.load(response)
        except urllib.error.HTTPError as error: code=error.code; value=json.load(error)
        assert code==expected,(command,code,value)
        return value.get('result') if code==200 else value
    try:
        start()
        saved=call('set_login_server',{'serverUrl':'http://127.0.0.1:8080/ib/'})
        assert saved['serverUrl']=='http://127.0.0.1:8080/ib' and not saved['loggedIn']
        call('set_login_server',{'serverUrl':'http://name:password@localhost'},expected=400)
        initial=call('agent_bootstrap')
        assert initial['projects'][0]['id']=='general'
        assert Path(initial['projects'][0]['directory'])==data/'cangling/projects/general'
        project=call('agent_new_project',{'name':'测试项目 / ../ 安全名称'})
        assert Path(project['directory']).parent==data/'cangling/projects'
        assert Path(project['directory']).is_dir()
        project_session=call('agent_new_session',{'projectId':project['id']})['id']
        assert next(s for s in call('agent_bootstrap')['sessions'] if s['id']==project_session)['projectId']==project['id']
        call('agent_pin_session',{'sessionId':project_session,'pinned':True})
        assert next(s for s in call('agent_bootstrap')['sessions'] if s['id']==project_session)['pinned'] is True
        call('agent_new_session',{'projectId':'missing'},expected=400)
        call('agent_bootstrap',origin='https://untrusted.example',expected=403)
        host = {'id':'test','name':'test','hostname':'192.0.2.10','port':22,'username':'root','auth':{'method':'password','password':'do-not-return-this-password'}}
        call('add_host',{'host':host})
        session = call('agent_new_session')['id']
        call('agent_send_message',{'sessionId':session,'content':'HTTP persistence test'})
        call('agent_run_tool',{'sessionId':session,'tool':'list_hosts'})
        for _ in range(80):
            detail=call('agent_session',{'sessionId':session})
            if detail['runs'][0]['status']!='running':break
            time.sleep(.05)
        assert detail['runs'][0]['status']=='succeeded'
        assert 'do-not-return-this-password' not in json.dumps(detail)
        approval=call('agent_run_tool',{'sessionId':session,'tool':'ssh_execute','arguments':{'hostId':'test','command':'uname -a','target':{'hostname':'untrusted.example'}}})
        assert approval['status']=='pending_approval'
        process.terminate();process.wait(timeout=10);start()
        assert call('get_login_status')['serverUrl']=='http://127.0.0.1:8080/ib'
        assert any(p['id']==project['id'] for p in call('agent_bootstrap')['projects'])
        assert next(s for s in call('agent_bootstrap')['sessions'] if s['id']==project_session)['pinned'] is True
        call('agent_pin_session',{'sessionId':project_session,'pinned':False})
        detail=call('agent_session',{'sessionId':session})
        assert detail['messages'][0]['content']=='HTTP persistence test'
        assert detail['runs'][1]['status']=='pending_approval'
        assert detail['runs'][1]['arguments']['target']['hostname']=='192.0.2.10'
        call('agent_decide_tool',{'runId':approval['id'],'approved':False})
        call('agent_decide_tool',{'runId':approval['id'],'approved':True},expected=400)
        assert call('agent_session',{'sessionId':session})['runs'][1]['status']=='rejected'
        print('Agent HTTP checks passed: same-origin guard, real read-only tool, credential filtering, restart persistence, approval binding and replay protection.')
    finally:
        if process is not None and process.poll() is None: process.terminate();process.wait(timeout=10)
        log.close()
