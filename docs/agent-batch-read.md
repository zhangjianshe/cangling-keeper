# 批量读取项目文件

read_files 用于读取已经确定相关的多个文件，减少逐文件工具往返。一次1–8个，files 中每项接受 read_file 参数。

```json
{"files":[{"path":"README.md","lineCount":80},{"path":"src/main.rs","startLine":1,"lineCount":120}]}
```

results 按输入顺序返回 index、path、status，以及 result 或 error。缺失文件、敏感路径、二进制或哈希不符不会伪装为读取成功，也不阻止其他合法文件读取。completedFiles 是已返回结果的数量，包含失败项；failedFiles 和 partialFailure 明确说明部分失败。

每个 result 的 nextRead 是该文件续页参数。deferred 是本次未返回结果的完整输入项，作为下一次 files；输出预算检查时可能已经读取但没有返回该项，续读会重新读取。需要版本一致时携带 expectedSha256。本工具不提供跨文件一致快照。

批次共享8秒预算，最多8个1MiB文件，输入最多8KiB；输出正文及续读字段按序列化体积限制。停止会丢弃未完成批次结果，不继续读取后续文件。不会执行命令、修改文件或自行重试。

验证：14项文件工具测试、上下文缩短测试、6组ReAct验收（模拟模型）。模拟模型验证一次批量返回两个文件和一个缺失错误，刷新后执行记录一致；现有账户隔离、停止、重连回归全部通过。
