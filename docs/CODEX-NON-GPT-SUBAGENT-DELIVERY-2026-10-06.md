# 非 GPT 子代理任务递送研究

日期：2026-10-06，Asia/Shanghai。范围：内置 Multi-Agent 递送诊断；本轮不实施 O03、不修改 TailSync 产品源码。

**结论：当前机器已经找到并实测通过的路径，是先开启 CPA-GUI / CLIProxyAPI 的 `client.codex.optimize-multi-agent-v2` 兼容开关，再向内置子代理传入明文 `message`，并使用 `fork_turns: "none"`。** DeepSeek/high 的短口令、排队及后续消息、完整中文 O03 范围均已准确到达。无需外部代理进程或 GPT 中转。

本次只临时打开兼容开关，验证后已恢复 `false`。代理配置与测试前逐字节一致，没有重启代理或 App。因此正常 O03 派发前仍需启用该开关。本轮只证明任务递送可用，O03 生产实现尚未开始。

## 1. 环境与根因证据

本机代理为 CPA-GUI 配置的 CLIProxyAPI v8.0.16，启用 ClinePassBridge v0.2.11。Codex 子会话版本记录为 0.160.0，模型 `cline-pass/deepseek-v4.1-flash`，思考强度 `high`，provider `cpa-gui`，通过本地 Responses 接口路由。

相关配置文件：[config.yaml](</Users/monet/Library/Application Support/com.cpa.gui/cpa-core/config.yaml:170>)。第 170–177 行说明此开关会刷新多代理模型信息、去除消息参数加密、规范化 agent_message，并为非 Codex 上游转换成标准用户消息。初始值为 `false`。此说明来自本机安装的代理配置，不是 OpenAI 对第三方代理的兼容保证。

关闭兼容开关时，即使主代理原本写入简短明文任务、设置 `fork_turns: "none"`，实际保存的工具调用 `message` 仍是 `gAAAA…` 的不透明字符串；子会话中可读内容只有 `Message Type: NEW_TASK / Payload:` 头部。DeepSeek 没有收到指定口令，回复未收到任务。

这证明 **关闭上下文继承与去除任务参数加密是两件事**。仅缩短提示、换中文、增加任务说明或重发同一类密文消息不能解决这条链路。

开关热加载后，第一条诊断请求仍沿用了密文参数。DeepSeek 当时能看到不透明字符串，开始查找环境线索；主代理已中断该探针。这条记录不计为成功，也不采纳它的猜测。后续新发送会话及当前主会话的工具参数均实际为明文，DeepSeek 精确回显成功。没有证明第一条旧格式请求残留的具体原因是缓存、预先生成还是其他时序；可确认的是打开开关不能恢复已经加密的正文，应以新请求的实际参数和回执验收。

还有一个记录细节：成功递送时，子会话内部字段名仍可能叫 `encrypted_content`，但其**实际值已经是任务原文**。不能仅凭字段名判断仍在使用密文；本次同时检查主代理工具参数、字段实际值和精确回执。成功探针不需要解密密文或寻找密钥。

## 2. 实测结果

所有 DeepSeek 探针均由内置 Multi-Agent 创建，`high`，`fork_turns: "none"`。

| 方法/状态 | 验收结果 | 证据含义 |
|---|---|---|
| 初始关闭开关，50 字符短英文任务 | FAIL：未收到口令 | 问题不依赖长提示或中文 |
| 开关热加载后，首条仍为密文的请求 | FAIL，中断 | 不能把看到密文字符串当作收到任务 |
| 普通 app-server 消息直接发给 v2 子线程 | 工具拒绝 | 返回 `direct app-server input is not allowed for multi-agent v2 sub-agents`，不是可用替代路径 |
| 新发送会话 → 内置 DeepSeek 子代理 | PASS | 精确返回 `DS_FRESH_RECEIVED_B6T3` |
| 当前主代理 → 内置 DeepSeek 子代理 | PASS | 精确返回 `DS_DIRECT_RECEIVED_L5W8`；不需要中转 |
| `send_message` 排队 → `followup_task` 唤醒 | PASS | 精确返回排队口令和独立唤醒口令，两行完全匹配 |
| 1171 字符完整中文任务 | PASS | 回显任务 ID、唯一工作树、O03、原件/派生与 PNG/APNG 限制、禁止进入 O04、NO-GO |

四项成功递送检查覆盖三个 DeepSeek 子线程。成功探针均 **0 次工具调用**，没有读取会话文件或环境来推断口令。新发送会话只是诊断用的对照，不是推荐常规增加的中转层。其他非 GPT 模型未测试，不能将 DeepSeek 的结果直接记为它们通过。

完整原始范围、实际消息值的脱敏提取和响应见 [results.json](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/artifacts/clipboard-improvement/2026-10-06-multi-agent-delivery/results.json)。失败输入只保留长度和摘要，不复制密文正文或认证信息。

## 3. 推荐调用方式

先在 CPA-GUI 对应配置中启用：

```yaml
client:
  codex:
    optimize-multi-agent-v2: true
```

确认运行中的代理已载入该值后，使用内置工具：

```json
{
  "task_name": "deepseek_o03",
  "model": "cline-pass/deepseek-v4.1-flash",
  "reasoning_effort": "high",
  "fork_turns": "none",
  "message": "任务ID、唯一工作目录、具体范围、约束、交付和验收要求的完整明文"
}
```

当前工具没有 `fork_context` 参数；关闭上下文继承使用 `fork_turns: "none"`。任务正文应包含实际工作树绝对路径，不能依赖环境默认 cwd。本项目默认 cwd 是另一份 checkout。

建议先用无工具的回执任务，让子代理逐字返回任务 ID、目录和范围。通过后再用明文 `followup_task` 交付完整实施任务；运行中的补充指令使用 `send_message`，空闲子代理的 `send_message` 只排队，需要 `followup_task` 唤醒。回执核验是代理之间的技术检查，不要求用户再次确认已授权任务。

若重新收到“无任务”或不透明字符串，检查代理开关是否生效及实际新请求是否仍被加密；不要将推断、从兄弟会话找出的口令或环境自查记作递送 PASS。

## 4. 恢复和边界

开关变更只有一个布尔值；本机日志确认 `false -> true` 热加载。本轮结束后恢复 `true -> false`，配置摘要与测试前一致，临时私密备份已移除：[恢复核验](/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/artifacts/clipboard-improvement/2026-10-06-multi-agent-delivery/config-test-restoration.json)。未改变 Codex 用户配置、认证信息或默认模型，没有后台代理进程。

TailSync 产品/配置/测试源码仍是 `364fbe7819331b2e28f3eb2477e219f1a32a691dba3e4469fec58493bb984aec`，771 文件，与 O03 派发前一致。新增内容仅为研究文档及独立证据目录。O03 未实施、未运行产品测试，发布状态继续 NO-GO。

证据解析首轮把内部字段名当作密文标志，断言失败；修正为检查实际值后全部递送断言通过。该过程保留在 `verification-attempts.json`，没有把首轮失败删除或计为产品缺陷。

官方 OpenAI 文档说明内置子代理的模型/思考强度配置、消息调度及线程管理：[Subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents)。第三方代理开关与本轮通过结果来自上述本机配置和执行证据；官方页没有承诺这一特定代理组合的行为。
