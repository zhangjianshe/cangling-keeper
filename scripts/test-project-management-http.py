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
        proc=subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'TMPDIR':folder,'HOME':folder},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        for _ in range(100):
            try: call('agent_bootstrap');return proc
            except (OSError,AssertionError):time.sleep(.1)
        raise AssertionError('startup')
    p=start()
    try:
        project=call('agent_new_project',{'name':'Project test'});pid=project['id'];args={'projectId':pid}
        directory=Path(project['directory']);marker=directory/'keep.txt';marker.write_text('keep')
        renamed=call('agent_project_update',dict(args,name='Renamed'))
        assert Path(renamed['directory']).name=='Renamed' and renamed['name']=='Renamed'
        directory=Path(renamed['directory']);marker=directory/'keep.txt'
        assert marker.read_text()=='keep'
        duplicate=call('agent_new_project',{'name':'Renamed'})
        assert Path(duplicate['directory']).name=='Renamed (1)'
        call('agent_new_project',{'name':'../escape'})
        assert Path(call('agent_new_project',{'name':'../escape'})['directory']).parent==directory.parent
        call('agent_project_update',dict(args,name=' '),False)
        call('agent_project_update',dict(args,name='x'*81),False)
        call('agent_project_delete',args,False)
        call('agent_project_delete',{'projectId':'general','confirmed':True},False)
        ids=[call('agent_new_session',args)['id'] for _ in range(3)]
        sid=ids[0]
        with sqlite3.connect(Path(folder)/'data.sql') as db:
            db.execute("UPDATE agent_sessions SET archived_at=1 WHERE id=?",(ids[1],))
            db.execute("UPDATE agent_sessions SET deleted_at=1,owner_key='other-owner' WHERE id=?",(ids[2],))
            db.execute("INSERT INTO agent_messages(id,session_id,role,content,created_at) VALUES ('project-message',?,'user','test',0)",(sid,))
            db.execute("INSERT INTO agent_model_contexts VALUES ('project-message','[]')")
            db.execute("INSERT INTO agent_model_pending(session_id,message_id) VALUES (?,'project-message')",(sid,))
        call('agent_project_delete',dict(args,confirmed=True),False)
        with sqlite3.connect(Path(folder)/'data.sql') as db:
            assert db.execute('SELECT count(*) FROM agent_sessions WHERE project_id=?',(pid,)).fetchone()[0]==3
            db.execute('DELETE FROM agent_model_pending WHERE session_id=?',(sid,))
            db.execute("CREATE TRIGGER fail_project_delete BEFORE UPDATE ON agent_projects BEGIN SELECT RAISE(ABORT,'fixture'); END")
        call('agent_project_delete',dict(args,confirmed=True),False)
        with sqlite3.connect(Path(folder)/'data.sql') as db:
            assert db.execute('SELECT count(*) FROM agent_sessions WHERE project_id=?',(pid,)).fetchone()[0]==3
            assert db.execute("SELECT count(*) FROM agent_model_contexts WHERE message_id='project-message'").fetchone()[0]==1
            db.execute('DROP TRIGGER fail_project_delete')
        result=call('agent_project_delete',dict(args,confirmed=True))
        assert result['filesPreserved'] and result['recycled'] and result['sessionCount']==3
        trash_directory=Path(result['directory'])
        assert trash_directory.name=='Renamed' and '.recycle-bin' in trash_directory.parts
        assert not directory.exists() and (trash_directory/'keep.txt').read_text()=='keep'
        assert any(p['id']==pid for p in call('agent_project_trash_list'))
        assert all(p['id']!=pid for p in call('agent_bootstrap')['projects'])
        call('agent_new_session',args,False)
        call('agent_restore_session',{'sessionId':ids[2]},False)
        call('agent_session',{'sessionId':sid},False)
        with sqlite3.connect(Path(folder)/'data.sql') as db:
            assert db.execute('SELECT count(*) FROM agent_sessions WHERE project_id=?',(pid,)).fetchone()[0]==3
            assert db.execute("SELECT count(*) FROM agent_messages WHERE id='project-message'").fetchone()[0]==1
            assert db.execute("SELECT count(*) FROM agent_model_contexts WHERE message_id='project-message'").fetchone()[0]==1
        assert all(s['projectId']!=pid for s in call('agent_bootstrap')['sessions'])
        occupied=call('agent_new_project',{'name':'Renamed'})
        assert Path(occupied['directory']).name=='Renamed'
        p.terminate();p.wait();p=start()
        assert any(item['id']==pid for item in call('agent_project_trash_list'))
        restored=call('agent_project_restore',args)
        directory=Path(restored['directory']);marker=directory/'keep.txt'
        assert restored['restored'] and directory.name=='Renamed (2)' and marker.read_text()=='keep'
        assert not trash_directory.exists()
        assert all(p['id']!=pid for p in call('agent_project_trash_list'))
        assert any(p['id']==pid for p in call('agent_bootstrap')['projects'])
        assert call('agent_session',{'sessionId':sid})['session']['id']==sid
        with sqlite3.connect(Path(folder)/'data.sql') as db:
            assert db.execute('SELECT archived_at FROM agent_sessions WHERE id=?',(ids[1],)).fetchone()[0]==1
            assert db.execute('SELECT deleted_at,owner_key FROM agent_sessions WHERE id=?',(ids[2],)).fetchone()==(1,'other-owner')
        call('agent_project_restore',args,False)
        # Simulate termination after filesystem rename, before SQLite commit.
        p.terminate();p.wait()
        pending=directory.parent/'interrupted-move';directory.rename(pending)
        journal=Path(folder)/'project-moves';journal.mkdir(exist_ok=True)
        (journal/'fixture.json').write_text(json.dumps({'project':pid,'old':str(directory),'target':str(pending)}))
        p=start()
        assert marker.read_text()=='keep' and not pending.exists()
        assert not (journal/'fixture.json').exists()
        # Legacy UUID directories acquire readable names on startup.
        p.terminate();p.wait()
        legacy=directory.parent/pid;directory.rename(legacy)
        with sqlite3.connect(Path(folder)/'data.sql') as db:
            db.execute('UPDATE agent_projects SET directory=? WHERE id=?',(str(legacy),pid))
        p=start()
        assert marker.read_text()=='keep' and not legacy.exists()

        print('PASS: named directories; collision handling; path safety; confirmation; busy guard; atomic rollback; recycle and restore all sessions/files with lifecycle preserved')
    finally:p.terminate();p.wait()
