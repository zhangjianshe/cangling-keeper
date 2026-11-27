# Live ReAct resilience evaluation — 2026-11-27

Final live run: **3/3 scenarios passed**, with the intended runtime path exercised
and artifacts checked independently. Model: `qwen-coder-next` through the configured
CIS proxy. No model responses or tool outcomes were fabricated.

| Scenario | Observed behavior | Independent result | Seconds | Tool calls |
|---|---|---|---:|---:|
| Tool failure | Missing file → repair calculator → run tests | Function cases passed; tests preserved | 22.35 | 10 |
| Context compaction | 48 seeded historical messages → real summaries → read inputs → create answer → tests | Total 42 and exact current-request marker; inputs preserved | 21.13 | 4 |
| Loop recovery | Three identical reads → no_progress stop → new follow-up | Recovery file verified; original input preserved | 12.73 | 5 |

The loop task includes both generations in its tool count. Follow-up is an explicit
new message, not replay or automatic resumption. Raw records remain locally under
`/tmp/keeper-live-react-20261127-{b,c,d,e}`; temporary CIS login tokens were removed.
Fresh project files remain available for inspection.

## Fixes found through evaluation

1. **Summary budgeting charged data that was never sent.** The summarizer's
   planning estimate included the full agent system prompt and tool schemas. It
   now estimates the actual summary request, including serialized historical data
   and the summarizer's own instructions. Request construction and estimation
   share one helper.
2. **Tool-output allocation ignored fixed context costs.** Instructions, tool
   schemas and assistant call arguments now reduce the available output allowance.
   This fixes a parallel-tool regression in the context HTTP suite.
3. **Tiny file bodies disappeared behind JSON header previews.** A constrained
   rerun successfully read both inputs, but the model could not see their text,
   repeatedly reread them, then proposed shell reads that the evaluator rejected.
   The response ended with blockers and no answer artifact; independent scoring
   correctly failed it despite response status `complete`. Shortening now retains
   small text bodies in full, spends larger previews on actual text, and exposes a
   sticky `textTruncated` flag. Regression tests cover both small input bodies and
   repeated shortening.

The baseline compaction sample required 12 summary requests; the final sample
required 7 with identical task prompt hash and seeded history contents. The
baseline and final times were 21.31 and 21.13 seconds, and reported prompt-token
usage was 56,700 and 56,760. This supports a reduction in summary request count,
not a claim of lower total tokens or faster execution.

## Retained attempts and limits

The initial text-only-promise probe completed its artifact but did not exercise
recovery: the model immediately called a tool. It is retained as unexercised,
not counted as a successful recovery test. The later repeated-read probe reached
`no_progress` and completed its follow-up in both intermediate and final runs.

The intermediate compaction failure is preserved in the JSON report. Two runner
issues were also corrected: isolated startup needed a separate temporary lock
directory and disabled HTTPS; the API exposes tool arguments as objects rather
than JSON strings. The first completed task's score was reconstructed from its
saved records without replaying model calls.

These are small fixtures with one final sample each and provider-default
sampling, not a statistical benchmark. The tool-failure samples varied from four
to ten calls, mainly because the model sometimes updated its plan after each
operation. This remains a model-efficiency observation, not a demonstrated
runtime correctness defect.

## Configuration and validation

All probes used fresh projects, 16 model rounds, 40 tool calls, 300 active seconds
per reply and 4,096 output tokens. Working context was 32K for compaction and 64K
otherwise; the service catalog advertises 128K. Only file actions and the generated
`run_tests` runner in the fresh project were approved. Other shell requests were
rejected. Original chats, hosts, MCP services and synchronization settings were
not copied or modified.

Final checks: **285 Rust tests passed, one existing test ignored**; debug build,
context HTTP suite, ReAct HTTP suite, Python syntax check, and diff whitespace
check passed. The reusable runner is `scripts/eval-react-resilience.py`; usage is
in `evals/agent-tasks/README.md`. Machine-readable results, prompt hashes, session
IDs, usage scopes, all scored attempts and final source hashes are in the adjacent
`2026-11-27-react-resilience.json`.
