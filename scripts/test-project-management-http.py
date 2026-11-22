#!/usr/bin/env python3
"""Disposable project lifecycle; no external model requests."""
import os, json, socket, subprocess, tempfile, time, urllib.request, urllib.error, sqlite3
from pathlib import Path
with tempfile.TemporaryDirectory(prefix='keeper-projects-') as folder:
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
        project=call('agent_new_project',{'name':'Project test'});pid=project['id'];args={'projectId':pid}
        directory=Path(project['directory']);marker=directory/'keep.txt';marker.write_text('keep')
        renamed=call('agent_project_update',dict(args,name='Renamed'))
        assert renamed['directory']==str(directory) and renamed['name']=='Renamed'
        call('agent_project_update',dict(args,name=' '),False)
        call('agent_project_update',dict(args,name='x'*81),False)
        call('agent_project_delete',args,False)
        call('agent_project_delete',{'projectId':'general','confirmed':True},False)
        sid=call('agent_new_session',args)['id']
        call('agent_project_delete',dict(args,confirmed=True),False)
        with sqlite3.connect(Path(folder)/'data.sql') as db:
            db.execute("UPDATE agent_sessions SET deleted_at=1,owner_key='other-owner' WHERE id=?",(sid,))
        call('agent_project_delete',dict(args,confirmed=True),False)
        with sqlite3.connect(Path(folder)/'data.sql') as db: db.execute('DELETE FROM agent_sessions WHERE id=?',(sid,))
        assert call('agent_project_delete',dict(args,confirmed=True))['filesPreserved']
        assert marker.read_text()=='keep'
        call('agent_project_open',args,False)
        marker.unlink();directory.rmdir()
        print('PASS: rename preserves directory; validation; confirmation; general protection; other owner trashed sessions block deletion; files preserved')
    finally:p.terminate();p.wait()
