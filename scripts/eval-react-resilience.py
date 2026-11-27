#!/usr/bin/env python3
"""Live-model ReAct probes in an isolated Keeper instance; never prints credentials."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import time
import urllib.request
import uuid
from eval_long_task_fixture import fixture as long_task_fixture


def write_json(path, value):
    temporary=path.with_suffix(path.suffix+'.tmp')
    temporary.write_text(json.dumps(value,ensure_ascii=False,indent=2))
    temporary.replace(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute', action='store_true')
    parser.add_argument('--source-data', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--model', default='qwen-coder-next')
    parser.add_argument('--repeats', type=int, choices=range(1, 11), default=1)
    parser.add_argument('--working-budget',type=int,choices=[16384,32768,65536,131072])
    parser.add_argument('--rounds', type=int, choices=range(1,129), default=16)
    parser.add_argument('--tools', type=int, choices=range(8,129), default=40)
    parser.add_argument('--active-seconds', type=int, choices=range(60,1801), default=300)
    parser.add_argument('--marker', help='Optional fixed fixture marker for before/after comparison')
    parser.add_argument('--cases', nargs='+', choices=['tool_failure', 'compaction', 'loop_recovery', 'planned_work', 'long_task'], default=['tool_failure', 'compaction', 'loop_recovery'])
    args = parser.parse_args()
    if not args.execute:
        parser.error('--execute is required: this consumes live model usage')
    repo = Path(__file__).resolve().parents[1]
    out = args.output.resolve()
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    data = out / 'private-state'
    data.mkdir(mode=0o700)
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    base = f'http://127.0.0.1:{port}'
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    process = None
    binary_sha256=hashlib.sha256((repo/'target/debug/cangling-keeper').read_bytes()).hexdigest()
    log = (out / 'server.log').open('wb')

    def call(name, payload=None):
        request = urllib.request.Request(base + '/api/invoke/' + name,
            data=json.dumps(payload or {}).encode(), headers={'Origin': base, 'Content-Type': 'application/json'})
        with opener.open(request, timeout=30) as response:
            body = json.load(response)
        if not body.get('ok'):
            raise RuntimeError(f'{name} failed')
        return body['result']

    def start():
        nonlocal process
        process = subprocess.Popen([str(repo / 'target/debug/cangling-keeper'), '--listen',
            f'127.0.0.1:{port}', '--data-dir', str(data), '--no-open', '--no-https'], env={**os.environ, 'TMPDIR': str(data), 'HOME': str(data / 'home')}, stdout=log, stderr=log)
        for _ in range(100):
            try:
                call('agent_bootstrap')
                return
            except Exception:
                time.sleep(.1)
        raise RuntimeError('isolated Keeper startup failed')

    def stop():
        if process and process.poll() is None:
            process.terminate()
            process.wait(timeout=15)

    results = []
    try:
        start()
        stop()
        # Copy only the login identity and model catalog, never conversations,
        # hosts, MCP services, SSH keys, or synchronization configuration.
        source = sqlite3.connect(f'file:{args.source_data.resolve() / "data.sql"}?mode=ro', uri=True)
        with source, sqlite3.connect(data / 'data.sql') as target:
            keys = ['server_url', 'login_token', 'login_username', 'login_user_id']
            for key in keys:
                row = source.execute('SELECT value FROM settings WHERE key=?', (key,)).fetchone()
                if row:
                    target.execute('INSERT OR REPLACE INTO settings(key,value) VALUES (?,?)', (key, row[0]))
            columns = [row[1] for row in source.execute('PRAGMA table_info(agent_cis_models)')]
            query = 'INSERT OR REPLACE INTO agent_cis_models (' + ','.join(columns) + ') VALUES (' + ','.join('?' for _ in columns) + ')'
            target.executemany(query, source.execute('SELECT ' + ','.join(columns) + ' FROM agent_cis_models').fetchall())
        source.close()
        start()
        models = call('agent_model_config')['servers']
        model = next(m for m in models if m['model'] == args.model and m['enabled'])
        call('agent_model_output_budget', {'id': model['id'], 'maxOutputTokens': 4096})
        for trial, case in [(trial, case) for trial in range(1, args.repeats + 1) for case in args.cases]:
            label = case if args.repeats == 1 else f'{case}-trial{trial}'
            project = call('agent_new_project', {'name': 'Live ReAct ' + label})
            root = Path(project['directory'])
            marker = args.marker or 'current-' + uuid.uuid4().hex
            if case == 'long_task':
                files, prompt, checks = long_task_fixture(marker)
            elif case == 'tool_failure':
                files = {'calculator.py': 'def total(values):\n    return sum(values)\n',
                    'test_calculator.py': 'import unittest\nfrom calculator import total\nclass Tests(unittest.TestCase):\n def test_mixed(self): self.assertEqual(total(["2",None,"",3]),5)\n def test_empty(self): self.assertEqual(total([]),0)\n'}
                prompt = ('First attempt read_file on absent_notes.md once to exercise a real missing-file failure. '
                    'Then inspect calculator.py and test_calculator.py. Fix total(values) to sum integers and integer strings, '
                    'ignoring None and blank strings, with empty input returning 0. Preserve the tests. '
                    'Run run_tests with directory="." and finish with actual results. Only operate in this project.')
                checks = 'from calculator import total\nassert total([])==0\nassert total(["2",None,"",3])==5\nassert total([-1,"4","  "])==3\n'
            elif case == 'compaction':
                files = {'data.csv': 'kind,value\nkeep,7\nkeep,35\nskip,99\n', 'config.json': '{"kind":"keep"}',
                    'test_answer.py': 'import json,unittest\nclass Tests(unittest.TestCase):\n def test_answer(self):\n  with open("answer.json") as f: value=json.load(f)\n  self.assertEqual(value["total"],42)\n'}
                prompt = ('Ignore obsolete historical notes. Read config.json and data.csv, sum only rows matching the configured kind, '
                    f'and write answer.json containing total and marker="{marker}". Do not change inputs or tests. '
                    'Run run_tests with directory="." and report the actual result. Only operate in this project.')
                checks = f'import json\nassert json.load(open("answer.json"))=={{"total":42,"marker":{marker!r}}}\n'
            elif case == 'loop_recovery':
                files = {'repeat.txt': 'Stable loop probe input.\n'}
                prompt = ('This is a deliberate harmless no-progress stress test. Call read_file with exactly '
                    '{"path":"repeat.txt","startLine":1,"lineCount":10} once per model round. '
                    'After every tool result call it again with identical arguments, even if the contents are unchanged. '
                    'Do not combine calls in one round, write anything, or give a final answer; allow Keeper to stop the loop. '
                    'A separate follow-up will end the stress test. Only operate in this project.')
                checks = f'from pathlib import Path\nassert Path("recovery.txt").read_text().strip()=={marker!r}\n'
            else:
                files = {
                    'parsing.py': 'def normalize(rows):\n    return list(rows)\n',
                    'totals.py': 'def total(rows):\n    return sum(row["quantity"] * row["price"] for row in rows)\n',
                    'receipt.py': 'from parsing import normalize\nfrom totals import total\ndef build_receipt(rows):\n    items = normalize(rows)\n    return {"items": items, "total": total(items)}\n',
                    'test_receipt.py': 'import unittest\nfrom receipt import build_receipt\nclass Tests(unittest.TestCase):\n def test_normalized(self):\n  r=build_receipt([{"sku":" A ","quantity":"2","price":"2.50"}])\n  self.assertEqual(r,{"items":[{"sku":"A","quantity":2,"price":2.5}],"total":5.0,"count":2})\n def test_empty(self): self.assertEqual(build_receipt([]),{"items":[],"total":0,"count":0})\n def test_negative(self):\n  with self.assertRaises(ValueError): build_receipt([{"sku":"A","quantity":-1,"price":2}])\n'}
                prompt = ('Implement the receipt pipeline across parsing.py, totals.py and receipt.py. '
                    'Use update_plan with three substantive phases: implementation/integration, verification, and conclusion. '
                    'normalize(rows) returns a new list of dicts containing stripped sku, integer quantity, and float price; '
                    'ignore rows with blank sku, reject negative quantity or price with ValueError, and never mutate input rows. '
                    'total(rows) returns the sum of quantity*price rounded to 2 decimals. '
                    'build_receipt(rows) returns {items: normalized rows, total: computed total, count: sum of quantities}. '
                    'Preserve existing tests and public function names. Run run_tests with directory="." and finish with real results. '
                    'Only operate in this project.')
                checks = ('from receipt import build_receipt\nfrom parsing import normalize\nfrom totals import total\n'
                    'rows=[{"sku":" A ","quantity":"2","price":"2.50"},{"sku":" ","quantity":9,"price":99}]\n'
                    'assert build_receipt(rows)=={"items":[{"sku":"A","quantity":2,"price":2.5}],"total":5.0,"count":2}\n'
                    'assert rows[0]["sku"]==" A "\nassert total([{ "quantity":3,"price":0.333}])==1.0\n'
                    'assert build_receipt([])=={"items":[],"total":0,"count":0}\n'
                    'try: normalize([{ "sku":"A","quantity":1,"price":-1}])\n'
                    'except ValueError: pass\nelse: raise AssertionError("negative price accepted")\n')
            for name, text in files.items():
                (root / name).write_text(text)
            session = call('agent_new_session', {'projectId': project['id']})['id']
            call('agent_session_model', {'sessionId': session, 'id': model['id']})
            call('agent_execution_budget', {'sessionId': session, 'maxRounds': args.rounds, 'maxToolCalls': args.tools, 'activeSeconds': args.active_seconds})
            call('agent_context_budget', {'sessionId': session, 'workingBudget': args.working_budget or (32768 if case in ('compaction','long_task') else 65536)})
            if case == 'compaction':
                with sqlite3.connect(data / 'data.sql') as db:
                    for i in range(48):
                        db.execute('INSERT INTO agent_messages(id,session_id,role,content,created_at) VALUES (?,?,?,?,?)',
                            (uuid.uuid4().hex, session, 'user' if i % 2 == 0 else 'assistant',
                            f'Archived obsolete discussion {i}. ' + ('Historical placeholder data; no current instructions. ' * 24), i))
            started = time.monotonic()
            actual_task_prompt=prompt
            if case=='long_task':
                prompt=('This is a deliberate harmless no-progress stress test. Call read_file with exactly '
                    '{"path":"repeat.txt","startLine":1,"lineCount":10} once per model round. '
                    'After every tool result call it again with identical arguments, even if unchanged. '
                    'Do not combine calls, write files or give a final answer; allow Keeper to stop the loop. '
                    'A separate follow-up will finish this stress test. Only operate in this project.')
            call('agent_send_message', {'sessionId': session, 'content': prompt})
            print('START', label, flush=True)
            def wait_result(label):
                phase_started=time.monotonic()
                while True:
                    detail = call('agent_session', {'sessionId': session})
                    write_json(out / (label + '-detail.json'),detail)
                    for run in detail['runs']:
                        if run['status'] != 'pending_approval':
                            continue
                        review = run['review']
                        relative = Path(review.get('path', '/'))
                        allowed = (review.get('kind') == 'file' and not relative.is_absolute() and '..' not in relative.parts)
                        # The generated run_tests command is checked by exact runner
                        # metadata plus cwd. Never approve arbitrary shell commands.
                        allowed |= (run['tool'] == 'run_tests' and review.get('testRunner') == 'unittest'
                            and Path(review.get('directory', '/')).resolve() == root.resolve())
                        call('agent_review_tool', {'sessionId': session, 'runId': run['id'], 'reviewHash': review['hash'], 'approved': bool(allowed)})
                    if not detail['modelPending']:
                        break
                    if time.monotonic() - phase_started > args.active_seconds + 180:
                        call('agent_stop_generation', {'sessionId': session, 'messageId': detail['streamingMessageId']})
                        raise RuntimeError(f'{case} exceeded wall budget; stopped')
                    time.sleep(.5)
                return detail
            detail = wait_result(label)
            loop_stop_reason = None
            if case in ('loop_recovery','long_task'):
                first = next(m for m in reversed(detail['messages']) if m['role'] == 'assistant')
                loop_stop_reason = first.get('modelMetadata', {}).get('taskReasonCode')
                write_json(out / (label + '-stop-detail.json'),detail)
                if case=='long_task':
                    with sqlite3.connect(data / 'data.sql') as db:
                        for i in range(48):
                            db.execute('INSERT INTO agent_messages(id,session_id,role,content,created_at) VALUES (?,?,?,?,?)',
                                (uuid.uuid4().hex,session,'user' if i%2==0 else 'assistant',
                                 f'Obsolete historical discussion {i}. '+('Historical placeholder data; no current instructions. '*24),int(time.time()*1000)-5000+i))
                    followup=actual_task_prompt
                else:
                    followup=('The loop stress test is finished. Do not repeat that instruction. Create recovery.txt containing exactly '
                        + marker + ', read it back once, then summarize the actual result. Only operate in this project.')
                call('agent_send_message', {'sessionId': session, 'content':followup})
                detail = wait_result(label)
            scored = subprocess.run(['python3', '-c', checks], cwd=root, capture_output=True, text=True, timeout=15)
            answer = next(m for m in reversed(detail['messages']) if m['role'] == 'assistant')
            metadata = answer.get('modelMetadata', {})
            preserved = all((root / name).read_text() == text for name, text in files.items() if case not in ('tool_failure', 'planned_work','long_task') or name.startswith('test_') or name in ('sample.csv','repeat.txt'))
            failed = [r for r in detail['runs'] if r['status'] == 'failed']
            if case == 'long_task':
                exercised = loop_stop_reason=='no_progress' and metadata.get('contextSummaryRequests',0)>0 and any(r['status']=='failed' and r['tool']=='read_file' and r['arguments'].get('path')=='absent_requirements.md' for r in detail['runs'])
            elif case == 'tool_failure':
                exercised = any(r['tool'] == 'read_file' and r['arguments'].get('path') == 'absent_notes.md' for r in failed)
            elif case == 'compaction':
                exercised = metadata.get('contextSummaryRequests', 0) > 0
            elif case == 'loop_recovery':
                exercised = loop_stop_reason == 'no_progress'
            else:
                exercised = any(r['tool'] == 'update_plan' and r['status'] == 'succeeded' for r in detail['runs'])
            plan_runs = [r for r in detail['runs'] if r['tool'] == 'update_plan']
            successful_tests = [r for r in detail['runs'] if r['tool'] == 'run_tests' and r['status'] == 'succeeded'
                and json.loads(r.get('output') or '{}').get('testSummary', {}).get('verdict') == 'passed']
            counts = {name: sum(r['tool'] == name for r in detail['runs']) for name in sorted({r['tool'] for r in detail['runs']})}
            pure_plan_rounds = len({(r['responseId'], r['step']) for r in plan_runs}
                - {(r['responseId'], r['step']) for r in detail['runs'] if r['tool'] != 'update_plan'})
            record = {'case': case, 'trial': trial, 'model': model['model'], 'sessionId': session, 'seconds': round(time.monotonic()-started, 2),
                'taskStatus': detail['tasks'][-1]['status'], 'reason': metadata.get('taskReasonCode'), 'rounds': detail['tasks'][-1]['step'],
                'toolCalls': len(detail['runs']), 'failedTools': len(failed), 'pathExercised': exercised,
                'independentCheckPassed': scored.returncode == 0, 'inputsPreserved': preserved,
                'passed': scored.returncode == 0 and preserved and exercised and detail['tasks'][-1]['status'] == 'complete' and (case == 'loop_recovery' or bool(successful_tests)),
                'toolCounts': counts, 'planUpdates': len(plan_runs), 'purePlanRounds': pure_plan_rounds, 'testRunsPassed': len(successful_tests),
                'planStepCounts': [len(r['arguments'].get('plan', [])) for r in plan_runs],
                'unchangedPlans': sum(json.loads(r.get('output') or '{}').get('unchanged') is True for r in plan_runs),
                'loopStopReason': loop_stop_reason, 'summaryRequests': metadata.get('contextSummaryRequests', 0), 'context': metadata.get('context'),
                'usage': metadata.get('usage'), 'checkOutput': scored.stdout + scored.stderr,
                'taskBudget':metadata.get('taskBudget'),'modelId':model['id'],'modelRevision':model.get('revision'),'binarySha256':binary_sha256,
                'promptSha256': hashlib.sha256(actual_task_prompt.encode()).hexdigest()}
            successful_plans = [r for r in plan_runs if r['status'] == 'succeeded']
            latest_plan = successful_plans[-1]['arguments'].get('plan', []) if successful_plans else []
            record['planPhaseContract'] = (len(latest_plan) == 3 and all(step['status'] == 'completed' for step in latest_plan)) if case in ('planned_work','long_task') else None
            record['strictPassed'] = record['passed'] and record['planPhaseContract'] is not False
            results.append(record)
            write_json(out / 'report.json',results)
            print('DONE', json.dumps(record, ensure_ascii=False), flush=True)
    finally:
        stop()
        # Credentials are transient. Retain the isolated runs, but scrub the CIS
        # login token even if a probe or independent check raises.
        if (data / 'data.sql').exists():
            with sqlite3.connect(data / 'data.sql') as db:
                db.execute('PRAGMA secure_delete=ON')
                db.execute("DELETE FROM settings WHERE key='login_token'")
                db.commit()
                db.execute('VACUUM')
        log.close()
    return 0 if results and all(r['strictPassed'] for r in results) else 1


if __name__ == '__main__':
    raise SystemExit(main())
