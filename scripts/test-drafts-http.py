#!/usr/bin/env python3
"""Disposable SQLite draft lifecycle; no external model requests."""
import os, json, socket, subprocess, tempfile, time, urllib.request, urllib.error, sqlite3
from pathlib import Path
with tempfile.TemporaryDirectory(prefix='keeper-drafts-') as folder:
    with socket.socket() as sock:
        sock.bind(('127.0.0.1',0)); port=sock.getsockname()[1]
    base=f'http://127.0.0.1:{port}'
    def call(name,args={},ok=True):
        req=urllib.request.Request(base+'/api/invoke/'+name,data=json.dumps(args).encode(),headers={'Content-Type':'application/json','Origin':base})
        try:
            with urllib.request.urlopen(req) as response: data=json.load(response)
            assert ok,(name,data)
            return data['result']
        except urllib.error.HTTPError:
            assert not ok,name
    def start():
        proc=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'TMPDIR':folder},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        for _ in range(100):
            try: call('agent_bootstrap');return proc
            except (OSError,AssertionError):time.sleep(.1)
        raise AssertionError('startup')
    p=start()
    try:
        sid=call('agent_new_session',{'projectId':'general'})['id'];args={'sessionId':sid}
        assert call('agent_draft_get',args)=={'content':'','revision':0}
        call('agent_draft_save',dict(args,content='unsent draft',revision=0))
        call('agent_draft_save',dict(args,content='stale',revision=0),False)
        call('agent_send_message',dict(args,content='message',draftRevision=0),False)
        assert not call('agent_session',args)['messages']
        p.terminate();p.wait();p=start()
        assert call('agent_draft_get',args)['content']=='unsent draft'
        call('agent_send_message',dict(args,content='message',draftRevision=1))
        assert call('agent_draft_get',args)=={'content':'','revision':2}
        call('agent_draft_save',dict(args,content='late stale save',revision=1),False)
        with sqlite3.connect(Path(folder)/'data.sql') as db:
            for key,value in {'server_url':'https://fixture.invalid/ib','login_token':'fixture','login_user_id':'another','login_username':'another'}.items():
                db.execute('INSERT OR REPLACE INTO settings(key,value) VALUES (?,?)',(key,value))
        call('agent_draft_get',args,False);call('agent_draft_save',dict(args,content='wrong owner',revision=2),False)
        print('PASS: restart persistence, conflict rollback, atomic send cleanup, stale writes, account isolation')
    finally:p.terminate();p.wait()
