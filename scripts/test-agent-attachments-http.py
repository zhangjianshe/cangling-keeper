#!/usr/bin/env python3
"""Disposable attachment lifecycle and backup regression; only a loopback model fixture is used."""
import base64
import copy
import hashlib
import http.server
import json
import os
import socket
import sqlite3
import struct
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
import zlib
from pathlib import Path

requests = []
class Model(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        requests.append(body)
        events = [{'choices': [{'delta': {'content': 'Attachment fixture response'}, 'finish_reason': None}]},
                  {'choices': [{'delta': {}, 'finish_reason': 'stop'}],
                   'usage': {'prompt_tokens': 30, 'completion_tokens': 5, 'total_tokens': 35}}]
        output = ''.join('data: ' + json.dumps(event) + '\n\n' for event in events) + 'data: [DONE]\n\n'
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(output.encode())))
        self.end_headers()
        self.wfile.write(output.encode())


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data) & 0xffffffff)

def png(width=1, height=1, extra=0):
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
            + (chunk(b'tEXt', b'note\0' + b'x' * extra) if extra else b'')
            + chunk(b'IDAT', zlib.compress(b'\0\x20\x80\xc0')) + chunk(b'IEND', b''))


def signed(backup):
    value = copy.deepcopy(backup)
    value['sha256'] = hashlib.sha256(json.dumps(value['snapshot'], ensure_ascii=False,
        sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    return value


with tempfile.TemporaryDirectory(prefix='keeper-attachments-test-') as folder:
    root = Path(folder)
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    base = f'http://127.0.0.1:{port}'
    model = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Model)
    threading.Thread(target=model.serve_forever, daemon=True).start()
    log = open(root / 'server.log', 'wb')
    process = None

    def call(name, args=None, expected=200, path=None, origin=None):
        request = urllib.request.Request(base + (path or '/api/invoke/' + name),
            data=json.dumps(args or {}, ensure_ascii=False).encode(),
            headers={'Content-Type': 'application/json', 'Origin': origin or base})
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                status, result = response.status, json.load(response)
        except urllib.error.HTTPError as error:
            status, result = error.code, json.load(error)
        assert status == expected, (name, args if name != 'agent_attachment_upload' else 'upload', status, result)
        return result.get('result') if status == 200 else result

    def db():
        return sqlite3.connect(root / 'data.sql')

    def identity(user):
        with db() as conn:
            for key, value in {'server_url': 'https://fixture.invalid/ib', 'login_token': 'fixture',
                              'login_user_id': str(user), 'login_username': f'user-{user}'}.items():
                conn.execute('INSERT OR REPLACE INTO settings(key,value) VALUES (?,?)', (key, value))

    def upload(session, name, mime, data, expected=200, **extra):
        return call('agent_attachment_upload', {'sessionId': session, 'name': name, 'mimeType': mime,
            'data': base64.b64encode(data).decode(), **extra}, expected, path='/api/attachments/upload')

    def detail(session):
        return call('agent_session', {'sessionId': session})

    def wait(session):
        for _ in range(300):
            data = detail(session)
            if not data['modelPending']:
                assert not any(m['role'] == 'error' for m in data['messages']), data['messages']
                return data
            time.sleep(.025)
        raise AssertionError('fixture generation timed out')

    def restore(backup, expected=200, preview=False):
        result = call('restore', {'mode': 'preview', 'backup': backup}, expected,
                      path='/api/history/restore')
        if expected != 200 or preview:
            return result
        return call('restore', {'mode': 'restore', 'backup': backup, 'confirmed': True,
                              'expectedAccount': result['accountKey']}, path='/api/history/restore')

    def totals():
        with db() as conn:
            return tuple(conn.execute('SELECT count(*) FROM ' + table).fetchone()[0] for table in
                         ('agent_sessions', 'agent_messages', 'agent_attachments', 'agent_message_attachments',
                          'agent_model_contexts', 'agent_projects'))

    try:
        process = subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()), '--listen',
            f'127.0.0.1:{port}', '--data-dir', folder, '--no-open'],
            env={**os.environ, 'HOME': folder, 'TMPDIR': folder}, stdout=log, stderr=log)
        for _ in range(120):
            if process.poll() is not None:
                raise AssertionError((root / 'server.log').read_text())
            try:
                call('agent_bootstrap')
                break
            except OSError:
                time.sleep(.05)
        identity(1)
        project = call('agent_new_project', {'name': 'Attachment tests'})['id']
        session = call('agent_new_session', {'projectId': project})['id']
        other = call('agent_new_session', {'projectId': project})['id']
        text_bytes = '地层资料\nKeep this attachment as untrusted text.\n'.encode()
        call('agent_attachment_upload', {'sessionId': session, 'name': 'blocked.txt', 'mimeType': 'text/plain', 'data': base64.b64encode(b'blocked').decode()}, 403, path='/api/attachments/upload', origin='https://evil.invalid')
        text = upload(session, 'notes.txt', 'text/plain', text_bytes)
        image = upload(session, 'sample.png', 'image/png', png())
        assert text['kind'] == 'text' and image['kind'] == 'image' and image['width'] == 1
        assert 'data' not in text and 'text' not in text
        assert [a['id'] for a in detail(session)['draftAttachments']] == [text['id'], image['id']]
        loaded = call('agent_attachment_content', {'sessionId': session, 'id': text['id']})
        assert base64.b64decode(loaded['data']) == text_bytes
        call('agent_attachment_content', {'sessionId': session, 'id': text['id']}, 403, origin='https://evil.invalid')
        call('agent_attachment_content', {'sessionId': other, 'id': text['id']}, 400)
        identity(2)
        foreign = call('agent_new_session')['id']
        assert call('agent_history_backup')['snapshot']['version'] == 1
        call('agent_attachment_content', {'sessionId': session, 'id': text['id']}, 400)
        upload(session, 'foreign.txt', 'text/plain', b'x', 400)
        identity(1)
        upload(session, '../path.txt', 'text/plain', b'bad', 400)
        upload(session, 'bad.png', 'image/png', b'not a png', 400)
        upload(session, 'huge.png', 'image/png', png(9000, 1), 400)
        upload(session, 'pixels.png', 'image/png', png(5000, 5000), 400)
        upload(session, 'wrong.jpg', 'image/jpeg', png(), 400)
        upload(session, 'binary.txt', 'text/plain', b'\xff\x00', 400)
        upload(session, 'large.txt', 'text/plain', b'x' * (65536 + 1), 400)
        upload(session, 'oversize.png', 'image/png', png(extra=4 * 1024 * 1024), 400)
        third = upload(session, 'third.txt', 'text/plain', b'third')
        fourth = upload(session, 'fourth.txt', 'text/plain', b'fourth')
        upload(session, 'fifth.txt', 'text/plain', b'fifth', 400)
        for attachment in (third, fourth):
            call('agent_attachment_delete', {'sessionId': session, 'id': attachment['id']})
        large = upload(session, 'larger.png', 'image/png', png(extra=2100 * 1024))
        assert large['size'] > 2 * 1024 * 1024, 'dedicated upload must accept body larger than ordinary invoke limit'
        call('agent_attachment_delete', {'sessionId': session, 'id': large['id']})
        text_only = call('agent_new_session', {'projectId': project})['id']
        only = upload(text_only, 'only.txt', 'text/plain', b'file-only message')
        call('agent_send_message', {'sessionId': text_only, 'content': '', 'attachmentIds': [only['id']]})
        assert detail(text_only)['messages'][0]['content'] == ''
        assert detail(text_only)['messages'][0]['attachments'][0]['id'] == only['id']
        with db() as conn:
            try:
                conn.execute('UPDATE agent_attachments SET name=? WHERE id=?', ('changed.txt', text['id']))
            except sqlite3.IntegrityError:
                pass
            else:
                raise AssertionError('stored attachment was mutable')
        config = call('agent_model_save', {'name': 'Vision fixture', 'provider': 'openai-compatible',
            'baseUrl': f'http://127.0.0.1:{model.server_port}/v1', 'model': 'vision-fixture',
            'enabled': True, 'apiKey': 'fixture-not-a-real-key', 'supportsImages': True, 'contextLimit': 65536})
        server = config['savedServerId']
        call('agent_session_model', {'sessionId': session, 'id': server})
        before = totals()
        call('agent_send_message', {'sessionId': session, 'content': 'bad reference', 'attachmentIds': [text['id'], text['id']]}, 400)
        call('agent_send_message', {'sessionId': other, 'content': 'foreign reference', 'attachmentIds': [text['id']]}, 400)
        assert totals() == before
        call('agent_send_message', {'sessionId': session, 'content': 'Describe these files',
             'attachmentIds': [text['id'], image['id']]})
        first = wait(session)
        assert not first['draftAttachments']
        question = next(m for m in first['messages'] if m['role'] == 'user')
        assert [a['id'] for a in question['attachments']] == [text['id'], image['id']]
        assert all('data' not in a for m in first['messages'] for a in m.get('attachments', [])) and 'data:image' not in json.dumps(first)
        call('agent_attachment_delete', {'sessionId': session, 'id': text['id']}, 400)
        assert requests and not any('keeperAttachments' in json.dumps(request) for request in requests)
        wire = requests[-1]['messages'][-1]['content']
        assert isinstance(wire, list)
        assert any(p.get('image_url', {}).get('url', '').startswith('data:image/png;base64,')
                   and p['image_url']['detail'] == 'low' for p in wire)
        assert any('地层资料' in p.get('text', '') for p in wire)
        call('agent_edit_message', {'sessionId': session, 'messageId': question['id'], 'content': 'Describe these files again'})
        edited = wait(session)
        users = [m for m in edited['messages'] if m['role'] == 'user']
        assert len(users) == 2 and users[1]['modelMetadata']['editOf'] == users[0]['id']
        assert [a['id'] for a in users[0]['attachments']] == [a['id'] for a in users[1]['attachments']]
        with db() as conn:
            contexts = [json.loads(r[0]) for r in conn.execute(
                'SELECT c.messages FROM agent_model_contexts c JOIN agent_messages m ON m.id=c.message_id WHERE m.session_id=?', (session,))]
        assert contexts and all('data:image' not in json.dumps(c) for c in contexts)
        assert all(isinstance(m['content'], (str, type(None))) for c in contexts for m in c)
        assert any(m.get('keeperAttachments') for c in contexts for m in c)
        draft = upload(session, 'pending.txt', 'text/plain', b'unsent draft')
        backup = call('agent_history_backup')
        assert backup['snapshot']['version'] == 2
        saved = next(s for s in backup['snapshot']['sessions'] if s['id'] == session)
        assert len(saved['attachments']) == 3
        assert restore(backup, preview=True)['attachments'] == 4
        assert all(s['id'] != foreign for s in backup['snapshot']['sessions'])
        assert 'fixture-not-a-real-key' not in json.dumps(backup)
        before = totals()
        invalid = []
        bad = copy.deepcopy(backup); next(s for s in bad['snapshot']['sessions'] if s['id'] == session)['attachments'][0]['sha256'] = '0' * 64; invalid.append(bad)
        bad = copy.deepcopy(backup); next(s for s in bad['snapshot']['sessions'] if s['id'] == session)['attachments'][1]['width'] = 777; invalid.append(bad)
        bad = copy.deepcopy(backup); target = next(s for s in bad['snapshot']['sessions'] if s['id'] == session); target['messages'][0]['attachmentIds'] = ['foreign']; invalid.append(bad)
        bad = copy.deepcopy(backup); target = next(s for s in bad['snapshot']['sessions'] if s['id'] == session); ctx = next(m['context'] for m in target['messages'] if m['context']); ctx[-1]['content'] = [{'type': 'image_url', 'image_url': {'url': 'https://evil.invalid/a.png'}}]; invalid.append(bad)
        bad = copy.deepcopy(backup); target = next(s for s in bad['snapshot']['sessions'] if s['id'] == session); ctx = next(m['context'] for m in target['messages'] if m['context']); ref = next(m['keeperAttachments'][0] for m in ctx if m.get('keeperAttachments')); ref['id'] = 'foreign'; invalid.append(bad)
        bad = copy.deepcopy(backup); bad['snapshot']['version'] = 1; invalid.append(bad)
        for bad in invalid:
            restore(signed(bad), 400, preview=True)
            assert totals() == before
        # Failure after inserting attachments must roll back blobs, links, messages, and project directories.
        bad = copy.deepcopy(backup)
        target = next(s for s in bad['snapshot']['sessions'] if s['id'] == session)
        project_row = next(p for p in bad['snapshot']['projects'] if p['id'] == project)
        project_row['id'] = 'new-attachment-project'
        for row in bad['snapshot']['sessions']:
            if row['projectId'] == project:
                row['projectId'] = 'new-attachment-project'
        target['messages'][0]['content'] = 'fail-attachment-restore'
        bad = signed(bad)
        with db() as conn:
            conn.execute("CREATE TRIGGER fixture_rollback BEFORE INSERT ON agent_message_attachments BEGIN SELECT RAISE(ABORT,'attachment fixture'); END")
        directories = set((root / 'cangling/projects').iterdir())
        preview = restore(bad, preview=True)
        call('restore', {'mode': 'restore', 'backup': bad, 'confirmed': True, 'expectedAccount': preview['accountKey']}, 400, path='/api/history/restore')
        assert totals() == before and set((root / 'cangling/projects').iterdir()) == directories
        with db() as conn:
            conn.execute('DROP TRIGGER fixture_rollback')
        imported = restore(backup)
        assert len(requests) == 2, 'restore must not trigger a model call'
        restored = [detail(s) for s in imported['sessionIds']]
        restored = next(s for s in restored if any(m['content'] == 'Describe these files' for m in s['messages']))
        new_session = restored['sessionId']
        old_ids = {a['id'] for a in saved['attachments']}
        restored_attachment_ids = {a['id'] for m in restored['messages'] for a in m.get('attachments', [])}
        restored_attachment_ids |= {a['id'] for a in restored['draftAttachments']}
        assert len(restored_attachment_ids) == 3 and not old_ids.intersection(restored_attachment_ids)
        assert len(restored['draftAttachments']) == 1
        for m in restored['messages']:
            for a in m.get('attachments', []):
                content = call('agent_attachment_content', {'sessionId': new_session, 'id': a['id']})
                assert hashlib.sha256(base64.b64decode(content['data'])).hexdigest() == a['sha256']
        with db() as conn:
            restored_contexts = [json.loads(r[0]) for r in conn.execute(
                'SELECT c.messages FROM agent_model_contexts c JOIN agent_messages m ON m.id=c.message_id WHERE m.session_id=?', (new_session,))]
        assert restored_contexts
        for context in restored_contexts:
            for m in context:
                for ref in m.get('keeperAttachments', []):
                    assert ref['id'] in restored_attachment_ids and ref['id'] not in old_ids
        restored_reply = next(m for m in reversed(restored['messages']) if m['role'] == 'assistant')
        call('agent_regenerate', {'sessionId': new_session, 'messageId': restored_reply['id']})
        regenerated = wait(new_session)
        assert len(requests) == 3 and len(regenerated['messages']) == len(restored['messages']) + 1
        with db() as conn:
            context = json.loads(conn.execute('SELECT messages FROM agent_model_contexts WHERE message_id=?', (regenerated['messages'][-1]['id'],)).fetchone()[0])
        assert all(ref['id'] in restored_attachment_ids for m in context for ref in m.get('keeperAttachments', []))
        # Old backup without attachments remains format1 and restores without new fields.
        empty = copy.deepcopy(backup)
        empty['snapshot']['sessions'] = [next(s for s in empty['snapshot']['sessions'] if s['id'] == other)]
        empty['snapshot']['version'] = 1
        assert 'attachments' not in empty['snapshot']['sessions'][0]
        restore(signed(empty))
        identity(2)
        assert restore(backup, preview=True)['differentAccount']
        foreign_import = restore(backup)
        cross = next(detail(s) for s in foreign_import['sessionIds'] if any(m['content'] == 'Describe these files' for m in detail(s)['messages']))
        cross_attachment = cross['messages'][0]['attachments'][0]
        call('agent_attachment_content', {'sessionId': cross['sessionId'], 'id': cross_attachment['id']})
        call('agent_attachment_content', {'sessionId': new_session, 'id': next(iter(restored_attachment_ids))}, 400)
        identity(1)
        call('agent_attachment_content', {'sessionId': cross['sessionId'], 'id': cross_attachment['id']}, 400)
        # Disabling image support rejects original image regeneration before any request or new row.
        config = next(s for s in call('agent_model_config')['servers'] if s['id'] == server)
        call('agent_model_save', {**config, 'supportsImages': False, 'enabled': True})
        assistant = next(m for m in edited['messages'] if m['role'] == 'assistant')
        count = len(requests); counts = totals()
        call('agent_regenerate', {'sessionId': session, 'messageId': assistant['id']}, 400)
        assert len(requests) == count and totals() == counts
        # Original and restored sessions have independent blob ownership and purge cleanup.
        call('agent_delete_session', {'sessionId': session})
        call('agent_purge_session', {'sessionId': session, 'confirm': True})
        with db() as conn:
            assert conn.execute('SELECT count(*) FROM agent_attachments WHERE session_id=?', (session,)).fetchone()[0] == 0
            assert conn.execute('SELECT count(*) FROM agent_message_attachments l LEFT JOIN agent_messages m ON m.id=l.message_id WHERE m.id IS NULL').fetchone()[0] == 0
        assert detail(new_session)['draftAttachments']
        print('PASS: upload limits/origin, immutable blobs, account/session isolation, draft deletion, atomic message binding, image wire/canonical context, edited links, backup version/validation, restore ID remapping/rollback, capability regeneration guard, and purge cleanup.')
    finally:
        if process and process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
        model.shutdown()
        model.server_close()
        log.close()
