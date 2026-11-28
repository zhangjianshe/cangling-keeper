"""Scope preflight support for existing isolated model-provider fixtures.

Execution requests keep their original counters and assertions. Tests dedicated
to the routing protocol override the assessment themselves; this helper does
not make production routing decisions.
"""
import json
import sys

def respond_scope(handler, body):
    if not isinstance(body, dict) or [t.get('function', {}).get('name') for t in body.get('tools', [])] != ['assess_task_scope']:
        return False
    assert body['temperature'] == 0
    assert body['tool_choice']['function']['name'] == 'assess_task_scope'
    request = json.loads(body['messages'][-1]['content'])['currentRequest']
    planned = any(marker in request for marker in ['plan fixture', 'plan retention fixture', 'blocked steps fixture', 'seed inheritance fixture'])
    category = 'known_operation' if any(s in sys.argv[0] for s in ['test-earth', 'test-map-mcp']) else 'localized_edit'
    args = {'category': 'development' if planned else category, 'explicitPlan': False, 'reason': 'Disposable fixture scope'}
    delta = {'tool_calls': [{'index': 0, 'id': 'scope-fixture', 'type': 'function', 'function': {'name': 'assess_task_scope', 'arguments': json.dumps(args)}}]}
    handler.send_response(200)
    handler.send_header('Content-Type', 'text/event-stream')
    handler.end_headers()
    handler.wfile.write(('data: ' + json.dumps({'choices': [{'delta': delta, 'finish_reason': 'tool_calls'}]}) + '\n\ndata: [DONE]\n\n').encode())
    handler.wfile.flush()
    return True
