#!/usr/bin/env python3
"""Real Keeper loop with a hostile local provider; disposable HOME/DB/project."""
import http.server, json, os, socket, sqlite3, subprocess, tempfile, threading, time, urllib.request
from pathlib import Path

requests = []
scopes = []
CASES = {'simple': 'known_operation', 'unsolicited': 'known_operation', 'complex': 'investigation',
         'ignored': 'development', 'upgrade': 'known_operation', 'invalid': 'known_operation'}
class Model(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_): pass
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        route = body['tools'][0]['function']['name'] == 'assess_task_scope'
        if route:
            prompt = json.loads(body['messages'][-1]['content'])['currentRequest']
            scopes.append(prompt)
            assert body['temperature'] == 0
            assert body['tool_choice']['function']['name'] == 'assess_task_scope'
        else:
            prompt = next(m['content'] for m in reversed(body['messages']) if m['role'] == 'user' and not m['content'].startswith('运行时'))
            requests.append((prompt, body))
        results = [m for m in body['messages'] if m['role'] == 'tool'] if not route else []
        names = {t['function']['name'] for t in body['tools']}
        def tool(name, args):
            return {'tool_calls': [{'index': 0, 'id': f'call-{len(results)}', 'type': 'function', 'function': {'name': name, 'arguments': json.dumps(args)}}]}
        plan = {'plan': [{'step': '定位原因并验证', 'status': 'in_progress'}]}
        if route:
            delta = tool('assess_task_scope', {'category': CASES[prompt], 'explicitPlan': False, 'reason': 'Isolated test scope'})
            if prompt == 'invalid': delta = {'content': 'pretend classified'}
        elif prompt == 'ignored':
            assert names == {'update_plan'}
            delta = {'content': '未建计划也想结束。'}
        elif prompt == 'complex':
            if not results:
                assert names == {'update_plan'}
                # Deliberately disregard forced tool_choice. Runtime must block it.
                delta = tool('write_file', {'path': 'must-not-exist.txt', 'content': 'bad', 'createOnly': True})
            elif len(results) == 1:
                assert json.loads(results[-1]['content'])['notExecuted'] is True
                assert names == {'update_plan'}
                delta = tool('update_plan', plan)
            elif len(results) == 2:
                assert 'list_directory' in names
                delta = tool('list_directory', {'path': '.'})
            elif len(results) == 3:
                delta = tool('update_plan', {'plan': [{'step': '定位原因并验证', 'status': 'completed'}]})
            else: delta = {'content': '已核对目录；无其他改动。'}
        elif prompt == 'unsolicited':
            assert 'update_plan' not in names
            if not results: delta = tool('update_plan', plan)
            elif len(results) == 1:
                assert json.loads(results[-1]['content'])['notExecuted'] is True
                delta = tool('list_directory', {'path': '.'})
            else: delta = {'content': '已查看目录。'}
        elif prompt == 'upgrade':
            if not results: delta = tool('list_directory', {'path': '.'})
            elif len(results) == 1:
                delta = tool('expand_task_scope', {'evidenceToolCallId': results[0]['tool_call_id'], 'reason': '实际检查表明需排查目录内容'})
            elif len(results) == 2:
                assert names == {'update_plan'}
                delta = tool('update_plan', plan)
            elif len(results) == 3: delta = tool('list_directory', {'path': '.'})
            elif len(results) == 4: delta = tool('update_plan', {'plan': [{'step': '定位原因并验证', 'status': 'completed'}]})
            else: delta = {'content': '已核对目录。'}
        else:
            assert 'update_plan' not in names
            delta = tool('list_directory', {'path': '.'}) if not results else {'content': '已查看目录。'}
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()
        packet = {'choices': [{'delta': delta, 'finish_reason': 'tool_calls' if 'tool_calls' in delta else 'stop'}]}
        self.wfile.write(('data: ' + json.dumps(packet) + '\n\ndata: [DONE]\n\n').encode())

api = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Model)
threading.Thread(target=api.serve_forever, daemon=True).start()
with tempfile.TemporaryDirectory(prefix='keeper-plan-policy-') as folder:
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0)); port = sock.getsockname()[1]
    base = f'http://127.0.0.1:{port}'
    def call(name, args=None):
        req = urllib.request.Request(base + '/api/invoke/' + name, data=json.dumps(args or {}).encode(), headers={'Content-Type': 'application/json', 'Origin': base})
        with urllib.request.urlopen(req, timeout=10) as r:
            data = json.load(r)
        assert data['ok'], data
        return data['result']
    def wait(session):
        for _ in range(300):
            detail = call('agent_session', {'sessionId': session})
            if not detail['modelPending']: return detail
            time.sleep(.025)
        raise AssertionError('generation timeout')
    log = open(Path(folder) / 'server.log', 'w')
    process = subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()), '--listen', f'127.0.0.1:{port}', '--data-dir', folder, '--no-open'], env={**os.environ, 'HOME': folder, 'TMPDIR': folder}, stdout=log, stderr=log)
    try:
        for _ in range(100):
            try: call('agent_bootstrap'); break
            except Exception: time.sleep(.05)
        else: raise AssertionError('startup')
        call('agent_model_save', {'name': 'Scope fixture', 'provider': 'openai-compatible', 'enabled': True, 'baseUrl': f'http://127.0.0.1:{api.server_port}', 'model': 'fixture', 'apiKey': 'fixture-only'})
        sessions = {}
        for prompt in CASES:
            session = call('agent_new_session')['id']; sessions[prompt] = session
            call('agent_security_mode', {'sessionId': session, 'mode': 'full', 'confirmFullAccess': True})
            call('agent_send_message', {'sessionId': session, 'content': prompt})
            detail = wait(session)
            meta = detail['messages'][-1]['modelMetadata']
            if prompt in ('invalid', 'ignored'):
                assert not detail['runs'], detail
                assert detail['tasks'][-1]['status'] == 'failed', detail
            elif prompt in ('simple', 'unsolicited'):
                assert meta['planPolicy']['mode'] == 'hidden', meta
                assert not any(r['tool'] == 'update_plan' and r['status'] == 'succeeded' for r in detail['runs'])
            else:
                assert meta['planPolicy']['mode'] == 'required', meta
                assert any(r['tool'] == 'update_plan' and r['status'] == 'succeeded' for r in detail['runs']), detail
                if prompt == 'upgrade': assert meta['planPolicy']['upgraded'] is True
        assert not list(Path(folder).rglob('must-not-exist.txt'))
        # Regeneration reuses the saved policy instead of reclassifying.
        session = sessions['simple']; detail = call('agent_session', {'sessionId': session})
        before = len(scopes)
        call('agent_regenerate', {'sessionId': session, 'messageId': detail['messages'][-1]['id']})
        detail = wait(session)
        assert len(scopes) == before and detail['messages'][-1]['modelMetadata']['planPolicy']['mode'] == 'hidden'
        # Persisted policy remains unchanged after an actual service restart.
        with sqlite3.connect(Path(folder) / 'data.sql') as db:
            assert db.execute("SELECT count(*) FROM agent_messages WHERE json_extract(model_metadata,'$.planPolicy.mode')='required'").fetchone()[0] >= 3
        process.terminate(); process.wait(timeout=10)
        process = subprocess.Popen([str(Path('target/debug/cangling-keeper').resolve()), '--listen', f'127.0.0.1:{port}', '--data-dir', folder, '--no-open'], env={**os.environ, 'HOME': folder, 'TMPDIR': folder}, stdout=log, stderr=log)
        for _ in range(100):
            try:
                restored = call('agent_session', {'sessionId': sessions['upgrade']})
                break
            except Exception: time.sleep(.05)
        else: raise AssertionError('restart')
        assert restored['messages'][-1]['modelMetadata']['planPolicy']['mode'] == 'required'
        assert restored['messages'][-1]['modelMetadata']['planPolicy']['upgraded'] is True
        print('PASS scope preflight, simple tool filtering, unsolicited plan suppression, mandatory-plan bypass blocking, bounded refusal, scope expansion, regeneration snapshot and persisted policy')
    finally:
        process.terminate(); process.wait(timeout=10); log.close(); api.shutdown()
