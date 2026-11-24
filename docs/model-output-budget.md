# 模型单次输出预算

模型服务列表中的“单次输出”按当前登录身份和模型 ID 保存到本机 SQLite settings，不上传 CIS。CIS 代理继续透传 max_tokens，无需更新服务器。

- 上下文至少 64K 的 DeepSeek 默认 16K；其他服务默认 4K。
- 可配置 1024–65536 token，且必须小于服务配置的上下文上限。用户仍须按上游模型的实际输出能力设置。
- 实际输出取配置值与当前工作窗口四分之一的较小值，为系统提示、输入、图片和工具结果保留空间。
- 任务开始时固定实际输出预算；所有 ReAct 请求与上下文压缩使用相同预留量。修改配置从下一次生成生效，包括重新生成。
- 历史回复记录实际 maxOutputTokens 与 context.outputReserved。重新生成仍使用原模型，使用其当前输出配置。
- finish_reason=length 仍终止本轮，不执行截断的工具参数；不会为了自动续写而重放有副作用操作。

验证：test-output-budget-http.py、test-output-budget-ui.cjs、test-context-http.py、test-cis-model-proxy-http.py。

2026-11-24 本机验证：HTTP/UI 回归通过；真实 CIS/DeepSeek 压力请求实际输出 16384 token 后安全截断（64.5 秒），证明请求已超过旧 4096 上限。范围明确的 TIFF 脚本请求 32.3 秒正常完成，输入 5708、输出 1844 token，Python 语法检查通过。两次均不执行工具、不写项目文件。此验证不是对任意长度任务都能在 16K 内完成的保证。
