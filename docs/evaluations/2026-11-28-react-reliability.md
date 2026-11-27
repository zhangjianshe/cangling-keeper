# Repeated Qwen Coder Next reliability evaluation — November 27–28, 2026

Ran **50 live tasks** in fresh Keeper projects and sessions: a 20-task baseline,
a 20-task candidate, and a 10-task planning-focused revision check. Every trial
is retained. The model was `qwen-coder-next` through the same CIS model revision;
no model responses or tool results were fabricated.

## Results

Each table cell represents five trials. Completion requires independent artifact
checks, preserved protected inputs, the actual target path, a completed response,
and a real passing `run_tests` call where applicable. For the multi-file planning
case, a separate strict score also requires the requested three-phase final plan.
Model-reported plan completion is not execution certification.

| Scenario | Baseline completion | Candidate completion | Latest planning revision completion |
|---|---:|---:|---:|
| Missing-file failure → repair → tests | 5/5 | 4/5 | 5/5 |
| Historical compaction → read inputs → output → tests | 4/5 | 5/5 | Not rerun live |
| Repeated-read stop → new follow-up | 5/5 | 5/5 | Not rerun live |
| Three-file implementation with an explicit plan | 3/5 | 5/5 | 4/5 |

Compaction summary requests had a **median of 7 before and 3 after** the context
fix; ranges were 6–7 and 3–4. Median case latency was 15.61 and 13.01 seconds.
The baseline's one compaction-case failure was a shell-write proposal rejected
by the evaluator, not lost input text. These small sequential samples do not
establish a causal improvement in success probability or latency.

Planning required another iteration. The first feedback candidate improved task
completion but **increased plan volume**: its small-repair trials made
`[5, 0, 5, 5, 0]` plan calls, versus zero throughout the baseline. Its multi-file
trials made `[4, 2, 2, 7, 7]` calls. This candidate is retained and is not claimed
as a planning-efficiency improvement.

After narrowing planning scope and prioritizing user-specified phases:

- All five small repairs passed and used **zero plan calls**.
- Four multi-file trials completed; each kept the requested **three phases** and
  used **2–4 plan calls**. The fifth failed on a model-server connection error
  after an initial plan and directory read. It remains a failed sample.
- The strict three-phase task score was 1/5 in the baseline, 3/5 in the first
  candidate, and 4/5 in the latest planning batch. The latest incomplete sample
  stopped on transport failure, not phase expansion.

The baseline, candidate, and latest planning batch had 17/20, 19/20, and 9/10
functional completions. Their strict scores were 15/20, 17/20, and 9/10. Do not
pool these into a single reliability rate: the runtime instructions changed and
the last batch intentionally covered only the two planning-sensitive scenarios.

## Changes

**Compaction prefix selection now accounts for what will remain.** Previously,
Keeper retained eight recent messages even when that tail plus instructions and
a summary would still exceed the compression threshold. It then repeatedly
summarized the previous summary with one or two more old messages. Prefix
selection now reduces the retained tail before requesting a summary, reserves
space for summary output, and preserves the current request and complete latest
tool-call group. An already derived summary is not summarized again merely to
meet the soft threshold when the request still fits the hard input budget.
Budgets and original persisted history are unchanged.

**Planning guidance now distinguishes task scope.** Simple repairs may proceed
through reading, editing and tests without a plan. Explicit user requests for a
plan and its phase structure take precedence over the default suggested count.
Complex work uses substantive phases rather than per-file steps. Initial
all-pending plans receive feedback to start real work rather than spend another
call changing a status. Final verified states can be combined before the final
summary. These are guidance changes: valid plan calls remain allowed, persisted
states are not silently rewritten, and verification/approval checks remain intact.
The system instructions were consolidated rather than extended with a long
repeated planning paragraph; an expanded draft exceeded the parallel-tool
context fixture and was corrected before any live use of that draft.

**Directory-hash failures explain the exact recovery.** One baseline trial guessed
three different hashes and wasted several rounds. Directory listings now say to
omit `expectedSha256` on the initial read and, after a mismatch, remove it and
reset `offset` to zero. Continuations must copy this directory's `nextList`.
Hash mismatches still fail; no stale-state check is bypassed.

**The runner records repeatability and overhead.** It supports `--repeats` and a
multi-file `planned_work` probe, counts plan-only rounds, plan updates and step
counts, and checks real passing test execution. Functional completion and the
three-phase contract have separate scores. Contract fields were recomputed from
the saved tool records for every stage, including the older runner output, using
the same rule.

## Retained failures and limits

- Baseline compaction trial 1: proposed shell write was rejected by the fixed
  approval policy; no artifact was created.
- Baseline multi-file trials 4 and 5: both exhausted 16 rounds. One combined six
  plan updates with edit retries; the other repeatedly guessed directory hashes
  and had only begun implementation.
- Candidate repair trial 2: visible tests passed but the independent check found
  whitespace-only strings were not ignored. This is a model implementation error;
  no test or scorer was weakened to hide it.
- Latest multi-file trial 1: model-server connection failure on round 3. No hidden
  retry or replacement sample was used.

Five samples per scenario are not a reliability guarantee. Plan adherence remains
model behavior, not an enforced invariant, and failures can still arise from
provider availability or incomplete implementations. The final planning revision
was live-tested on ten targeted tasks; the compaction and loop live figures above
belong to the prior candidate. The final build also passed both HTTP regression
suites, including context compaction and parallel tool groups.

## Configuration, evidence and verification

All trials ran sequentially with 16 model rounds, 40 tools and 300 active seconds
per reply, 4,096 output tokens, 32K working context for compaction and 64K otherwise.
The catalog advertises 128K. Sampling used provider defaults without a fixed seed.
User task prompts and fixture markers were identical across stages; their hashes
and the observed CIS revision are recorded in the adjacent JSON. The evaluator
approved confined file actions and the generated `run_tests` runner in each fresh
project; arbitrary shell commands stayed rejected throughout.

The separate instance copied only CIS login identity and catalog information.
Original chats, hosts, MCP configuration and synchronization settings were not
modified. Temporary login tokens were scrubbed after every batch and their
absence was checked during report generation.

Final verification: **289 Rust tests passed, one existing test ignored**; debug
build, context HTTP suite, ReAct HTTP suite, Python syntax and diff checks passed.
Regression coverage includes prefix sizing, latest tool-group preservation,
all-pending plan feedback without status rewriting, actionable hash reset without
weakening validation, and a three-summary ceiling for the deterministic context
fixture.

Machine-readable per-trial results, complete failure records, scoring definitions,
model revision, prompt/source/binary hashes and raw-evidence paths are in
`2026-11-28-react-reliability.json`. Raw batches remain under the `/tmp` directories
listed there. Reproduction commands are in `evals/agent-tasks/README.md`.
