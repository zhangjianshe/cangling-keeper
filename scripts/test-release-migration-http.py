#!/usr/bin/env python3
"""Legacy schema migration and versioned draft backup regression; isolated local data."""
import os, json, socket, subprocess, tempfile, time, urllib.request, urllib.error, sqlite3
from pathlib import Path
from contextlib import closing
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
        proc=subprocess.Popen([str(Path(os.environ.get('KEEPER_TEST_BINARY','target/debug/cangling-keeper')).resolve()),'--listen',f'127.0.0.1:{port}','--data-dir',folder,'--no-open'],env={**os.environ,'TMPDIR':folder,'TMP':folder,'TEMP':folder},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        for _ in range(100):
            try: call('agent_bootstrap');return proc
            except (OSError,AssertionError):time.sleep(.1)
        raise AssertionError('startup')
    p=start()
    try:
        sid=call('agent_new_session',{'projectId':'general'})['id'];args={'sessionId':sid}
        call('agent_send_message',dict(args,content='history sentinel'))
        call('agent_draft_save',dict(args,content='unsent draft sentinel',revision=0))
        mcp=call('agent_mcp_save',{'name':'legacy config','transport':'stdio','command':'python3','args':['never-run-fixture.py'],'enabled':False})['id']
        p.terminate();p.wait()
        with closing(sqlite3.connect(Path(folder)/'data.sql')) as db, db:
            db.execute('ALTER TABLE agent_mcp_servers DROP COLUMN plugin_manifest')
            db.execute('ALTER TABLE agent_cis_models DROP COLUMN supports_images')
            db.execute('DROP TABLE agent_mcp_images')
            db.execute("INSERT INTO agent_cis_models VALUES ('guest','legacy','Legacy','openai-compatible','fixture',32768,'revision')")
            db.execute("INSERT INTO agent_runs(id,session_id,tool,arguments,status,created_at,updated_at) VALUES ('legacy-run',?,'run_command','{}','running',1,1)",(sid,))
        for _ in range(2):
            p=start()
            assert call('agent_draft_get',args)['content']=='unsent draft sentinel'
            assert call('agent_session',args)['messages'][0]['content']=='history sentinel'
            assert call('agent_session',args)['runs'][0]['status']=='interrupted'
            assert call('agent_mcp_list')[0]['id']==mcp and call('agent_mcp_list')[0]['plugin']=={}
            with closing(sqlite3.connect(Path(folder)/'data.sql')) as db, db:
                assert db.execute("SELECT supports_images FROM agent_cis_models WHERE id='legacy'").fetchone()[0]==0
                assert db.execute('SELECT count(*) FROM agent_mcp_images').fetchone()[0]==0
            p.terminate();p.wait()
        p=start()
        archive=call('agent_history_backup');assert archive['snapshot']['version']==4
        assert archive['snapshot']['sessions'][0]['draft']=='unsent draft sentinel'
        def restore(body,ok=True):
            req=urllib.request.Request(base+'/api/history/restore',data=json.dumps(body).encode(),headers={'Content-Type':'application/json','Origin':base})
            try:
                with urllib.request.urlopen(req) as response:result=json.load(response)['result']
                assert ok;return result
            except urllib.error.HTTPError:assert not ok
        def apply(value):
            preview=restore({'mode':'preview','backup':value})
            return restore({'mode':'restore','backup':value,'confirmed':True,'expectedAccount':preview['accountKey']})
        restored=apply(archive)['sessionIds'][0]
        assert restored!=sid and call('agent_draft_get',{'sessionId':restored})=={'content':'unsent draft sentinel','revision':1}
        assert call('agent_draft_get',args)['content']=='unsent draft sentinel'
        import copy,hashlib
        def sign(value):value['sha256']=hashlib.sha256(json.dumps(value['snapshot'],sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest();return value
        for version in [1,2,3]:
            old=copy.deepcopy(archive);old['snapshot']['version']=version
            for session in old['snapshot']['sessions']:session.pop('draft',None)
            legacy=apply(sign(old))['sessionIds'][0];assert call('agent_draft_get',{'sessionId':legacy})['content']==''
        bad=copy.deepcopy(archive);bad['snapshot']['sessions'][0]['draft']='x'*32001;restore({'mode':'preview','backup':sign(bad)},False)
        bad=copy.deepcopy(archive);bad['snapshot']['version']=3;restore({'mode':'preview','backup':sign(bad)},False)
        print('PASS: idempotent schema migration, history/draft/config preservation, interrupted tasks not replayed, v1-v4 backup compatibility, draft restore and bounds')
    finally:p.terminate();p.wait()
