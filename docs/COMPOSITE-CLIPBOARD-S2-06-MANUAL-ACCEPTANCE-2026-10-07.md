# 复合剪贴板 S2-06 人工验收与证据采集指南（2026-10-07）

本指南对应执行计划 [`docs/COMPOSITE-CLIPBOARD-S2-EXECUTION-PLAN-2026-10-07.md`](COMPOSITE-CLIPBOARD-S2-EXECUTION-PLAN-2026-10-07.md) 的 S2-06「QQ 与系统提示的真实闭环」，以及用户反馈 F01/F05（[`docs/COMPOSITE-CLIPBOARD-USER-FEEDBACK.md`](COMPOSITE-CLIPBOARD-USER-FEEDBACK.md)）。它只描述**离线**的证据整理与校验流程；配套工具是 [`scripts/validate-s2-06-evidence.py`](../scripts/validate-s2-06-evidence.py)，单元测试是 [`scripts/tests/test_s2_06_evidence.py`](../scripts/tests/test_s2_06_evidence.py)。

本阶段**不执行任何采集**：不启动候选 App、不触碰 general clipboard、不读取 QQ 登录态/聊天正文/私有缓存或数据库、不联网。真正采集由用户在场并单独授权后，按本指南手工完成。

## 1. 目的与边界

- 工具做的是**证据完整性校验**，不是产品验收。它只输出三种类别：`INVALID`、`INCOMPLETE`、`READY_FOR_MANUAL_REVIEW`。**没有 PASS，也没有 Release GO / S2 完成。**
- `READY_FOR_MANUAL_REVIEW` 只表示清单结构齐备、显式引用文件的字节与申报长度/hash 一致，可以交人工审查；**不验证来源真实性、回执正文与字段的语义对应、实际运行的候选或验收成功**。把 `synthetic` 改成 `real_qq` 不会获得真实来源证明。
- 真实 A07/A02 在人工/父级实际观察前**恒为 UNKNOWN**。工具输出中 `real_a07`、`real_a02` 永远是 `UNKNOWN`，`release` 永远是 `NO-GO`，`s2_complete` 永远是 `false`。
- 自动化结果与真实证据必须分开记录。自动门禁（如 S2-05 的 25/25）属于自动化证据，写进各自的运行记录，**不得**放进本清单冒充真实 QQ 现场证据。
- 缺失的样本、阶段、成对目标、条件、提示一律记为 UNKNOWN（`INCOMPLETE`），**不继承**任何更早的 PASS。

## 2. 授权与同意范围

采集前必须由用户显式确认，并把结论写进清单的 `authorization`：

- `consent_confirmed`：用户确认在指定测试会话中执行本次采集。
- `public_test_content`：本次只使用**可公开的测试内容**（见第 5 节的红/蓝样本），不涉及用户真实聊天内容。
- `scope`：`public_test_only`（仅公开测试内容）或 `user_content_authorized`（用户另行授权真实内容）。两者都没有时用 `none`。

以下行为需要**单独授权**，本指南与工具都不代为执行，也不默认开启：

- 候选 App 的构建、安装、启动、切换与日常 App 替换。
- 触碰 general clipboard（读写用户当前剪贴板）。
- 读取 QQ 登录态、聊天正文、私有缓存或数据库。
- 关闭/修改系统权限提示或通知开关来「消除」提示。

## 3. 隔离候选与 hash 绑定

1. 使用隔离的候选构建，**不要**覆盖或删除用户正在使用的 App、现有包与缓存。
2. 记录候选与 helper 的 SHA256，写入清单 `environment`：
   - `candidate_sha256`：本次安装/使用的候选主二进制摘要；不把主二进制 hash 冒充整个 App bundle hash。
   - `helper_sha256`：与候选匹配的 `clipboard-helper` 摘要。
   - `source_sha256`：候选构建时实际源码的 `scripts/composite-evidence.py` 完整文件清单指纹；另保存其 inventory 与构建绑定回执。旧 S2-05 包或裸复制 hash 不能认证新构建。
   - 取摘要（macOS 系统命令）：`shasum -a 256 <候选主二进制> <候选内的 clipboard-helper>`。
3. helper 协议版本必须与平台一致：`macos/src-tauri/clipboard-helper.swift:18` 为 4；`shared/platform-clipboard-composite.rs:516` 为 macOS 4、其他平台 2，不能推广为 Windows 4。经授权可对候选内**绝对路径**的 helper 执行 `--self-check`（Swift `:651`），macOS 输出形如 `CWH1 protocol=4 analyze=1`。self-check 不读 pasteboard，也不证明 QQ 采集或候选已经启动。
4. 运行期 helper 解析与健康检查在 `shared/platform-clipboard-composite.rs:564`（`resolve_helper`）与 `:606`（`helper_health`）；macOS 侧调用入口在 `macos/src-tauri/src/clipboard_file.rs:23`（`resolve_clipboard_helper`）。
5. `environment` 还须记录：`source_app`（固定 `qq`）、`source_version`（QQ 版本）、`os_version`（macOS 版本）、`target_app` 与 `target_version`（粘贴目标应用及版本，必须与样本内 `targets` 一致）。
6. 实际启动副本的绝对路径、主进程和 runtime helper 解析路径另存人工回执，核对与上面候选一致。不要停止/替换日常 App 来制造验收；隔离候选无法安全并存时保持 UNKNOWN。

## 4. 目标应用与「同一目标」原则

- 所有样本的 `targets.direct` 与 `targets.history` 必须指向**同一** `target_app`/`target_version`，且与 `environment` 一致；不一致判为 `INVALID`（`E_TARGET_MISMATCH`）。
- 直接粘贴（来源复制后立即在同一目标粘贴）与历史恢复（从 TailSync 历史里恢复后再粘贴）必须成对采集；缺任一侧记 `INCOMPLETE`（`I_MISSING_PAIR`）。
- 目标应用本身不支持的样式不算 TailSync 保真失败；「看起来相似」也不能当作对象可编辑性通过。

## 5. 安全公开样本（红/蓝）

使用**自造、可公开**的测试内容，避免任何真实聊天正文：

- 红样本：一张纯色（如纯红）小图。
- 蓝样本：一段明确标注为测试的短文字 + 一张纯色（如纯蓝）小图。
- 文字+双图：上述文字 + 红、蓝两张纯色小图。

推荐在 QQ 的**本地草稿**中组织这些内容，或使用用户明确允许的测试会话；不需要真的发送消息。若某动作只能发送后才可复制，则先获得用户对该测试会话的单独授权，并在清单 `authorization` 中如实记录范围。

## 6. QQ 版本与复制动作（五个 case）

清单要求覆盖五个来源/动作 case（`samples[].case`）：

| case | 内容与动作 |
|---|---|
| `single_image` | 单张图片 |
| `text_plus_one` | 文字 + 一张图片 |
| `text_plus_two` | 文字 + 两张图片 |
| `selection` | 在 QQ 中拖选内容后复制 |
| `menu` | 通过 QQ 右键/应用菜单「复制」 |

每个 case 至少一个样本，且每个样本独立采集，**不共享**其他样本的结论。QQ 版本固定后记录到 `environment.source_version`。样本必须带与顶层一致的 `run_id`、`session_id` 及唯一 `source_action_id`；inventory/每个 stage/直接历史 pair/持久条件均重复绑定同一 `source_action_id`，不能把两次复制拼成一个来源快照。选择/菜单样本的具体内容、按钮与顺序写进公开测试回执。

## 7. 阶段调用链（从当前代码核实）

逐层定位时，按下面的真实调用链记录每个阶段（`samples[].stages`），不要凭猜测编造命令或结论：

| 阶段（清单 key） | 代码位置（本轮 dirty 工作树，不是 HEAD 已提交内容） |
|---|---|
| 来源类型/字节 `saved` | `shared/platform-clipboard-composite.rs:97`（`classify`）、`:206`（`read`）；macOS 采集循环 `macos/src-tauri/src/clipboard.rs:125`（`start_monitor`）、`:196`（`clipboard_loop`） |
| helper | `macos/src-tauri/clipboard-helper.swift:18/:651`；`shared/platform-clipboard-composite.rs:516/:564/:606`；`macos/src-tauri/src/clipboard_file.rs:23` |
| 加密 DB `db` | `shared/rust-core/src/db/composite.rs:930`（`add_composite_event`）、`:955`（`add_native_snapshot`）、`:588`（`commit_composite_archive`）；macOS 落库入口 `macos/src-tauri/src/clipboard.rs:837/:856` |
| 内容分析 `analysis` | `shared/rust-core/src/composite/content_analysis.rs:260`（`analyze`，`:10` 为 `ANALYSIS_VERSION`）；运行期 `shared/tailsync-runtime/src/content_analysis.rs:19`；读取入口 `shared/rust-core/src/db/composite.rs:1631`（`prepare_content_analysis`） |
| 列表 `list` | `shared/tailsync-runtime/src/history_display.rs:56`（`rebuild_display_batch`）、`:113`（`page_display_async`）、`:135`（`entries_display_async`） |
| 预览 `preview` | `shared/tailsync-runtime/src/history_display.rs:147`（`image_payload_checked_async`）；`shared/tailsync-runtime/src/document_preview.rs:595`（`load_document_preview`）；Swift 侧 `macos/swift-ui/Sources/TailSync/Services/HistoryPreviewViewModel.swift` |
| RestorePlan `restore_plan` | `shared/rust-core/src/composite/restore.rs:7/:76`；`shared/tailsync-runtime/src/history.rs:264`（`prepare_restore_plan`） |
| 同目标粘贴 | `shared/tailsync-runtime/src/history.rs:711`（`restore_payload`）、`:1113`（`restore_payload_checked_async`）、`:151`（`restore_composite_async`）；`shared/platform-clipboard-composite.rs:217`（`write`）、`:281`（`write_native`）；Swift `macos/swift-ui/Sources/TailSync/Services/ApiClientHistory.swift:336`（`restoreEntry`） |

每个格子记录 `source_action_id`、`observation` 和 `result`（`ok`/`failed`/`partial`/`unavailable`/`unknown`）。真实失败用 `observed` + `failed`，不是把失败藏成未测；材料完整的失败也能进入人工审查，READY 不是成功。`not_observed`/`unknown` 的 stage/pair/condition 只允许 `result: unknown`，保持 INCOMPLETE。

`receipts` 是明确授权导出的公开测试文件（相对路径 + size + SHA256）。**任何声称 `observed`/`not_observed` 的格子都必须有 receipt**；没有回执的已观察为 `INVALID`。UI 截图不能独立证明 helper 原字节、加密 DB 或 RestorePlan；没有该层证据就留 UNKNOWN，禁止擅自导出用户 DB 来补齐。工具不解析/验证回执的业务正文，父/人工须逐层检查相同 capture/revision、原件摘要、首图与目标实测。

## 8. 直接 vs 历史，条件必须分开

- 直接/历史：见第 4 节，成对采集。
- 四种条件（`samples[].conditions`）必须**分别**记录，不能互相推导：
  - `immediate`：来源复制后立即恢复/粘贴。
  - `source_closed`：关闭 QQ（或退出该来源）后恢复。
  - `offline`：断网后恢复。
  - `restarted`：重启 TailSync 后恢复。
- 某个条件未测就写 `unknown`，得到 `INCOMPLETE`，不得用「立即粘贴正常」推断其它条件正常。

每个样本先记录来源直接粘贴基线、来源格式/原件摘要与对应 history entry/capture/revision；再通过历史原样恢复到同一个目标，核对文字、图片数量/红蓝顺序、分类与首图。关闭来源、断网、重启分别仍恢复同一历史条目，不能重新从 QQ 复制补图，不能用一次条件结果冒充另一条件。断网/关闭/重启动作须确认测试会话安全，不停止日常 App 或损失未保存数据。

若来源直接粘贴就只有文字，记录该 QQ 版本/动作的限制，不伪造 TailSync 完整图文；若可持久格式存在但中间层丢失，先绑定具体失效层再授权修复；若依赖临时资源，另评估 O04/O05 显式授权能力，文件桥不自动代表 QQ 支持。

## 9. 系统提示归因（A02）

- 提示必须**单独**记录到 `notifications[]`，每条至少包含：
  - `original_text`：提示原文（照抄，不改写）。
  - `observed_at`：观察时间（含时区）。
  - `emitter`：发出应用（例如 `qq`、`tailsync`、`system`）。
  - `log_code`：实际可追溯的稳定日志码，保留大小写；没有就留空并标 UNKNOWN，不自创码。
  - `receipts`：至少一条授权证据文件（截图或日志导出）。
- 提示链路参考：`macos/src-tauri/src/api.rs:130`（`push_runtime_notification`）、`:176`（`runtime_notification_snapshot`）、`macos/src-tauri/src/commands/storage.rs:63`（`tauri_plugin_notification`）。
- 没有采集到提示就记 UNKNOWN（`I_MISSING_NOTIFICATION`），**不推断**「没有提示 = 正常」。
- 提示记录还须带 `session_id`、已登记样本的 `source_action_id` 和 `observation`；时间必须是实际的 ISO 8601 带时区时间。稳定码保留实际大小写，例如当前生产 `capture_failed`/`capture_not_saved`，不编造 `TS-NOTIFY-001` 来填门禁。无法归因或没有稳定码时保持 INCOMPLETE。
- 如果确实在明确观察时段内没有出现提示，可以记 `observation: not_observed`，附时段回执与 `observed_at`，`original_text`/`emitter`/`log_code` 留空；无需虚构提示或发出方。此材料可待人工审查，但 A02 仍是 UNKNOWN，不能证明所有成功与失败路径都正确。
- 合成样本**不能**用于验证真实 A02。
- 不给出「关闭通知或关闭权限提示来消除误报」的建议：权限提示是否必要要按事实判断，不能被静默。

## 10. 清单 schema（`tailsync-s2-06-evidence/v1`）

顶层字段：

| 字段 | 类型 | 说明 |
|---|---|---|
| `schema` | string | 固定 `tailsync-s2-06-evidence/v1` |
| `run_id` / `session_id` | string | 本次运行与会话标识 |
| `provenance` | enum | `real_qq` 或 `synthetic`；`synthetic` 永不 READY |
| `authorization` | object | `public_test_content`(bool)、`consent_confirmed`(bool)、`scope`(enum) |
| `environment` | object | 见第 3 节 |
| `samples` | array | 见下 |
| `notifications` | array | 见第 9 节 |

样本字段：

| 字段 | 类型 | 说明 |
|---|---|---|
| `sample_id` | string | 唯一 |
| `run_id` / `session_id` / `source_action_id` | string | 绑定顶层运行/会话和本次唯一复制动作 |
| `case` | enum | `single_image`/`text_plus_one`/`text_plus_two`/`selection`/`menu` |
| `source_inventory` | object | `source_action_id`、`declared`(不重复的 string 数组)、`receipts`(非空) |
| `stages` | object | 七个 key：`saved`/`helper`/`db`/`analysis`/`list`/`preview`/`restore_plan` |
| `targets` | object | 两侧含 `source_action_id`、`observation`、`result`、`receipts`、非空 `target_app`/`target_version` |
| `conditions` | object | 四个 key：`immediate`/`source_closed`/`offline`/`restarted` |

receipt 结构（相对 `--evidence-root` 的路径）：

```json
{ "path": "sample-single_image/stage-saved.png", "size": 12345, "sha256": "<64 位小写十六进制>" }
```

结构示意（节选，`...`/hash 占位不是可运行清单；未知字段会拒绝）：

```json
{
  "schema": "tailsync-s2-06-evidence/v1",
  "run_id": "s2-06-qq-2026-10-08-a",
  "session_id": "user-session-1",
  "provenance": "real_qq",
  "authorization": { "public_test_content": true, "consent_confirmed": true, "scope": "public_test_only" },
  "environment": {
    "source_app": "qq", "source_version": "9.9.1", "os_version": "macOS 15.x",
    "target_app": "QQ", "target_version": "9.9.1",
    "helper_sha256": "<64 hex>", "candidate_sha256": "<64 hex>", "source_sha256": "<64 hex>"
  },
  "samples": [
    {
      "sample_id": "sample-single_image", "case": "single_image", "run_id": "s2-06-qq-2026-10-08-a", "session_id": "user-session-1", "source_action_id": "copy-1",
      "source_inventory": { "source_action_id": "copy-1", "declared": ["image"], "receipts": [ { "path": "sample-single_image/inventory.txt", "size": 12, "sha256": "<64 hex>" } ] },
      "stages": { "saved": { "source_action_id": "copy-1", "observation": "observed", "result": "ok", "receipts": [ { "path": "sample-single_image/stage-saved.png", "size": 1024, "sha256": "<64 hex>" } ] }, "...": "其余阶段同理" },
      "targets": {
        "direct": { "source_action_id": "copy-1", "observation": "observed", "result": "ok", "receipts": [ { "path": "sample-single_image/target-direct.png", "size": 2048, "sha256": "<64 hex>" } ], "target_app": "QQ", "target_version": "9.9.1" },
        "history": { "source_action_id": "copy-1", "observation": "observed", "result": "ok", "receipts": [ { "path": "sample-single_image/target-history.png", "size": 2048, "sha256": "<64 hex>" } ], "target_app": "QQ", "target_version": "9.9.1" }
      },
      "conditions": { "immediate": { "source_action_id": "copy-1", "observation": "observed", "result": "ok", "receipts": [ { "path": "sample-single_image/cond-immediate.txt", "size": 8, "sha256": "<64 hex>" } ] }, "...": "source_closed/offline/restarted 同理" }
    }
  ],
  "notifications": [
    { "notification_id": "notice-1", "session_id": "user-session-1", "source_action_id": "copy-1", "observation": "observed", "original_text": "实际提示原文", "observed_at": "2026-10-08T10:00:00+08:00", "emitter": "tailsync", "log_code": "capture_failed", "receipts": [ { "path": "notifications/notice-1.png", "size": 4096, "sha256": "<64 hex>" } ] }
  ]
}
```

用 `--template` 生成空白模板（不含任何采集数据）：

```bash
python3 scripts/validate-s2-06-evidence.py --template > /tmp/s2-06-template.json
```

空白模板校验结果是 `INCOMPLETE`（退出码 3），**不是** READY，也不会被当成 fixture 证据。

## 11. 工具用法与输出

校验（只读，不联网、不起子进程、不读剪贴板/DB/缓存）：

```bash
python3 scripts/validate-s2-06-evidence.py \
  --manifest /path/to/manifest.json \
  --evidence-root /path/to/evidence-root
```

- `--manifest`：必须位于显式 evidence root 内的普通清单文件（必填，`--template` 除外）。
- `--evidence-root`：所有 receipt 相对路径必须落在该目录内（必填，`--template` 除外）。
- `--template`：向 stdout 打印空白模板后退出 0，不能与校验参数同时使用。

输出是一行有界的 JSON 摘要（只有固定枚举、错误码和计数，**不含**文件路径、文件内容或输入文本）：

```json
{"cases_present": ["menu","selection","single_image","text_plus_one","text_plus_two"], "class": "READY_FOR_MANUAL_REVIEW", "incomplete_codes": [], "invalid_codes": [], "provenance": "real_qq", "real_a02": "UNKNOWN", "real_a07": "UNKNOWN", "release": "NO-GO", "s2_complete": false, "schema": "tailsync-s2-06-evidence/v1"}
```

退出码：

| 退出码 | 类别 | 含义 |
|---|---|---|
| `0` | `READY_FOR_MANUAL_REVIEW` | 结构完整、字节长度/hash 匹配，可交人工评审（**非**真实来源或验收证明） |
| `2` | `INVALID` | 输入非法（类型/枚举/重复/矛盾/路径/预算/哈希等） |
| `3` | `INCOMPLETE` | 结构合法但缺样本/阶段/成对目标/条件/提示，或 provenance 为 synthetic |

### 错误码

`INVALID`（`invalid_codes`）：`E_ARGUMENTS`、`E_SCHEMA`、`E_TYPE`、`E_ENUM`、`E_DUPLICATE`、`E_CONTRADICTION`、`E_TARGET_MISMATCH`、`E_MANIFEST_NOT_FILE`、`E_MANIFEST_TOO_LARGE`、`E_MANIFEST_JSON`、`E_EVIDENCE_ROOT`、`E_RECEIPT_PATH`、`E_RECEIPT_SYMLINK`、`E_RECEIPT_MISSING`、`E_RECEIPT_NOT_FILE`、`E_RECEIPT_OUT_OF_ROOT`、`E_RECEIPT_SIZE`、`E_RECEIPT_HASH`、`E_BUDGET_FILE`、`E_BUDGET_AGGREGATE`、`E_BUDGET_COUNT`。

`INCOMPLETE`（`incomplete_codes`）：`I_IDENTITY`、`I_ENVIRONMENT`、`I_AUTHORIZATION`、`I_PROVENANCE_SYNTHETIC`、`I_SOURCE_INVENTORY`、`I_MISSING_CASE`、`I_MISSING_STAGE`、`I_UNKNOWN_STAGE`、`I_MISSING_PAIR`、`I_UNKNOWN_TARGET`、`I_MISSING_CONDITION`、`I_UNKNOWN_CONDITION`、`I_MISSING_NOTIFICATION`、`I_NOTIFICATION`。

父二修补充：`E_FIELDS`、`E_ACTION_MISMATCH`、`E_SESSION_MISMATCH`、`E_RECEIPT_EMPTY`、`E_RECEIPT_CHANGED`、`E_SECURE_IO_UNAVAILABLE`；缺项码 `I_ACTION_BINDING`、`I_SAMPLE_BINDING`、`I_TARGET_IDENTITY`。manifest 路径/字节问题输出固定 `E_MANIFEST_NOT_FILE`/`E_MANIFEST_TOO_LARGE`/`E_MANIFEST_JSON`，不暴露内部路径或 traceback。空 receipt 不是证据；提示原文允许真实换行/制表符，不要求改写。

### 安全与预算约束

- receipt 路径必须**相对**、位于 `--evidence-root` 内；拒绝绝对路径、URL、`..` 穿越、反斜杠、符号链接、非普通文件与越界读取。
- 单文件 ≤ 16 MiB、累计申报引用 ≤ 64 MiB、总引用 ≤ 128、单格引用 ≤ 32；样本/提示各 ≤ 64，清单 ≤ 64 KiB。读取前检查累计预算，再核对已打开 fd 的实际长度并有界流式读取；文件增长/截断/修改拒绝，最多多读一字节作超界探测。
- 用 POSIX root directory fd、逐层 `O_NOFOLLOW`、普通文件 `fstat` 和 `O_NONBLOCK` 防止路径检查后被换成符号链接或 FIFO。显式 root 会规范化系统父目录；root 内每级链接和 manifest 链接均拒绝。不支持这些安全原语的宿主 fail-closed，Windows 上运行本校验器尚未验证；可在 macOS 校验显式导出的 Windows 公开测试材料。
- 重复 JSON key、非标准数值和未知字段拒绝；未确认授权时不打开任何 receipt。错误输出固定码，不回显输入正文、恶意 provenance、未知 CLI 参数或原提示。
- 只读取清单显式引用的文件，不做任何其它发现或采集。

## 12. 保留 UNKNOWN，不得伪造端到端诊断

- 只要缺真实 QQ 现场条件，就把对应样本/阶段/条件/提示写成 `unknown`，得到 `INCOMPLETE`；**不要**用「看起来相似」或其它 case 的结果补齐。
- 用户报告、已复现根因、自动测试、完整 GUI、真实 QQ、Windows、双设备、CI 分别登记；本清单只覆盖「真实 QQ 现场」这一类。
- 本工具与指南都不产出 Release GO 或 S2 完成结论；最终判定由父级/人工评审给出。

## 13. 停止条件

完成「真实 QQ + 系统提示」现场采集与清单校验、并把 `INCOMPLETE`/`INVALID` 处理到只剩确实无法取得的 UNKNOWN 之后停止，交给父级评审。不要顺手做 S2-07 的整体封存，也不要改动产品代码。
