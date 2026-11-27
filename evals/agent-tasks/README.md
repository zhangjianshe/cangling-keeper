# 真实编程任务评测集

运行 `python3 scripts/eval-agent-tasks.py prepare /tmp/keeper-eval-唯一名称` 创建三个独立项目。
在 Keeper 中分别使用各项目目录，将 TASK.md 中任务发送给待评测模型；使用相同模型配置、安全模式和预算。
完成后运行 `python3 scripts/eval-agent-tasks.py score /tmp/keeper-eval-唯一名称`。
评分器从本仓库读取验收代码，不信任模型修改的测试，输出 report.json。评分会执行项目 Python 代码，应在隔离测试环境中运行。

覆盖空输入错误修复、跨文件实现、边界错误修复，检查用户原有内容。每次运行使用新目录。
同时记录模型/版本、任务会话 ID、token、模型任务耗时、工具次数及人工干预次数；评分器 seconds 仅为验收耗时，不是模型耗时。
任务面板用于核对实际文件差异及命令退出码。模型声称完成不能替代评分。

本轮验证评分器：原始任务必须失败，参考修复后必须通过。真实模型运行结果见 real-results-2026-11-23.md；可用 scripts/eval-agent-real.py 显式启动复跑。
撤销单独通过模拟模型端到端测试及文件单元测试验证，避免评测任务中的任意命令混淆撤销覆盖范围。

## Live ReAct resilience probes

`scripts/eval-react-resilience.py` starts the current debug build in a separate
Keeper instance and creates fresh projects. It copies only CIS login identity
and the model catalog from `--source-data`; credentials are never printed and
the temporary login token is scrubbed when the runner exits. Existing chats,
hosts, MCP services, and synchronization settings are not copied or changed.

```bash
cargo build
python3 scripts/eval-react-resilience.py --execute \
  --source-data /path/to/keeper-data \
  --output /tmp/keeper-react-new-run \
  --model qwen-coder-next
```

The tool-failure task requires a real missing-file error before implementing and
testing a small function. The compaction task seeds 48 obsolete messages and
requires a correct artifact containing a marker from the current request. The
loop task deliberately repeats an identical read until `no_progress`, then sends
a new follow-up and checks that useful work can finish. The evaluator approves
only confined file actions and the generated `run_tests` runner in the fresh
project; other commands are rejected. It never fabricates model responses.

`report.json` distinguishes actual path coverage, independent artifact checks,
input preservation, and response status. A completed response alone is not a
passing task. `--cases` selects probes; `--marker` fixes the artifact marker for
before/after comparisons. Retain failed and unexercised attempts when reporting
results. See `docs/evaluations/2026-11-27-react-resilience.md` for a live run.

For repeated reliability measurements, add `--repeats 5` and select the explicit
multi-file planning probe as well:

```bash
python3 scripts/eval-react-resilience.py --execute \
  --source-data /path/to/keeper-data \
  --output /tmp/keeper-react-repeated-new \
  --marker repeatable-fixture-marker --repeats 5 \
  --cases tool_failure compaction loop_recovery planned_work
```

Trials run sequentially in fresh projects and sessions. Reports count plan calls,
plan-only rounds, unchanged plan submissions, actual passing test runs, and plan
step counts. `planned_work` requires three phases; `planPhaseContract` checks its
final reported plan and `strictPassed` combines that requirement with functional
completion. `passed` retains the functional completion score for comparison with
older reports. Neither plan status nor model summaries certify business success.
Retain every trial, including policy-blocked runs and independent-check failures;
compare medians and ranges rather than selecting a successful run.


### Qwen Coder Next long task with 128/128/1800 budgets

The long-task fixture covers a four-module SQLite order library, forced history
compaction, an identical-read loop probe followed by resumed work, a genuine
missing-file failure, and independent acceptance beyond the supplied tests.
A 64K working window is a separate configuration from the unsuccessful 32K
stress samples; do not combine these into a claimed reliability rate.

```bash
python3 scripts/eval-react-resilience.py --execute \
  --source-data /zjs/satway/.local/share/cn.cangling.keeper \
  --output /tmp/keeper-qwen-long-128-reproduction \
  --model qwen-coder-next --cases long_task \
  --rounds 128 --tools 128 --active-seconds 1800 --working-budget 65536
```

`--execute` consumes live model usage. The runner uses a private HOME and database,
approves confined file edits and the generated unittest runner only, and scrubs
its temporary CIS token on exit. JSON snapshots are replaced atomically for live
inspection. The observed canonical-retry failure can be repaired by the same model
in the same retained session using `scripts/eval-long-task-repair.py --execute
--source-data ... --batch ...`; that follow-up is scored separately and never
turns the original failed sample into a first-pass success.

See [the complete report](../../docs/evaluations/2026-11-28-qwen-long-128.md) and
[per-stage evidence](../../docs/evaluations/2026-11-28-qwen-long-128.json).
