# 复合剪贴板原生与双设备验收清单

2026-10-03 建立。用户选择稍后安排双设备；本清单不代表验收已经执行。所有真实应用结果初始为 UNKNOWN；本地合成证据在实施记录和 `artifacts/composite-evidence/`。

## 证据模板

每组记录以下字段，不保存真实敏感剪贴板正文：

```json
{
  "case_id": "APP-01",
  "baseline_sha": "f719d581a1aa5198a1b3f2483222df5c69e3dfc0",
  "source_fingerprint": "UNKNOWN",
  "mac_bundle_sha256": "UNKNOWN",
  "windows_artifacts_sha256": "UNKNOWN",
  "source_os_version": "UNKNOWN",
  "target_os_version": "UNKNOWN",
  "source_app_version": "UNKNOWN",
  "target_app_version": "UNKNOWN",
  "connection_mode": "UNKNOWN",
  "feature": "protocol-composite-clipboard",
  "source_formats_and_sizes": [],
  "target_formats_and_sizes": [],
  "visible_result": "UNKNOWN",
  "history_groups_before_after": "UNKNOWN",
  "unexpected_outbound_events": "UNKNOWN",
  "result": "UNKNOWN"
}
```

## 启动与数据隔离

1. 使用最终源码指纹对应的 Mac / Windows 候选，记录构建日志、sidecar 和安装包摘要；Windows 原生编译、PowerShell 包入口与安装 smoke 必须真实执行。
2. 保存当前剪贴板，使用合成网页和文档。关闭旧实例并确认 TCP 19890 无监听，再启动隔离数据目录中的候选；确认 PID 与准确可执行路径。不要混用旧 daemon 与新 UI。
3. 不使用用户现有 history DB、配对记录和正式设置作故障注入。v12 备份回退使用完整、匹配的隔离数据树，保留密钥和图片文件；仅回退数据库不足以证明内容可恢复。
4. 每次明确记录是否新端↔新端、运行时关闭或真实旧版端。测试结束恢复剪贴板并正常退出候选。

## 真实应用矩阵

| ID | 来源与操作 | 目标 | 预期判定 | 当前 |
|---|---|---|---|---|
| APP-01 | Mac Chrome：中文、emoji、文字+内嵌透明 PNG 双图 | Windows 微信 | 一次粘贴可见文字及双图；无重复，顺序与内容正确 | UNKNOWN |
| APP-02 | Windows Chrome：同一内容 | Mac 微信 | 同上 | UNKNOWN |
| APP-03 | 双端 Chrome：实际普通网页选中文字和 HTTP 图片 | 对端微信 | 记录源是否只有 URL；若缺失不可判完整成功；明确支持组合 | UNKNOWN |
| APP-04 | Mac / Windows Word：图文、富文本、表格 | 对端 Word / Mac Notes | 不丢格式/图片，不把替代表示变成重复文档对象 | UNKNOWN |
| APP-05 | APP-01/02 同一条内容 | Windows 记事本 / Mac 纯文本编辑器 | 文本可粘贴，中文和原始空白保留 | UNKNOWN |
| APP-06 | Mac RTFD 与真实多 native items | 原 Mac 与 Windows | 原 Mac 保持形状；跨端明确不支持/不完整，不静默截断 | UNKNOWN |
| APP-07 | Windows DIB/DIBV5：透明、stride、上下方向 | Mac 图像编辑器 | 像素、方向和 alpha 正确 | UNKNOWN |
| APP-08 | 大型 Excel / Word 表格 | 双端 | 记录所有格式、捕获耗时、helper 截止、UI 响应与 RSS；失败明确 | UNKNOWN |

微信支持必须按实际版本和输出格式组合判定。若只粘贴图片或只粘贴文字，APP-01/02 的原图文门槛未通过，不能用“系统包含 HTML”替代可见结果。

## 可靠性与历史矩阵

| ID | 场景 | 判定 | 当前 |
|---|---|---|---|
| REL-01 | 10,000 次跨设备交替远端写入，统计真实出站事件 | 无自身写入回传；一条内容一个历史组 | UNKNOWN |
| REL-02 | 远端写入后 50ms、500ms、1s、29s 用户重新复制相同内容，携带旧标记 | 新 revision 可同步，无固定时间吞事件 | UNKNOWN |
| REL-03 | 慢图文 A 重组中本机复制 B、恢复文字/图片/文件批次 | A 不覆盖 B；明确 Superseded 与历史状态 | UNKNOWN |
| REL-04 | 缺片、重复片、ACK 丢失、重连、两个 peer、能力减少 | 无部分发布、无重复副作用，编码和身份固定 | UNKNOWN |
| REL-05 | 完成前撤销配对或禁用 peer | 后续帧和原生副作用被拒绝 | UNKNOWN |
| REL-06 | 发布前 claim、系统发布后、DB 提交前后和最终 ACK 前后进程退出 | 不重复自动发布不确定事件；历史可恢复，结果可解释 | UNKNOWN |
| REL-07 | LAN、当前支持的 Tailscale 路由、Iroh、睡眠和网络切换 | 按连接模式分别记录；2026-10-04 完整串行 Core 含 Iroh 已通过，实机链路仍待验收 | UNKNOWN |
| NET-01 | N0 冷启动，普通 Iroh 配对/已配对重连，仅 Iroh 可达 | 分段记录地址发现、QUIC、Noise；确认 5 秒预算是否阻止稍后可成功的连接；明确失败和重试 | UNKNOWN |
| NET-02 | 邀请配对，不同出口、强制 relay、代理/Tailscale 开关及近过期邀请 | 记录 direct/relay 与阶段耗时；30 秒连接截止、120 秒窗口和取消可解释；过期不得进入确认 | UNKNOWN |
| HIS-01 | 图文历史重启、搜索、整组恢复、显式文字/图片恢复 | 格式仍在；一个卡片；选择恢复结果符合选择 | UNKNOWN |
| HIS-02 | 两条历史共享大图、收藏、删除、quota、满盘和外部目录迁移 | 不误删共享图或收藏；配额按实际数据树；失败无假成功 | UNKNOWN |
| HIS-03 | v11→v12→隔离回退；旧 text/image/file/batch | 备份与旧历史可读；旧二进制拒绝直接打开 v12 | UNKNOWN |
| UI-01 | 两端 History 搜索、缩略图、菜单、键盘、中文 IME、辅助功能 | 可见界面符合原生行为；错误和不完整提示可见 | UNKNOWN |
| COMPAT-01 | 新↔旧、构建 feature 关闭、运行时关闭 | 不向旧端发送新命令；仅一个降级事件，状态明确 | UNKNOWN |

Iroh 本地诊断见 `docs/IROH-TEST-DIAGNOSIS-2026-10-04.md`。已确认测试短窗口对延迟敏感，本机隧道单地址不可达；最初自然延迟触发尚未锁定。NET-01/02 使用真实 Windows/Mac 候选与 N0 环境，不以 Minimal 本机回环或故障注入替代。

## 后续适配的进入条件

RTFD 附件转换和微信专用输出配置需要先采集真实来源/目标格式证据，再确定实现。URL-only 资源若要补齐，需要另行确定可访问来源、大小与时限、认证语义和明确的完整性验证；当前代码不自动抓取远程或本地 URL。缺少这些条件的场景保留 UNKNOWN / 未实现，不通过合成 HTML 用例替代。

所有必须场景应记录 PASS、FAIL 或带理由的 UNKNOWN；有 FAIL/UNKNOWN 时继续保持候选开关，不认定完整需求或发布门槛完成。
