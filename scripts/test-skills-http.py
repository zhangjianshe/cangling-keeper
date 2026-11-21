#!/usr/bin/env python3
"""Project guides and lazy Skills against a disposable Keeper/model. No real keys or calls."""
import http.server
import json
import os
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

requests = []
errors = []
change_during_turn = False
RULE_ONE = 'RULES_FIXTURE_ONE: use only reviewed project changes.'
RULE_TWO = 'RULES_FIXTURE_TWO: preserve the current project convention.'
BODY = 'SKILL_BODY_LAZY_SENTINEL. Write notes using write_file; wait for normal user approval.\n'


class Model(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        global change_during_turn
        try:
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append(body)
            assert self.headers['Authorization'] == 'Bearer fixture-only-key'
            assert body['stream'], 'small fixtures should not require a summary call'
            prompt = next(m['content'] for m in reversed(body['messages']) if m['role'] == 'user')
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()

            def event(delta, reason=None):
                value = {'model': 'skills-fixture', 'choices': [{'delta': delta, 'finish_reason': reason}]}
                self.wfile.write(('data: ' + json.dumps(value, ensure_ascii=False) + '\n\n').encode())
                self.wfile.flush()

            def tool(name, arguments):
                event({'tool_calls': [{'index': 0, 'id': f'fixture-{len(requests)}', 'type': 'function',
                                      'function': {'name': name, 'arguments': json.dumps(arguments)}}]}, 'tool_calls')

            if body['messages'][-1]['role'] != 'tool':
                if change_during_turn and prompt == 'use-skill':
                    guide.write_text('MID_TURN_RULE_SENTINEL')
                    change_during_turn = False
                tool('load_skill', {'name': 'write-notes'})
            else:
                last = body['messages'][-1]
                result = json.loads(last['content'])
                called = next(c['function']['name'] for m in reversed(body['messages'])
                              for c in m.get('tool_calls', []) if c['id'] == last['tool_call_id'])
                if called == 'load_skill':
                    assert BODY.strip() in result['instructions'], result
                    if 'write-after-skill' in prompt:
                        tool('write_file', {'path': 'guide-stopped.txt' if 'stop' in prompt else 'guide-generated.txt', 'content': 'reviewed skill output\n'})
                    else:
                        event({'content': 'Loaded project skill using current project guidance.'}, 'stop')
                else:
                    assert called == 'write_file'
                    event({'content': 'The reviewed operation result has been retained.'}, 'stop')
            self.wfile.write(b'data: [DONE]\n\n')
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass
        except Exception as error:
            errors.append(repr(error))
            try:
                self.send_response(500)
                self.end_headers()
            except OSError:
                pass


api = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Model)
threading.Thread(target=api.serve_forever, daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-skills-http-') as folder:
    data = Path(folder)
    project = data / 'project'
    project.mkdir()
    other_project = data / 'other-project'
    other_project.mkdir()
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    base = f'http://127.0.0.1:{port}'
    process = None
    log = open(data / 'server.log', 'wb')

    def db():
        return sqlite3.connect(data / 'data.sql')

    def call(name, args=None, expected=200, origin=None):
        request = urllib.request.Request(base + '/api/invoke/' + name,
            data=json.dumps(args or {}, ensure_ascii=False).encode(),
            headers={'Content-Type': 'application/json', 'Origin': origin or base})
        try:
            with urllib.request.urlopen(request, timeout=15) as response:
                status, value = response.status, json.load(response)
        except urllib.error.HTTPError as error:
            status, value = error.code, json.load(error)
        assert status == expected, (name, status, value)
        return value.get('result') if status == 200 else value

    def start():
        global process
        process = subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()), '--listen',
            f'127.0.0.1:{port}', '--data-dir', folder, '--no-open'],
            env={**os.environ, 'TMPDIR': folder}, stdout=log, stderr=log)
        for _ in range(150):
            if process.poll() is not None:
                raise AssertionError((data / 'server.log').read_text())
            try:
                call('agent_bootstrap')
                return
            except (OSError, AssertionError):
                time.sleep(.04)
        raise AssertionError('Keeper did not start')

    def stop():
        global process
        if process and process.poll() is None:
            process.terminate()
            process.wait(timeout=15)
        process = None

    def identity(user):
        with db() as conn:
            for key, value in {'server_url': f'http://127.0.0.1:{api.server_port}/ib', 'login_token': 'fixture-token',
                               'login_user_id': str(user), 'login_username': f'user-{user}'}.items():
                conn.execute('INSERT OR REPLACE INTO settings(key,value) VALUES (?,?)', (key, value))

    def wait(session, predicate=lambda d: not d['modelPending']):
        for _ in range(400):
            detail = call('agent_session', {'sessionId': session})
            if predicate(detail):
                assert not errors, errors
                return detail
            time.sleep(.025)
        raise AssertionError(('request timeout', detail, errors))

    def new_session(project_id='general'):
        return call('agent_new_session', {'projectId': project_id})['id']

    def skill(root, name, frontmatter, body):
        directory = root / '.agents' / 'skills' / name
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / 'SKILL.md'
        path.write_text('---\n' + frontmatter + '\n---\n' + body)
        return path

    try:
        start()
        # Redirect only this disposable database's projects; do not replace HOME or alter real files.
        with db() as conn:
            conn.execute("UPDATE agent_projects SET directory=? WHERE id='general'", (str(project),))
            conn.execute("INSERT INTO agent_projects(id,name,directory,created_at) VALUES ('other','Other',?,1)", (str(other_project),))
        identity(1)
        guide = project / 'AGENTS.md'
        guide.write_text(RULE_ONE)
        (other_project / 'AGENTS.md').write_text('OTHER_PROJECT_RULE_SENTINEL')
        notes = skill(project, 'write-notes', 'name: write-notes\ndescription: >-\n  用于写入项目说明。\n  Preserve reviewed file changes.\nallowed-tools: write_file run_command', BODY)
        long_body = ('中文分页说明\n' * 900) + 'END_OF_PAGED_SKILL'
        skill(project, 'large-notes', 'name: large-notes\ndescription: "Large paged skill"', long_body)
        skill(project, 'invalid-name', 'name: mismatched-directory\ndescription: invalid', 'INVALID_SKILL_BODY')
        skill(project, 'bad-yaml', 'name: [broken\ndescription: invalid', 'INVALID_YAML_BODY')
        skill(other_project, 'other-skill', 'name: other-skill\ndescription: Other project only', 'OTHER_PROJECT_SKILL_SENTINEL')
        nested = project / 'nested'
        nested.mkdir()
        (nested / 'AGENTS.md').write_text('NESTED_RULE_MUST_NOT_BE_GLOBAL')
        s = new_session()
        other = new_session('other')
        snapshot = call('agent_project_guidance', {'sessionId': s})
        assert snapshot['rules']['status'] == 'loaded' and RULE_ONE in snapshot['rules']['content']
        assert {x['name'] for x in snapshot['skills']} == {'write-notes', 'large-notes'}
        assert '用于写入项目说明' in next(x['description'] for x in snapshot['skills'] if x['name'] == 'write-notes')
        assert BODY.strip() not in json.dumps(snapshot, ensure_ascii=False), 'catalog must not eagerly expose body'
        assert snapshot['warnings'], 'invalid skills should have visible warnings'
        assert 'NESTED_RULE_MUST_NOT_BE_GLOBAL' not in json.dumps(snapshot)
        second = call('agent_project_guidance', {'sessionId': other})
        assert second['rules']['content'] == 'OTHER_PROJECT_RULE_SENTINEL'
        assert [x['name'] for x in second['skills']] == ['other-skill']
        call('agent_skill_content', {'sessionId': other, 'name': 'write-notes'}, 400)
        call('agent_project_guidance', {'sessionId': s}, 403, origin='https://untrusted.invalid')
        for name in ('../write-notes', '/write-notes', 'invalid-name', 'bad-yaml'):
            call('agent_skill_content', {'sessionId': s, 'name': name}, 400)
        page = call('agent_skill_content', {'sessionId': s, 'name': 'large-notes'})
        body, next_offset, digest = page['instructions'], page['nextOffset'], page['sha256']
        assert len(body.encode()) <= 2200 and next_offset is not None
        while next_offset is not None:
            page = call('agent_skill_content', {'sessionId': s, 'name': 'large-notes',
                'offset': next_offset, 'expectedSha256': digest})
            assert len(page['instructions'].encode()) <= 2200
            body += page['instructions']
            next_offset = page['nextOffset']
        assert body == long_body, 'paged content must be complete without overlap or UTF-8 splitting'
        call('agent_skill_content', {'sessionId': s, 'name': 'large-notes', 'offset': -1}, 400)
        call('agent_skill_content', {'sessionId': s, 'name': 'large-notes', 'expectedSha256': '0' * 64}, 400)
        original_page = call('agent_skill_content', {'sessionId': s, 'name': 'write-notes'})
        notes.write_text(notes.read_text() + 'changed\n')
        call('agent_skill_content', {'sessionId': s, 'name': 'write-notes',
            'expectedSha256': original_page['sha256']}, 400)
        notes.write_text(notes.read_text().removesuffix('changed\n'))
        identity(2)
        call('agent_project_guidance', {'sessionId': s}, 400)
        call('agent_skill_content', {'sessionId': s, 'name': 'write-notes'}, 400)
        identity(1)
        call('agent_model_save', {'name': 'Skills fixture', 'provider': 'openai-compatible', 'enabled': True,
            'baseUrl': f'http://127.0.0.1:{api.server_port}', 'model': 'fixture', 'apiKey': 'fixture-only-key'})
        # Existing sessions deliberately retained their local-only selection; new ones use the fixture.
        s = new_session()
        change_during_turn = True
        call('agent_send_message', {'sessionId': s, 'content': 'use-skill'})
        detail = wait(s)
        assert detail['runs'][0]['tool'] == 'load_skill' and detail['runs'][0]['status'] == 'succeeded'
        assert detail['runs'][0]['review'] is None, 'loading is read-only and must not grant approvals'
        assert BODY.strip() in json.loads(detail['runs'][0]['output'])['instructions']
        first = requests[0]
        system = first['messages'][0]['content']
        assert RULE_ONE in system and 'write-notes' in system and '用于写入项目说明' in system
        assert 'NESTED_RULE_MUST_NOT_BE_GLOBAL' not in system
        assert BODY.strip() not in system and BODY.strip() not in json.dumps(first, ensure_ascii=False)
        assert any(t['function']['name'] == 'load_skill' for t in first['tools'])
        assert BODY.strip() in json.dumps(requests[1], ensure_ascii=False)
        assert RULE_ONE in requests[1]['messages'][0]['content']
        assert 'MID_TURN_RULE_SENTINEL' not in requests[1]['messages'][0]['content'], 'one ReAct loop must retain its initial rule snapshot'
        assert detail['messages'][-1]['modelMetadata']['context']['estimatedTokens'] + 6144 <= 32768
        answer = detail['messages'][-1]['id']
        guide.write_text(RULE_TWO)
        before = len(requests)
        call('agent_regenerate', {'sessionId': s, 'messageId': answer})
        wait(s)
        assert RULE_TWO in requests[before]['messages'][0]['content']
        assert RULE_ONE not in requests[before]['messages'][0]['content']
        pending_session = new_session()
        call('agent_send_message', {'sessionId': pending_session, 'content': 'write-after-skill'})
        pending = wait(pending_session, lambda d: bool(d['runs']) and d['runs'][-1]['status'] == 'pending_approval')
        assert pending['runs'][0]['tool'] == 'load_skill' and pending['runs'][0]['status'] == 'succeeded'
        assert pending['runs'][-1]['tool'] == 'write_file'
        assert not (project / 'guide-generated.txt').exists(), 'skill allowed-tools must not bypass approvals'
        run = pending['runs'][-1]
        call('agent_review_tool', {'sessionId': pending_session, 'runId': run['id'],
            'reviewHash': run['review']['hash'], 'approved': True})
        completed = wait(pending_session)
        assert (project / 'guide-generated.txt').read_text() == 'reviewed skill output\n'
        cancel_session = new_session()
        call('agent_send_message', {'sessionId': cancel_session, 'content': 'write-after-skill-stop'})
        pending = wait(cancel_session, lambda d: bool(d['runs']) and d['runs'][-1]['status'] == 'pending_approval')
        call('agent_stop_generation', {'sessionId': cancel_session, 'messageId': pending['streamingMessageId']})
        stopped = wait(cancel_session)
        assert stopped['runs'][0]['status'] == 'succeeded' and stopped['runs'][-1]['status'] == 'stopped'
        assert not (project / 'guide-stopped.txt').exists()
        before = len(requests)
        stop()
        start()
        resumed = call('agent_session', {'sessionId': cancel_session})
        assert not resumed['modelPending'] and resumed['runs'][0]['output'] == stopped['runs'][0]['output']
        assert len(requests) == before, 'restart must not replay skill/tool/model calls'
        # A long root is never silently truncated into a misleading partial instruction.
        guide.write_text('LONG_ROOT_SENTINEL ' + 'x' * 5000)
        oversized = call('agent_project_guidance', {'sessionId': s})
        assert oversized['rules']['status'] == 'too_large' and not oversized['rules'].get('content')
        if os.name == 'posix':
            guide.unlink()
            guide.symlink_to(other_project / 'AGENTS.md')
            unsafe = call('agent_project_guidance', {'sessionId': s})
            assert unsafe['rules']['status'] == 'invalid'
            assert 'OTHER_PROJECT_RULE_SENTINEL' not in json.dumps(unsafe)
            guide.unlink()
            guide.write_text(RULE_TWO)
            link = project / '.agents' / 'skills' / 'linked'
            link.symlink_to(other_project / '.agents' / 'skills' / 'other-skill', target_is_directory=True)
            assert not any(x['name'] == 'linked' for x in call('agent_project_guidance', {'sessionId': s})['skills'])
            call('agent_skill_content', {'sessionId': s, 'name': 'linked'}, 400)
        assert not errors, errors
        print('PASS: project root rules and current regeneration guidance; lazy skill metadata/body separation; '
              'YAML and path validation; UTF-8 paging and changed-hash protection; project/account/origin isolation; '
              'unchanged write approvals; stop and restart without replay; oversized/symlink guidance handling.')
        if '--preview' in sys.argv:
            print('Skills preview:', base, 'session:', s, flush=True)
            input('Press Enter after browser verification: ')
    finally:
        stop()
        log.close()
        api.shutdown()
        api.server_close()
