#!/usr/bin/env python3
"""Conversation lifecycle/search regression in a disposable SQLite/project directory.

No real credentials, model requests, or existing Keeper data are used.
"""
import copy
import hashlib
import json
import os
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

BINARY = Path('target/debug/cangling-keeper').resolve()

with tempfile.TemporaryDirectory(prefix='keeper-history-management-') as folder:
    root = Path(folder)
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    base = f'http://127.0.0.1:{port}'
    process = None
    log = open(root / 'server.log', 'wb')

    def call(name, args=None, expected=200, path=None):
        request = urllib.request.Request(
            base + (path or '/api/invoke/' + name),
            data=json.dumps(args or {}, ensure_ascii=False).encode(),
            headers={'Content-Type': 'application/json', 'Origin': base})
        try:
            with urllib.request.urlopen(request, timeout=20) as response:
                status, value = response.status, json.load(response)
        except urllib.error.HTTPError as error:
            status, value = error.code, json.load(error)
        assert status == expected, (name, args, status, value)
        return value.get('result') if status == 200 else value

    def db():
        return sqlite3.connect(root / 'data.sql')

    def identity(user):
        with db() as conn:
            for key, value in {'server_url': 'https://fixture.invalid/ib',
                               'login_token': 'fixture-not-a-secret',
                               'login_user_id': str(user),
                               'login_username': f'user-{user}'}.items():
                conn.execute('INSERT OR REPLACE INTO settings(key,value) VALUES (?,?)', (key, value))

    def start():
        global process
        process = subprocess.Popen(
            [str(BINARY), '--listen', f'127.0.0.1:{port}', '--data-dir', folder, '--no-open'],
            env={**os.environ, 'HOME': folder, 'TMPDIR': folder}, stdout=log, stderr=log)
        for _ in range(120):
            if process.poll() is not None:
                raise AssertionError((root / 'server.log').read_text())
            try:
                call('agent_bootstrap')
                return
            except (OSError, AssertionError):
                time.sleep(.05)
        raise AssertionError('Keeper startup timed out')

    def stop():
        global process
        if process and process.poll() is None:
            process.terminate()
            process.wait(timeout=15)
        process = None

    def items(**args):
        return call('agent_sessions', args)['items']

    def ids(**args):
        return [item['id'] for item in items(**args)]

    def restore(backup):
        path = '/api/history/restore'
        preview = call('restore', {'mode': 'preview', 'backup': backup}, path=path)
        return call('restore', {'mode': 'restore', 'backup': backup, 'confirmed': True,
                                'expectedAccount': preview['accountKey']}, path=path)

    def sign(backup):
        backup['sha256'] = hashlib.sha256(json.dumps(backup['snapshot'], ensure_ascii=False,
            sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        return backup

    try:
        start()
        identity(1)
        project = call('agent_new_project', {'name': '会话管理测试'})['id']
        first = call('agent_new_session', {'projectId': project})['id']
        with db() as conn:
            owner = conn.execute('SELECT owner_key FROM agent_sessions WHERE id=?', (first,)).fetchone()[0]
            conn.execute('UPDATE agent_sessions SET title=?,updated_at=100 WHERE id=?', ('标题包含地质灾害', first))
            for index in range(72):
                conn.execute('INSERT INTO agent_sessions(id,title,created_at,updated_at,project_id,owner_key,pinned) VALUES (?,?,?,?,?,?,?)',
                    (f'page-{index:03}', f'Page {index:03}', 100, 100, project, owner, index == 0))
            conn.execute('INSERT INTO agent_messages(id,session_id,role,content,created_at,model_metadata) VALUES (?,?,?,?,?,?)',
                         ('u-search', first, 'user', '中文问题', 1, '{}'))
            for mid, content, metadata in [('a-search-root', 'original answer', {}),
                                          ('a-search-v2', '隐藏版本中特殊内容：地层搜索 100%_literal', {'regenerateOf': 'a-search-root'})]:
                conn.execute('INSERT INTO agent_messages(id,session_id,role,content,created_at,model_metadata) VALUES (?,?,?,?,?,?)',
                             (mid, first, 'assistant', content, 2, json.dumps(metadata)))
            conn.execute('INSERT INTO agent_reply_versions VALUES (?,?,?)', (first, 'a-search-root', 'a-search-root'))
        if '--preview' in sys.argv:
            print('PREVIEW URL: ' + base, flush=True)
            print('Disposable user-1 fixture: 73 conversations, an older pinned session, and historical reply versions.', flush=True)
            input('Press Enter after browser verification to stop and remove all fixture data. ')
            sys.exit(0)
        bootstrap = call('agent_bootstrap')
        assert len(bootstrap['sessions']) <= 30 and bootstrap['sessionPage']['hasMore']
        assert 'page-000' in [s['id'] for s in bootstrap['pinnedSessions']], 'older pins must remain accessible'
        seen, cursor = [], None
        while True:
            page = call('agent_sessions', {'projectId': project, 'limit': 7, **({'cursor': cursor} if cursor else {})})
            seen += [item['id'] for item in page['items']]
            assert len(page['items']) <= 7
            if not page['hasMore']:
                assert page['nextCursor'] is None
                break
            cursor = page['nextCursor']
            assert cursor and cursor['id']
        assert len(seen) == len(set(seen)) == 73, ('keyset duplicates or missing tied timestamps', len(seen))
        assert seen == sorted(seen, reverse=True), 'tie breaker must be stable ID descending'
        assert ids(q='地质灾害') == [first]
        result = items(q='地层搜索')[0]
        assert result['id'] == first
        assert result['match']['messageId'] == 'a-search-v2' and result['match']['rootId'] == 'a-search-root'
        assert '地层搜索' in result['match']['snippet']
        assert ids(q='100%_literal') == [first]
        assert not ids(q='100ZZliteral'), 'search treats percent and underscore as literal characters'
        other_project = call('agent_new_project', {'name': 'Other'})['id']
        assert not ids(q='地层搜索', projectId=other_project)
        call('agent_sessions', {'state': 'unknown'}, 400)
        call('agent_sessions', {'cursor': {'updatedAt': 'wrong', 'id': first}}, 400)

        identity(2)
        foreign = call('agent_new_session')['id']
        call('agent_rename_session', {'sessionId': foreign, 'title': 'BOB-PRIVATE-地层搜索'})
        assert first not in ids(q='地层搜索')
        for name, extra in [('agent_session', {}), ('agent_archive_session', {'archived': True}),
                            ('agent_delete_session', {}), ('agent_restore_session', {}),
                            ('agent_purge_session', {'confirm': True})]:
            call(name, {'sessionId': first, **extra}, 400)
        identity(1)
        assert foreign not in ids(q='BOB-PRIVATE')

        call('agent_archive_session', {'sessionId': first, 'archived': True})
        assert first not in ids(projectId=project)
        archived = items(state='archived', q='地层搜索')[0]
        assert archived['id'] == first and archived['archivedAt'] > 0
        assert call('agent_session', {'sessionId': first})['session']['archivedAt'] > 0
        for name, extra in [('agent_send_message', {'content': 'must reject'}),
                            ('agent_rename_session', {'title': 'must reject'}),
                            ('agent_pin_session', {'pinned': True}),
                            ('agent_security_mode', {'mode': 'full', 'confirmFullAccess': True}),
                            ('agent_run_tool', {'tool': 'repository_status', 'arguments': {}}),
                            ('agent_edit_message', {'messageId': 'u-search', 'content': 'must reject'}),
                            ('agent_regenerate', {'messageId': 'a-search-root'}),
                            ('agent_reply_version', {'rootId': 'a-search-root', 'messageId': 'a-search-v2'})]:
            call(name, {'sessionId': first, **extra}, 400)
        call('agent_delete_session', {'sessionId': first})
        assert first not in ids(state='archived') and first in ids(state='deleted')
        call('agent_restore_session', {'sessionId': first})
        assert first in ids(state='archived'), 'restore preserves pre-delete archived status'
        call('agent_archive_session', {'sessionId': first, 'archived': False})
        assert first in ids(projectId=project)
        call('agent_delete_session', {'sessionId': first})
        call('agent_restore_session', {'sessionId': first})
        assert first in ids(projectId=project), 'active soft-deleted session restores active'
        call('agent_purge_session', {'sessionId': first, 'confirm': True}, 400)

        # Lifecycle changes cannot strand a generation or an outstanding tool approval.
        for status in ('pending_approval', 'approved', 'running'):
            with db() as conn:
                conn.execute('INSERT INTO agent_runs(id,session_id,tool,arguments,status,created_at,updated_at) VALUES (?,?,?,?,?,?,?)',
                             ('busy-tool', first, 'run_command', '{}', status, 1, 1))
            for name, extra in [('agent_archive_session', {'archived': True}), ('agent_delete_session', {})]:
                call(name, {'sessionId': first, **extra}, 400)
            with db() as conn:
                conn.execute('DELETE FROM agent_runs WHERE id=?', ('busy-tool',))
        with db() as conn:
            conn.execute('INSERT INTO agent_model_pending(session_id,message_id) VALUES (?,?)', (first, 'a-search-root'))
        call('agent_delete_session', {'sessionId': first}, 400)
        call('agent_archive_session', {'sessionId': first, 'archived': True}, 400)
        with db() as conn:
            conn.execute('DELETE FROM agent_model_pending WHERE session_id=?', (first,))

        # The backup format keeps lifecycle state and still accepts the previous omission of both fields.
        call('agent_archive_session', {'sessionId': first, 'archived': True})
        deleted = call('agent_new_session', {'projectId': project})['id']
        call('agent_delete_session', {'sessionId': deleted})
        backup = call('agent_history_backup')
        backup_sessions = {s['id']: s for s in backup['snapshot']['sessions']}
        assert backup_sessions[first]['archivedAt'] > 0 and backup_sessions[deleted]['deletedAt'] > 0
        restored = restore(backup)['sessionIds']
        restored_states = [call('agent_session', {'sessionId': sid})['session'] for sid in restored]
        assert sum(s['archivedAt'] > 0 for s in restored_states) == 1
        assert sum(s['deletedAt'] > 0 for s in restored_states) == 1
        legacy = copy.deepcopy(backup)
        for session in legacy['snapshot']['sessions']:
            session.pop('archivedAt', None)
            session.pop('deletedAt', None)
        old = restore(sign(legacy))['sessionIds']
        assert all(call('agent_session', {'sessionId': sid})['session']['deletedAt'] == 0 for sid in old)

        # Purge every database dependent row, retain actual user files and other sessions.
        call('agent_archive_session', {'sessionId': first, 'archived': False})
        directory = Path(next(p for p in call('agent_bootstrap')['projects'] if p['id'] == project)['directory'])
        preserved_file = directory / 'user-project-file.txt'
        preserved_file.write_text('user data must survive conversation purge')
        with db() as conn:
            conn.execute('INSERT INTO agent_model_contexts VALUES (?,?)', ('a-search-root', '[]'))
            conn.execute('INSERT INTO agent_context_summaries VALUES (?,?,?,?,?)', (first, 'prefix', 'summary', 1, 1))
            conn.execute('INSERT INTO agent_runs(id,session_id,tool,arguments,status,created_at,updated_at,response_id) VALUES (?,?,?,?,?,?,?,?)',
                         ('purge-run', first, 'read_file', '{}', 'succeeded', 1, 1, 'a-search-root'))
            conn.execute('INSERT INTO agent_reviews VALUES (?,?,?)', ('purge-run', '{}', 'hash'))
            conn.execute('INSERT INTO agent_tasks(response_id,session_id,project_directory,status,created_at,updated_at) VALUES (?,?,?,?,?,?)',
                         ('a-search-root', first, str(directory), 'complete', 1, 1))
            conn.execute('INSERT INTO agent_events(session_id,response_id,event,payload,created_at) VALUES (?,?,?,?,?)',
                         (first, 'a-search-root', 'fixture', '{}', 1))
        call('agent_delete_session', {'sessionId': first})
        call('agent_purge_session', {'sessionId': first}, 400)
        with db() as conn:
            tables = ('agent_sessions', 'agent_messages', 'agent_model_contexts', 'agent_reviews', 'agent_runs',
                      'agent_tasks', 'agent_events', 'agent_reply_versions', 'agent_context_summaries')
            counts = {table: conn.execute('SELECT count(*) FROM ' + table).fetchone()[0] for table in tables}
            conn.execute('CREATE TRIGGER fixture_purge_failure BEFORE DELETE ON agent_messages '
                         'WHEN OLD.session_id="' + first + '" BEGIN SELECT RAISE(ABORT,"fixture purge rollback"); END')
        call('agent_purge_session', {'sessionId': first, 'confirm': True}, 400)
        with db() as conn:
            assert all(conn.execute('SELECT count(*) FROM ' + table).fetchone()[0] == count
                       for table, count in counts.items()), 'failed purge must roll back every dependent table'
            conn.execute('DROP TRIGGER fixture_purge_failure')
        call('agent_purge_session', {'sessionId': first, 'confirm': True})
        call('agent_session', {'sessionId': first}, 400)
        with db() as conn:
            for table in ('agent_sessions', 'agent_messages', 'agent_runs', 'agent_reply_versions',
                          'agent_model_pending', 'agent_tasks', 'agent_events', 'agent_context_summaries'):
                key = 'id' if table == 'agent_sessions' else 'session_id'
                assert conn.execute(f'SELECT count(*) FROM {table} WHERE {key}=?', (first,)).fetchone()[0] == 0, table
            assert not conn.execute('SELECT 1 FROM agent_reviews WHERE run_id=?', ('purge-run',)).fetchone()
            assert not conn.execute('SELECT 1 FROM agent_model_contexts WHERE message_id=?', ('a-search-root',)).fetchone()
            assert conn.execute('SELECT count(*) FROM agent_sessions').fetchone()[0] > 0
        assert preserved_file.read_text() == 'user data must survive conversation purge'
        stop()
        # Simulate the previous schema to verify non-destructive migration on restart.
        with db() as conn:
            before = conn.execute('SELECT count(*) FROM agent_messages').fetchone()[0]
            indexes = list(conn.execute("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='agent_sessions' AND sql IS NOT NULL"))
            for (name,) in indexes:
                conn.execute('DROP INDEX "' + name.replace('"', '""') + '"')
            conn.execute('ALTER TABLE agent_sessions DROP COLUMN archived_at')
            conn.execute('ALTER TABLE agent_sessions DROP COLUMN deleted_at')
        start()
        with db() as conn:
            assert conn.execute('SELECT count(*) FROM agent_messages').fetchone()[0] == before
            assert conn.execute('SELECT count(*) FROM agent_sessions WHERE archived_at<>0 OR deleted_at<>0').fetchone()[0] == 0
        print('PASS: account isolation; literal Chinese/title/body/version search; tied timestamp keyset paging; old pins; '
              'archive/read-only/delete/restore; busy guards; lifecycle backup and previous-format import; '
              'transactional dependent purge without deleting project files; non-destructive schema migration.')
    finally:
        stop()
        log.close()
