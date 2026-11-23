# 上下文预算对照 · 2026-11-23

## 上限证据
当前 CIS 管理接口确认上游为 https://api.deepseek.com，模型 deepseek-flash。
官方 /models 文档示例公开 context_window=1048576、max_output_tokens=393216： https://api-docs.deepseek.com/api/list-models/ 。本次直接请求上游 /models 返回401，未取出服务端密钥；因此此值是官方公布上限，不是本账号的大输入边界实测。
CIS 服务配置原为32768，已通过现有管理员接口改为131072，保留密钥及其他配置。Keeper 显示的是这个服务配置上限；工作预算单独按会话保存，不能超过它。
CIS 当前源码限制配置最多131072，输出max_tokens也按服务配置校验。未做百万token极限请求。

## 估算与预算
原按UTF-8字节计数；现为ASCII每字符0.5、其他字符1.5、每结构64封装余量，工具及图片单独计入。是启发式，不是精确tokenizer或最大误差保证。官方文字比例参考 https://api-docs.deepseek.com/quick_start/token_usage/ ，实际计费仍以usage为准。
工作预算包含4096输出和2048安全预留；在剩余输入预算80%时尝试压缩。短摘要不收敛时扩大完整历史前缀；不可再压缩但仍在硬输入预算内时不因软阈值失败。
会话未设置工作预算时默认取128K与服务上限的较小值，切换到更小模型时自动钳制；不会修改模型实际能力。

## 同题对照
两组均使用新项目、同一CIS模型与版本、相同任务和追加提示、审批策略、8轮/32工具/180秒执行限制。每题一次，无固定随机种子。服务端时延和缓存、模型路径会影响耗时；样本太小不能推断统计优势。

|预算|任务|正常收尾|代码验收|耗时秒|输入token|输出token|摘要调用|
|---|---|---|---|---:|---:|---:|---:|
|64K|sum-empty|complete|True|20.5|46398|965|0|
|64K|cross-file|failed|True|91.14|48354|1338|0|
|64K|retry-after-test-failure|complete|True|20.4|43204|1575|0|
|128K|sum-empty|failed|True|19.44|46647|832|0|
|128K|cross-file|complete|True|17.5|35583|1075|0|
|128K|retry-after-test-failure|complete|True|14.47|28809|1043|0|

64K 合计：132.04秒，输入137956、输出3878 token。正常收尾2/3，代码验收与撤销3/3，摘要请求0。

128K 合计：51.41秒，输入111039、输出2950 token。正常收尾2/3，代码验收与撤销3/3，摘要请求0。

两组失败均为8轮执行上限，不是上下文超限。64K跨文件任务等待上游较久，不能把两组耗时差归因于窗口。
这组三个小任务在64K已无需压缩；128K未表现出成功率优势。大型任务仍需另建压力评测。
评分后所有内置文件修改均已撤销，恢复原始基线。测试记录保留在本机 /tmp/keeper-context-64k-01 与 /tmp/keeper-context-128k-01。
对照运行后另完善软阈值内不可压缩情况的处理，不影响本次两组均未压缩的调用路径。

## 复跑
`python3 scripts/eval-agent-real.py --execute --budget 65536 --output /tmp/新的64k目录`
`python3 scripts/eval-agent-real.py --execute --budget 131072 --output /tmp/新的128k目录`
真实模型评测会消耗额度；出现任务异常会返回非零退出码并保留报告，不会将代码测试通过当作正常收尾。
