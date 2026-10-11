# R7 实机与双设备验收：执行方式

完整设备阶段已有单 Mac 子场景实测，尚未全部通过，发布 **NO-GO**。当前原生观察见 [单 Mac 报告](COMPOSITE-CLIPBOARD-SINGLE-MAC-ACCEPTANCE-2026-10-05.md)。本机私有 board 故障探针和 R6 回退证明是支持证据，
不能关闭完整 App、真实存储故障/撤销授权或实际安装版本切换的设备门禁。
因此完整 R7 的当前结论是 **PASS 0 / UNKNOWN 10**，本机支持项可单独显示 PASS 2。

## 执行入口

当前 R6 候选目录：`artifacts/composite-repair/2026-10-05-single-mac-r6-self-contained-final/`。
先完成 R6 的 44 条命令并封存；新增加的两条检查为 R7 harness 与 R6 证据校验负向回归。

```bash
# 验证当前源码、正式矩阵、原始日志、每条命令及包文件与封存证据一致（只读）
python3 scripts/composite-evidence.py \
    artifacts/composite-repair/2026-10-05-single-mac-r6-self-contained-final --verify

# 显示十项完整设备门禁；--status 不创建状态或证据目录
python3 scripts/run-composite-r7.py 2026-10-05-single-mac-r7-self-contained-final --status

# 只重取本机支持证据，不把完整设备门禁置为 PASS
python3 scripts/run-composite-r7.py 2026-10-05-single-mac-r7-self-contained-final --run-auto \
    --r6-run 2026-10-05-single-mac-r6-self-contained-final

# 为 Windows 原生验收生成完整场景模板；未填写项全部 UNKNOWN
python3 scripts/run-composite-r7.py 2026-10-05-single-mac-r7-self-contained-final \
    --template windows-native --r6-run 2026-10-05-single-mac-r6-self-contained-final \
    > /path/to/windows-observation.json

# 实际执行并填写模板后，记录完整场景与原始证据
python3 scripts/run-composite-r7.py 2026-10-05-single-mac-r7-self-contained-final \
    --record windows-native --result PASS --r6-run 2026-10-05-single-mac-r6-self-contained-final \
    --evidence /path/to/windows-observation.json --note "真实 MSVC/dev/portable/NSIS 验收"

# 产出 native-acceptance.md；未全部通过时报告 NO-GO，进程退出 1
python3 scripts/run-composite-r7.py 2026-10-05-single-mac-r7-self-contained-final --emit
```

`--record ... --result FAIL/UNKNOWN` 可带说明而不附证据。`PASS` 必须提交一个观察 JSON，
普通非空文本不再足够。`--r6-run` 必须显式选择，不回退到历史默认目录。

## 观察记录

模板包含 `gate`、候选 `source_sha256`、`r6_run`、操作者、带时区的时间、设备身份/OS 版本、
应用/辅助技术版本、实际测试包文件的路径与 SHA256，以及该门禁的全部子场景。
每个子场景必须记载预期行为、实际行为、PASS 和至少一份非空原始证据。
相对路径相对于观察 JSON；多个场景可以引用同一份详细日志，但仍须逐场景说明观察结果。
双设备门禁必须列出身份不同的 macOS 与 Windows 设备；Windows 原生门禁要求 Windows 设备。

包文件、原始观察文件与 JSON 都会被 hash，并在报告时重新核验。记录包 hash 与源码 hash
是操作者对候选来源的声明；此工具不能从任意安装文件自动证明其由该源码构建。
对人的观察同样如此：工具检查覆盖范围与变化，不能判断描述、截图或操作者声明是否真实。
独立验收仍应核对构建日志、候选身份与实际粘贴内容。

## 判定规则

- 当前源码必须与完整、封存的 R6 源码指纹一致；每条命令、正式矩阵、原始日志、辅助证明与包文件均校验。
- PASS 逐场景覆盖；缺项、重复项、非 PASS 场景、空/缺失证据或错误平台被拒绝。
- 观察 JSON、关联原始证据、测试包、源码或 R6 封存记录变化后，已有 PASS 变 STALE。
- 无候选绑定的旧格式 PASS 变 STALE；非法状态是 INVALID；两者均阻断验收。
- 本机支持项另列范围；私有 board 与合成 authority 不证明完整设备门禁通过。
- 故障探针须同时满足退出码和结构化断言；`exit 0 + satisfied=false` 不得通过。
- 每次探针保留独立输出；更正记录保留旧观察，自动支持记录不会覆盖人工设备记录。
- 本工具不启动完整 App、不触碰 general clipboard、不驱动第二台设备。

## 十项完整设备门禁

| id | 必须覆盖的场景 |
|---|---|
| `mac-app` | 新候选启动、采集、历史恢复、通知、故障反馈；daemon/GUI/helper 全接通 |
| `windows-native` | MSVC、tauri dev、portable、NSIS 安装、真实剪贴板读写、错误反馈 |
| `source-apps` | Chrome、Word、Notes/富文本编辑器；版本、format 集合与 resource 完整性 |
| `target-apps` | 微信、Word、富文本编辑器、纯文本目标、图片消费者的实际粘贴 |
| `two-way-sync` | Mac→Win、Win→Mac，各自单图/两图/RTF/中文 emoji |
| `intent-race` | 同内容新 revision、慢传输中再复制、双端交替 100 轮 |
| `network` | LAN、Tailscale、Iroh direct/relay、重连、睡眠、ACK 丢失 |
| `faults` | helper 缺失/挂起/写后断回执、真实 storage 满/不可用、撤销授权、响应与不晚写 |
| `rollback` | 完整 v11 快照、实际安装旧 reader、外置 payload、v12 归档、真实 identity/trust/receipts 保留 |
| `accessibility` | 两端读屏、键盘、IME、深浅色、高对比、缩放；Mac VoiceOver，Windows 可用读屏工具 |

## Gate R7

承诺范围全部通过，才能进入独立候选/发布评审。`UNKNOWN`、`STALE`、`INVALID` 或 `FAIL`
均输出 **Release NO-GO**，且 `--emit` 退出 1。用户明确接受的范围调整应另有审阅记录；
本工具不会自行豁免未知场景，也不会执行发布。
