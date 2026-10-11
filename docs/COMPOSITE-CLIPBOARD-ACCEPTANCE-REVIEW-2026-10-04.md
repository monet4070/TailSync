# TailSync 图文复合剪贴板验收评审（2026-10-04）

**状态**：一次性验收评审快照，尚未修复。本文记录问题、证据与根因判断，不改变任何生产代码。

**受审对象**：worktree `/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，
分支 `codex/composite-clipboard`，基线提交 `f719d58`（继承 Phase 3.5 / Phase 4B 与历史键盘导航）。

**受审变更**：全部为**未提交**的工作树改动——101 项变更（约 +3,473 / −295 行）加约 3,800 行新增文件，
横跨协议、Core、DB、原生 I/O、打包、Swift/React UI 六层。

**源码指纹**：`68b500c60d2add0947b07126bc5318ff46c662492ddb9a93bc1e3c4e8003d0b3`
（按 `artifacts/composite-evidence/source-fingerprint.json` 声明的 `hash_method` 独立复算，713 文件逐一匹配，零漂移）

**评审方式**：6 路独立子代理分区对抗审查（协议/安全、Core 可靠性、存储/迁移、原生 I/O/打包、UI/契约、证据审计）
→ 2 路对抗裁决员交叉核实（专责**推翻**结论）→ 评审人本人复算全部可复现证据。

**环境**：macOS 27.0.1 / arm64 / rustc 1.91.0 / node v26 / bash 3.2.57（`/bin/bash`）

> 说明：本文位于 `docs/`，而 `docs/` 属于指纹的 `excluded_prefixes`，因此新增本文档**不会**使
> `source-fingerprint.json` 或 `verification.json` 失效。

---

## 〇、总体判断

| 维度 | 评价 | 依据 |
|---|---|---|
| 后端设计（协议 / Core / 存储） | **A−** | 分层正确、有界设计系统化、因果模型优雅、硬不变量有测试 |
| 证据与诚实度 | **A** | 指纹与包哈希可独立复算、真机项一律 UNKNOWN、主动声明指标适用范围 |
| 前端 / UI 完成度 | **C+** | 零 CSS、多图只发第一张、部分成功报成失败、macOS 无 a11y/键盘路径、Swift 零测试 |
| 默认路径可用性 | **F** | 默认（feature 关闭）macOS 构建脚本在系统 bash 下直接中断 |
| **可发布性** | **不通过** | 1 个 Critical + 不可逆 DB 升级 + 一族"静默丢失"路径 |

**结论**：工程质量总体很高，但**当前状态不能验收通过**。阻断项不是设计缺陷，而是 5 个可精确定位、
多数只需一行修复的实现问题。修完 §五 第一阶段后可进入真机验收阶段。

**一句话根因**：这不是"富文本写砸了"，而是**一次同时跨越三个既有抽象从未设计过的边界的变更
（持久化 schema / 外部进程生命周期 / 不可逆且结果未知的原生副作用），用一套为单界设计的抽象去承载，
并且验收矩阵只覆盖了开启态**。详见 §三。

---

## 一、独立复现的证据

本节全部由评审人实跑，**不是转述实施记录**。

### 1.1 测试矩阵（全绿，数字与实施记录完全一致）

| 检查 | 命令 | 实测结果 |
|---|---|---|
| Core（3 扩展，排除 iroh） | `cargo test -p tailsync-core --features 'protocol-composite-clipboard protocol-file-sliding-window protocol-image-compressed-chunks' --lib -- --skip iroh_transport` | 540 passed / 4 ignored / 12 filtered / **0 failed** |
| Core 默认 feature | `cargo test -p tailsync-core --lib -- --skip iroh_transport` | 540 passed / 4 ignored / 12 filtered / 0 failed |
| Protocol / Runtime | `cargo test -p tailsync-protocol -p tailsync-runtime --features tailsync-core/protocol-composite-clipboard` | 33 / 24 passed |
| macOS Rust | `cargo test --manifest-path macos/src-tauri/Cargo.toml --features protocol-composite-clipboard --lib` | 116 passed / 3 ignored |
| Windows crate（host） | 同上 `--manifest-path windows/src-tauri/Cargo.toml` | 125 passed / 2 ignored / 2 warnings |
| Swift | `swift test --package-path macos/swift-ui` | 250 executed / 4 skipped / **0 failures** |
| React / Vitest | `npm --prefix windows test` | 267 passed / 51 files |
| 契约 | `node scripts/check-local-contracts.mjs` | 77 Windows + 45 macOS 命令分类通过 |
| 跨平台漂移 | `node windows/scripts/check_cross_platform_sync.mjs …` | 通过 |
| Windows helper MSVC | `cargo check -p tailsync-clipboard-helper --features portable-check --target x86_64-pc-windows-msvc` | 通过 |

所有运行均经 `node scripts/run-isolated-tests.mjs` 隔离数据目录，未触碰日常用户数据。

### 1.2 证据完整性（全部可独立复算）

| 项目 | 复算结果 |
|---|---|
| 源码指纹（713 文件） | 逐文件 sha256 比对：**0 missing / 0 mismatch**；聚合哈希与记录一致 |
| 当前 git 文件集 vs 记录集 | 完全相同（无新增/无删除） |
| 候选包（8 文件） | 逐文件 sha256 **8/8 匹配**；聚合 `d72ebb18b633cb1a8aa78a84102afc8b7530fb3d05d82e7b2f42a0ef3a6f0eb4` 一致 |
| 包元数据 | arm64、`LSMinimumSystemVersion=13.0`、ad-hoc 签名（`satisfies its Designated Requirement`） |
| 证据日志 | `verification.json` 引用的 **21/21 日志存在且 sha256 匹配** |

### 1.3 原生行为（实跑）

包内 helper 合成探针：

```bash
python3 tools/clipboard-probe/probe.py \
  --helper artifacts/TailSync-Composite.app/Contents/MacOS/clipboard-helper --self-test
```

实测输出（与实施记录一致）：

```json
{"result":"passed","fixtures":5,
 "pasteboard":"tailsync-probe.<uuid>",
 "verified":["text+HTML","text+HTML+PNG alpha","RTF","embedded two images",
             "multiple native items","operation revision","superseded before write","malformed before clear"],
 "general_clipboard_modified":false,"target_app_acceptance":"UNKNOWN"}
```

10,000 次来源因果回环：

```bash
node scripts/run-isolated-tests.mjs -- cargo run -p tailsync-core \
  --example composite_origin_probe --features protocol-composite-clipboard -- \
  "$PWD/artifacts/TailSync-Composite.app/Contents/MacOS/clipboard-helper" 10000
```

实测：

```json
{"self_writes":10000,"unexpected_echoes":0,"equal_user_copies":10000,
 "elapsed_ms":4863,"general_clipboard_modified":false,"target_app_acceptance":"UNKNOWN"}
```

**即：来源因果模型在本地合成条件下成立**——10,000 次自写零回音，且 10,000 次"同内容的新用户复制"
全部正确识别为外部（未被自写标记吞掉）。探针只使用 `tailsync-probe.` 前缀的命名 pasteboard，
未修改用户通用剪贴板。

### 1.4 阻断缺陷复现（P0）

见 §2.A，已在 `/bin/bash` 3.2.57 上直接复现。

---

## 二、问题清单

严重度定义：🔴 P0 阻断发布；🟠 P1 发布前必须处理；🟡 P2 应在合并前处理；🟢 P3 可排期。
标注 **[已复现]** 的条目由评审人实跑验证，其余为静态阅读 + 裁决员交叉核实。

### 2.A 🔴 P0：默认构建脚本直接中断 **[已复现]**

`macos/build-mac.sh:16-19` 新增：

```bash
CARGO_FEATURE_ARGS=()
if [[ "${TAILSYNC_COMPOSITE_CLIPBOARD:-0}" == "1" ]]; then
    CARGO_FEATURE_ARGS=(--features protocol-composite-clipboard)
fi
```

随后在 `set -euo pipefail`（脚本第 2 行）下，于第 51、58 行展开 `"${CARGO_FEATURE_ARGS[@]}"`。
**bash < 4.4（macOS 系统 `/bin/bash` 3.2.57，也正是本脚本 shebang 指定的解释器）对空数组展开报
`unbound variable`**，`set -e` 立即退出。

复现（`/bin/bash` 3.2.57）：

```
before expansion
argc=0
./empty-arr-test.sh: line 9: CARGO_FEATURE_ARGS[@]: unbound variable   → exit 1（cargo 从未执行）
```

**触发条件**：`TAILSYNC_COMPOSITE_CLIPBOARD` 未设为 `1`——即**默认路径**。

**影响面**：

| 入口 | 位置 |
|---|---|
| README 记载的本地构建 | `README.md:172` / `README.zh-CN.md:172` 的 `./build-mac.sh` |
| DMG 构建 | `macos/build-dmg.sh:54` |
| macOS 发布验证脚本 | `macos/scripts/verify_macos_release.sh:8` |
| CI 打包门禁 | `.github/workflows/ci.yml:304`（未设该环境变量） |

**为何逃过所有本地证据**：`verification.json` 的打包检查只跑了
`TAILSYNC_COMPOSITE_CLIPBOARD=1`（非空数组）路径；而 `root-feature-off` / `macos-feature-off`
那些 PASS 是 `cargo check`，覆盖不到 shell 脚本。这是验证矩阵的真实盲区，不是偶发。

**修复**（一行）：

```bash
cargo build --locked --release ${CARGO_FEATURE_ARGS[@]+"${CARGO_FEATURE_ARGS[@]}"} …
```

或改用字符串拼接 / `set --` 形式，避免空数组展开。

> 注：`bash` 在 GitHub macOS runner 上默认解析为 `/bin/bash`（3.2），故 CI 大概率同样失败；
> runner 是否安装了更新的 bash 未经实测，不作断言。

### 2.B 🟠 P1：schema v12 不可逆升级且未门控，"默认关闭"给错安全感

三条事实叠加：

1. `protocol-composite-clipboard` 是**空 feature**：`shared/rust-core/Cargo.toml:57` 定义为 `= []`，
   `shared/rust-core/src/lib.rs:5` 的 `pub mod composite;` 无 `cfg`。
   **全仓唯一** `#[cfg(feature = "protocol-composite-clipboard")]` 位于
   `shared/platform-network-server.rs:1759`，且那是一个**测试**。其余 3 处是运行期 `cfg!()` 布尔。
   → 协议编解码、DB 层、投递逻辑**进入每一个构建**。
2. v12 迁移与建表**完全无条件**：`shared/rust-core/src/db/migrations.rs:422`、`db/schema.rs:46-57`、
   `db.rs:61`（`SCHEMA_VERSION = 12`）。`db/open.rs:41` 无条件调用 `backup_before_composite_upgrade`，
   在用户数据目录写入 `history-before-v12.db`。
3. 旧二进制拒绝 v12：`ensure_supported_schema`（`db/open.rs:253-277`）在基线即存在
   （`f719d58:shared/rust-core/src/db/open.rs:239`，当时 `SCHEMA_VERSION = 11`）。

**后果**：任何用户——包括使用 feature 关闭的默认构建、甚至设了
`TAILSYNC_DISABLE_COMPOSITE_CLIPBOARD` 的用户——只要打开一次应用，生产库即被**不可逆**升到 v12，
**无法再回退到已发布的 v2.3.2**。

文档在 `COMPOSITE-CLIPBOARD-EXECUTION-2026-10-03.md` 的括号里承认了这一点（"schema v12 迁移并不受该开关
控制，回退旧二进制必须使用升级前备份"），但：M3 行仍标 PASS，`verification.json` 的 `unverified`
列表未收录该不可选退出副作用。

**修复**：二选一——(a) 把 v12 块纳入 feature gate（关闭时不写 12）；(b) 接受不可逆升级，但把
"升级 + 回滚流程"升级为发布说明中的显式风险项与 Go/No-Go 条件。

### 2.C 🟠 P1：`validate()` 二次复杂度 CPU 放大 DoS（已配对 peer 可达）

`shared/tailsync-protocol/src/composite.rs:188` 把 `std::str::from_utf8(&r.bytes)`（整块 HTML，
上限 2 MiB，见 `composite.rs:40` 的 `Mime::limit()`）**放在 bindings 循环体内**；
第 190 行还每条 binding 执行一次 `format!("tailsync-resource:{}", …)` 分配。

裁决员核实后**加重**了这条：

- bindings 上限 **5383**（`decode` 只要求 `16 + (reps+resources)*44 + bindings*12 <= MANIFEST_LIMIT`，
  即 65536；`MANIFEST_LIMIT` 是唯一上界）。
- 攻击者只需在 2 MiB HTML 内放 5383 个互不重叠的 18 字节字面量（约 97 KB），其余 ASCII 填充；
  `ranges.insert`（`:195`）与重叠检查（`:200-206`）均能通过。
- 单次 `validate()` ≈ 5383 × 2 MiB ≈ **10.9 GB** UTF-8 扫描。
- **接收路径上 `validate()` 实测被调用 8 次**：`chunks.rs:240` 的
  `validate_for_publish(&PortableBundle::decode(&raw)?)` 贡献 2 次；`composite_receiver.rs:35/36` 各 1 次；
  `:41` 的 `digest()` → `encode()` → `validate()` 1 次；`:74` 发布 1 次；`:106` 事件入库 1 次
  （`db/composite.rs:165`）、其内 `digest()` 再 1 次（`:169`）。
- `chunks.rs:51` 用**压缩后**长度决定分片数，而 `raw_len` 独立可达 `BODY_LIMIT`（`chunks.rs:47`）
  → 一个 **1–3 KB** 的 `CompositeChunk` 即可解出 2 MiB 原始包。
- `check_peer_chunk_bytes`（`platform-network-server.rs:543-548` / `rate_limit.rs:70-75`）只按
  **wire 字节**计费，且只有 index=0 消耗事件令牌（30 burst / 2 每秒）→ 单个 peer 首秒可发起约 30 次
  × 8 次 validate ≈ 数分钟 CPU。
- 8 次中有 1 次（`composite_receiver.rs:41`）落在 **tokio 异步 worker** 上，并发连接可各自占住一个 worker。

**可达性限定**：必须**已配对且协商出 composite 能力**（`platform-network-server.rs:477-483` 的
`peer_is_allowed` 在进入接收循环前执行；`secure.rs:388-392` 要求协商）。**不是授权前可达**，
故不是未认证 DoS，而是"恶意/有 bug 的已配对设备"型缺陷。

**修复**（一行 + 两处小改）：把 `from_utf8` 提出循环；用预计算前缀比较替代每条 `format!`；
给 bindings 加显式上限；把 `raw_len` 计入 peer 字节预算或限制压缩比。

### 2.D 🟠 P1：一族"静默丢失"路径，违反项目自身的可见性原则

| 子项 | 机制 | 后果 |
|---|---|---|
| D-1 helper 缺失/被杀 | `shared/platform-clipboard-composite.rs:12-15` 的 `enabled()` 只看编译期 feature 与 env，**从不检查 helper 是否存在**；一旦 enabled，除 `CaptureError::Files` 外所有失败分支都 `continue`，绕过 legacy 检测器（`macos/src-tauri/src/clipboard.rs:247-375`、`windows/src-tauri/src/clipboard.rs:255-383`） | 文本/图片/文件同步**全部失效**，UI 无任何提示，却仍向对端通告 composite 能力（`secure.rs:87`） |
| D-2 读超时 | 5s 超时重试 3 次后直接 `acknowledge(observed)`（同文件读路径，macOS `~:347`） | 该次剪贴板变更被**永久丢弃**：无历史、无同步、无错误 |
| D-3 assembly 被抢占 | `composite/chunks.rs:142-153` 的 `accept_with_revision` 只要 `inner` 出错就 `active = None`；`:191` 的 `a.meta != m` 使"交错到达的另一条传输"被判协议错误，同时丢弃在途 assembly | 在途传输被丢弃 + 新传输被拒 |
| D-4 assembly 预算耗尽 | `chunks.rs:104-113` 的 `ASSEMBLIES` 全局上限 4，`:187` 的 `Lease::acquire()?` 失败 → `AdmissionRejected`（`platform-network-server.rs:743-745` 回 PeerError） | 一个已配对 peer 开 4 条连接各发一个 index=0 分片后静默，即可让其他所有对端 30–60s 内的复合同步被拒 |
| D-5 丢失不上报 | `peer/delivery.rs:524-531` 的 `record_permanent_delivery_warning` 只处理 `CompositeResult` 与 `Expired`，**不处理 `Rejected`** | D-1~D-4 造成的丢失对用户**完全不可见**，只有日志 |

D-3/D-4 的严重性在于**结果类型**：对端被归类为 `DeliveryError::Rejected`（`is_retryable() == false`），
即永久、不重试、不告警。D-5 使整族问题失去用户可见出口。

**修复**：`Rejected` 且携带剪贴板内容时必须落 `sync_warning`；assembly 预算耗尽应表现为可重试背压
（关连接或回可重试错误）并记 warning；message_id 变更应视为"抢占"而非协议错误。

### 2.E 🟠 P1：超时后回执缺失 → 自写回播 + 剪贴板可能被清空

**这是本次最值得记录的实现问题，因为它违反的是作者自己声明的不变量（REL-01），且仓库里已有正确做法。**

原生写入的实际顺序（`macos/src-tauri/clipboard-helper.swift`）：

```
165  let clearedRevision = pb.clearContents()      ← 剪贴板被清空
168  let wrote = pb.writeObjects(items)            ← 剪贴板被写入（不可逆副作用已发生）
171  emitReceipt(...)                              ← 回执最后才输出
```

而 Rust 侧 `shared/platform-clipboard-composite.rs:116`：

```rust
let receipt = PublishReceipt::decode(&run_helper("--write-snapshot", Some(input))?)?;
```

超时（硬编码 5s）时 `?` 直接返回，**不会执行 `:135-143` 的 `tracker().record(...)`**。于是本机剪贴板
已被修改、回执却缺失 → 下次轮询 `classify` 无匹配 receipt → `Origin::External`（`composite/origin.rs:64-70`）
→ **重新捕获、写入历史、再次广播**（回环 / 重复条目）。更糟的是超时路径会 `child.kill()`，
若落在 165–168 窗口，用户剪贴板被留空。

**并且新路径丢掉了一个仓库早就有的顺序纪律**：

| | 注册时机 | 失败补偿 |
|---|---|---|
| legacy `shared/rust-core/src/sync/clipboard.rs:112-118` | `shadow_filter.insert()` **在写入之前** | 写入失败则 `remove()` 回滚 |
| composite `sync/clipboard.rs:26` → `platform-clipboard-composite.rs:139` | `tracker().record()` **在写入成功之后** | 无 |

即 legacy 是"先登记意图 → 执行不可逆动作 → 失败则补偿"，composite 是"先执行 → 成功了才登记"。
**P1-E 正是这个顺序倒置的直接后果**，不是框架缺少能力。

**修复**：写入前先登记回执（或在超时后补一次 token 探测读并补记）；超时后不得把"结果未知"当作
"未发生"。

### 2.F 🟠 P1：最长 5s 的阻塞子进程调用持有全局 engine 锁

调用链：

```
peer/event_receiver.rs:188      let mut engine = sync_engine.lock().await;   ← 取全局锁
peer/event_receiver.rs:194      engine.handle_incoming_text(&text, …).await
sync/clipboard.rs:110-119       restore_text → self.platform()?.write_text(text)
macos/src-tauri/src/sync_adapter.rs:78-96   （feature 开启）clipboard_composite::write(...)
shared/platform-clipboard-composite.rs:147-234  run_helper：spawn 子进程 + sleep(10ms) 轮询，≤5s
```

`event_receiver.rs:188` 的 guard 覆盖到 `:201`，即**跨过了 `:194` 的 await**。于是单个对端连发
text 事件即可让整个引擎（本地捕获循环、其他对端 apply、composite publish 所需的 engine lock）
每次阻塞最长 5s，同时占住 tokio worker。

**注意不对称**：composite 自己的路径已正确使用 `spawn_blocking`（`composite_receiver.rs:34-40, 68, 104`），
**legacy 入站路径没有**。

**修复**：该调用移入 `spawn_blocking`，并在调用前释放 engine guard。

### 2.G 🟡 P2：其他已确认问题

| ID | 位置 | 问题 |
|---|---|---|
| G-1 | `windows/src/pages/history/HistoryItem.tsx:174` | 新按钮 `className="item-copy-format"` 在全仓 CSS 中 **0 处规则**（对照 `windows/src/styles/history.css:219` 的 `.icon-button` 有完整样式）→ 每个 composite 行渲染 WebView 默认按钮，行高不统一，新功能主入口视觉上是坏的 |
| G-2 | `shared/tailsync-runtime/src/history.rs:135` | png 恢复回退取 `bundle.resources.first()`，多图组**只发布第 1 张却返回 Ok**；徽标仍写 "images N"（静默部分内容） |
| G-3 | `shared/rust-core/src/sync/clipboard.rs:52-56` | `PublishState::Partial` 被转成硬 `Err`——剪贴板已部分写入却报"失败"，用户可能再次恢复并覆盖；peer 侧却有 `composite_partial` 专门语义（`sync_warning.rs:58`），两端不一致 |
| G-4 | `shared/tailsync-runtime/src/history.rs:121-136` | `restore_format` 分支早于 `native_only` 判断 → API 可绕过"只能在本机恢复"的承诺，拿到被 `Mime::Text.limit()` 截断的文本（UI 已隐藏菜单故当前不可达，但语义自相矛盾） |
| G-5 | `macos/swift-ui/Sources/TailSync/Views/HistoryRow.swift:257-264` | 菜单 label 只有图标，无 `.accessibilityLabel`，未暴露为 `.accessibilityAction`；`HistoryKeyboardNavigation` 的 `HistoryKeyboardCommand` 无格式变体 → VoiceOver 报未命名按钮，macOS 键盘用户完全无法使用"仅文字/仅图片"恢复（Windows 的 button 可 Tab） |
| G-6 | `macos/swift-ui/Sources/TailSync/Views/HistoryView.swift:962` | composite 恢复失败直接把 Rust 英文串经 `error.localizedDescription` 显示（`history.rs:117/127/136/184`）→ zh-CN 用户在新路径看到未翻译错误 |
| G-7 | `peer/delivery/executor.rs:527-533` | 每次重试重新 zlib 压缩整个 bundle（32 MB 事件 5 分钟内约 60 次重试 ≈ 数 GB 级 CPU/内存抖动）；`composite_fallback` 只缓存 legacy 路径 |
| G-8 | `composite/chunks.rs:43` vs `peer/delivery/worker.rs:46` | 事件时间窗 5 min vs 入队 TTL 5 min，但 64 分片 × 10s ACK ≈ 640s > 300s → 慢链路后段分片被判 `InvalidComposite` → PeerError → 永久丢弃；两窗口起点还不同 |
| G-9 | `platform-network-server.rs:543-548` | `CompositeChunk` 只按**压缩后**字节扣费，`raw_len` 独立可达 32 MB，压缩比无约束 → 64 MB 字节预算可换到近乎无限的 CPU/内存放大（P1-C 的廉价利用机制） |
| G-10 | `peer/composite_receipts.rs:46-56, 65-87, 137-146` | 单个 JSON 日志每事件读-解析-序列化-fsync 4~5 次；文件损坏或 >2MB 时返回永久错误且无修复路径 → 所有对端的 composite 接收永久失败（仅日志） |
| G-11 | `platform-clipboard-composite.rs:16-32` + `sync/clipboard.rs:18` | Windows `GetClipboardSequenceNumber() == 0` 时 `revision()` 返回 `None` → `publish_composite` 一律返回 `Superseded` → 用户看到不诚实的"被抢占"且内容永不落地 |
| G-12 | `peer/delivery/executor.rs:515-517` + `composite/mod.rs:227-241` | 对端无 composite 能力时 `legacy_content` 只取 Png/Text；仅有 Html/Rtf 的 bundle → `InvalidComposite` → `Rejected` → 整事件永久静默丢失 |
| G-13 | `peer/composite_receiver.rs:42` | 全局 `FINISH_LOCK` 覆盖整个原生 publish（含 5s 子进程）与 DB 写入 → 串行化所有对端的 composite apply，队头阻塞 |
| G-14 | `peer/composite_receiver.rs:60-98` | 原生命中"至多一次"：Claim 先持久化，若在 claim→publish 间进程死亡，重试只写 history 并回 `ApplyUnknown`，**永不重放**（claim 不移除）→ 内容永久丢失。属有意取舍，但未在文档中声明 |

### 2.H 🟢 P3：低severity 与代码卫生

| ID | 位置 | 问题 |
|---|---|---|
| H-1 | `db/lifecycle.rs:128-132, 163` | `trim("composite")` 是死代码（无调用者传 `"composite"`）；且因 `:129-132` 提前 return，`:163` 新增的 `\| "composite"` 分支**构造上永不可达**——作者的 composite 字节配额意图双重落空 |
| H-2 | `db/composite.rs:348` | `crypto::encrypt(&serde_json::to_vec(&m)?)?` 的 `?` 绕过 `:344-347` 的 `cleanup_external_payloads(&written)`（无 Drop 守卫）→ 外置部件残留至 1–1.5h 后被 `sweep_orphan_payloads` 回收（内容寻址，重试会复用） |
| H-3 | `db/preview.rs:30-34` + `db/reads.rs:416` | `get_preview_metadata` 对 composite 强制 `size_bytes = 0`，`prepare_preview` 覆写为 `summary.len()`；该 0 是**为绕开 64MB 预览上限的无注释承重哨兵**（`db.rs:64` 的 `PREVIEW_MAX_BYTES`），直接调用语义不一致（生产路径不可观测） |
| H-4 | `db/storage.rs:216-242` + `db/composite.rs:559-563` | 配额预检中每行一次 `prepare+query`（`oldest_evictable_external_payload_id` 对每个 composite 行调 `composite_part_payloads`），且全程持 DB 锁 → 纯 composite 历史退化为 O(n) 次 SQL 往返。本次改动新引入 |
| H-5 | `platform-clipboard-composite.rs:186-197` | `?` 提前返回时 `Child` 未 `wait()`（僵尸）；stdin 写线程从不 join 且长期持有整份 payload |
| H-6 | `platform-clipboard-composite.rs:182, 230` | `stderr(Stdio::null())` 丢弃全部 helper 诊断；超时文案对读路径也写 "publish outcome unknown" |
| H-7 | `clipboard_file.rs:342` vs `platform-clipboard-composite.rs:152` | 两套不同的 helper 路径解析实现（前者 canonicalize 且优先 bundle），存在漂移风险 |
| H-8 | `platform-clipboard-composite.rs:162-167` vs `windows/scripts/build-composite-helper.ps1:9-11` | debug 回退路径是 `CARGO_MANIFEST_DIR/tailsync-clipboard-helper.exe`，而脚本只产出 `binaries/tailsync-clipboard-helper-<triple>.exe`，无人拷贝 → Windows `tauri dev` + feature 开启时永远找不到 helper（叠加 D-1 即完全无同步） |
| H-9 | `macos/src-tauri/clipboard-helper.swift:62, 72` | `items.count <= 16` / `count < 64` 直接 `exit(65)`；Finder 复制 17+ 文件即触发，叠加 D-1 连 legacy 文件同步也不执行 |
| H-10 | `tools/clipboard-helper/src/windows.rs:312-319` | ~~PNG-only 发布时补写 CF_DIBV5 但**从不写 CF_DIB** → 只认 CF_DIB 的老消费者（部分 Office/Explorer 路径）粘贴不到图~~ **2026-10-04 R5-07 更正：此推断不成立。** Microsoft《Clipboard Formats》把 `CF_DIBV5 → CF_DIB` 列在 *Synthesized Clipboard Formats* 表内，并明确写道「若系统为某格式提供了自动类型转换，把转换格式一并放上剪贴板没有好处」；该页也建议复制位图时放「CF_DIB 或 CF_DIBV5」（二者之一即可，系统按需渲染另一个）。因此**缺少 CF_DIB 本身不能作为粘贴失败的解释**，只认 CF_DIB 的老消费者仍能粘贴。真正的结论是：补兼容 DIB 必须有**具名目标应用的实测失败**（APP-07，仍为 UNKNOWN，属 R7）；当前 Windows 兜底 DIB 只在**已观察到的主路径失败**后触发，并由 `scripts/check-windows-dib-policy.sh` 结构性地守住。参见 [Clipboard Formats](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats)。 |
| H-11 | `tools/clipboard-helper/src/windows.rs:167-253` | 整个 snapshot 读取期（含 `:226-250` 的 DIB→PNG 编码，最多 8M 像素）都持有 Clipboard guard，直到 `:253` 才 drop → 编码期间阻塞其它进程 `OpenClipboard`；`OpenClipboard` 仅重试 25×10ms |
| H-12 | `tools/clipboard-helper/Cargo.toml:10, 14` | `blake3` 在本 crate 中从未被引用，`portable-check` 只是借 feature 强制传递依赖 `blake3/pure`；而打包脚本 `:7` 不传该 feature → 无 C 工具链主机无法对 helper 做类型检查。属隐式耦合 |
| H-13 | `shared/rust-core/src/sync_warning.rs:17, 112-125` | 只有单个全局"最新 warning"槽位 → 一批 superseded/incomplete 结果互相覆盖，并非每个对端的结果都被诚实呈现 |
| H-14 | `macos/src-tauri/src/clipboard.rs:270` | `classify` 在 `before != after` 时返回 `Unknown`，调用方只跳过 `SelfWrite` → **`Unknown` 被当作 `External` 广播** |
| H-15 | `shared/tailsync-runtime/src/history.rs` / UI | 标签"格式/formats"后面打印的是 `representation_count`（`HistoryItem.tsx:168`、`HistoryRow.swift:121`），真正的去重格式列表 `formats` 从不展示 → text+2×png 显示"格式 3"而实际 2 种 |
| H-16 | `windows/src/pages/history/HistoryItem.tsx:169, 176` | 分隔符 `" · "` 硬编码在视图里；动态键 `t(\`history.composite.copy.${format}\`)` 绕过键对齐检查（新增格式漏翻译只会静默显示原始 key）；Swift 固定打印 "images 0" 而 React 会省略该段 |
| H-17 | `shared/schema/local-contract.schema.json:73` | `CompositeInfo` 的 description 是 "A clipboard history entry."（源自 `db/types.rs:50` 的复制粘贴注释），`HistoryEntry` 反被清空；契约静默删除 `PreviewKind` 定义与生成的 `decodePreviewKind`/`ContractPreviewKind`，而 `LocalCapabilities.schema_version` 仍为 1（未做版本提升）。仓内无消费者，影响限于仓外 codegen 客户端 |
| H-18 | `macos/src-tauri/src/api/routes.rs:303-306` | sync_engine 锁被持有跨越 `write_clipboard_files`（spawn helper 并等待至超时）；`commands/storage.rs:168-171` 做同样的事却提前 drop → 文件批恢复期间 macOS 剪贴板监控与 peer 同步被阻塞（非死锁，是停顿） |
| H-19 | `shared/rust-core/src/composite/origin.rs:34-52` | 回执仅保留 128 条 / 60s；若某次自写在捕获轮询前被淘汰则归为 `External` → 回播自己的写。实际不可达（每次写需 spawn helper），属推测 |
| H-20 | 摘要命名空间（推测） | 写入时回执 digest 由**合成** snapshot（`to_macos_snapshot`/`to_windows_snapshot`）经 `from_native` 计算，分类时由**回读** snapshot 计算；表示顺序差异、OS 附加 flavor（如 Windows helper 自动补的 CF_DIBV5）或 materialize/extract_resources 归一化差异都会让自写被判 `External` 并回播。往返稳定性仅在 `composite/html.rs:418-455` 的单个双图 HTML 用例被断言 |

### 2.I 📋 文档与验证流程

| ID | 位置 | 问题 |
|---|---|---|
| I-1 | `windows/scripts/check_cross_platform_sync.mjs:166-172` | canonical include 注册表**未登记** `shared/platform-clipboard-composite.rs`（两端 `clipboard_composite.rs:1` 都 include 它）→ 该 PASS 未覆盖这个新共享文件 |
| I-2 | `CONTEXT.md:8`、`README.md:143` | 仍写 "数据库 schema v11"，实际 `db.rs:61` 为 12；两文件本次均未修改 |
| I-3 | `docs/features/`、`docs/adr/` | 该功能无 `features/` 规格、无 ADR；仅有三份带日期的过程文档（且均未跟踪） |
| I-4 | `.gitignore:60`、`:70` | `**/logs/` 忽略全部 21 份证据日志，`/artifacts/TailSync-Composite.app/` 忽略候选包 → 一旦提交，所有 `log_sha256` 与包指纹**第三方无法验证**。需 `!artifacts/composite-evidence/logs/**` 反向规则或记录可复现构建配方 |
| I-5 | `verification.json` | `warnings` 字段非日志派生、不可靠（windows-host 记 3，实测 2；windows-feature-off 记 4，实测 3）。声称 `warnings: 0` 的检查经逐条确认成立 |
| I-6 | `COMPOSITE-CLIPBOARD-EXECUTION-2026-10-03.md:34, 41` | M0/M7 标 PASS，但计划 `:243`/`:250` 的退出条件要求真实 Chrome/Word/微信基线与 Mac↔Windows 双向验证。文档表头与同行第三列已充分限定（"尚需证据或实现"），`NATIVE-ACCEPTANCE:57` 更把 10,000 次远端回路单列为 `REL-01 UNKNOWN` → **属标注口径问题**：违反的是计划 `:252` 自己"未验收阶段保持 Pending/Unknown"的规则，不是隐瞒 |
| I-7 | 测试缺口 | 无"默认构建不得迁移到 v12"断言；无备份 ENOSPC 行为测试；无 v12 DDL+版本同事务断言（仅覆盖 v10）；无 crash-between-file-write-and-commit 孤儿恢复测试；无 `item-copy-format` 渲染测试；React 新测试 mock 的 `t` 直接返回 key（无法证明本地化）；**Swift 侧对 composite 零测试** |
| I-8 | 构建缓存 | `root-clippy` / `root-feature-off` 日志未显示 `tailsync-protocol`（新增 3 文件）与新工作区成员 `tools/clipboard-helper` 的 Checking。cargo 仅缓存未命中时打印，故不矛盾，但**未验证推测**：该次运行未对这两个 crate 施加 `-D warnings` |

---

## 三、根因分析：为什么会有如此之多的问题

**结论先行**：不是一个笼统的"框架落后"。真实结构是——**这个功能第一次同时跨越了三个既有抽象
从未设计过的边界**，缺陷因此全部堆在这三处；而富文本最核心的部分反而是整份改动里最扎实的。
归并后**独立根因只有 6 条**。

### 3.1 接缝①：能力开关的约定无法表达"持久化变更" → §2.B

`protocol-file-sliding-window` 与 `protocol-image-compressed-chunks` 这两个先例都是**空 feature**，
它们门控的是"同一份已存数据的另一种线编码"。composite 沿用了这个形状，但它是**第一个改持久化
schema**的扩展（见 `Cargo.toml:57`、`migrations.rs:422`）。

约定本身没错，错在**这类变更需要的是"迁移开关"，而框架只提供了"能力开关"**。
所以"默认关闭"字面为真、语义为假——这是抽象层次错配，不是谁偷懒。

### 3.2 接缝②：`SyncPlatform` 的签名没有"不确定结果"的词汇 → §2.E、§2.F、G-3

基线 `shared/rust-core/src/sync.rs:186-188`：

```rust
fn write_text(&self, text: &str) -> Result<(), String>;
fn write_image(&self, width: u32, height: u32, rgba: &[u8]) -> Result<(), String>;
```

同步签名，**没有超时预算、没有"结果未知"、没有部分成功、没有降级通道**。这套接口是在"写入是
进程内 NSPasteboard 调用、基本不会失败"的年代定的。composite 把它变成了
"spawn 子进程 + 5s 截止 + 可能已生效但回执丢失"。

于是三件事同时发生，且都不是"写错了"：

- 5s 阻塞调用被塞进 async worker 并持锁（§2.F）——因为接口只能同步返回；
- 超时后无法表达"我可能已经改了剪贴板"（§2.E）；
- `Partial` 只能硬塞进 `Err`（G-3）——因为返回类型里没有"部分成功"。

**注意**：composite 新加的 `write_composite` / `write_native_snapshot` 也沿用了这个同步签名，
没有借机引入 `PlatformResultFuture`（同一 trait 里 `files_received` 已经是 async 的）。
框架有更好的形状可用，没用。

### 3.3 接缝③：帮助进程生命周期是既有技术债，被沿用并放大 → §2.D、§2.E

基线 `macos/src-tauri/src/clipboard_file.rs` 早就有 `wait_for_child`：
`try_wait` + `std::thread::sleep(10ms)` 轮询 + 到点 `kill`。新的
`shared/platform-clipboard-composite.rs:147-234` 的 `run_helper` 是它的**近似复制**。

所以"同步阻塞 + 轮询"不是这次发明的，是继承的。但 composite 让它变成了**每次剪贴板事件都可能
走这条路径**（legacy 只在文件/图片批量时走），于是既有的设计债被放大成 §2.D/§2.E 的严重度。

### 3.4 不是接缝的部分：有正确机制却没接上

**最能说明性质的是新路径丢掉了一个仓库早就有的顺序纪律**（见 §2.E 的对照表）：
legacy 在 `sync/clipboard.rs:112-118` 先登记 echo guard 再执行不可逆写入、失败则回滚；
composite 在 `platform-clipboard-composite.rs:139` 写入成功后才登记、无补偿。
**§2.E 正是这个顺序倒置的直接后果**——不是框架缺少能力。

同类还有三处，全是"机制已存在，只是没接线"：

- **§2.D 的静默丢失**：仓库有完整的 `sync_warning` 机制，`delivery.rs:524-531` 也接了
  `CompositeResult` 与 `Expired`，**就是漏了 `Rejected`**。
- **§2.C 的二次复杂度**：仓库对每个数量/字节都有上限、设计极其克制，
  **却在 `composite.rs:188` 把 O(n) 的 `from_utf8` 放进了 O(m) 的循环**。
- **§2.A 的 bash**：纯实现疏忽（bash 3.2 空数组语义），与接缝和富文本都无关。

### 3.5 真正的"不够优雅"在最后一公里（前端）

后端 A− / 前端 C+ 的落差不是框架造成的，是**投入不均**：G-1（有 `.icon-button` 现成约定没用）、
G-2（语义没想清）、G-5（与刚做完的键盘导航工作流脱节）、I-7（Swift 零测试）。

### 3.6 为什么"如此之多"：计数被三个因素放大

1. **审查强度**：6 路对抗审查 + 2 路裁决，每路都把 Low 级也报了；同一根因在多个文件重复出现
   （如"helper 失败静默"在采集/发布/读路径各出现一次）。
2. **表面大**：约 3,800 行新代码横跨 6 层，且未提交、从未过 CI。
3. **盲区**：验证矩阵只跑开启态，所以 §2.A 这类"默认路径崩"的缺陷天然逃过所有本地证据。

### 3.7 富文本本身不是问题，反而是最好的部分

| 富文本专属难点 | 实际状态 |
|---|---|
| CF_HTML 的 `StartHTML`/`StartFragment` 字节偏移 | `composite/html.rs:1` 有专门注释"UTF-8 byte offsets, never character offsets"；中文/emoji 与越界测试齐备（`:90`、`:105`） |
| DIB/DIBV5 stride、bottom-up、alpha 掩码 | `bitmap.rs:64-95, 128-143` 全程 `checked_*` + round-trip 测试 |
| HTML 资源绑定 | `html.rs:457` 拒绝注释/脚本/裸文本被误判为资源；`:418` 两图离线物化测试 |
| PNG / 压缩炸弹 | 累计像素预算（`composite.rs:174`）+ decoder 分配上限（`composite/mod.rs:14-30`）+ 拒绝 APNG |
| 来源因果 | 评审人实跑 10,000 次：0 回音、10,000 次同内容新复制均正确识别为外部 |

**这些地方一个严重缺陷都没有。** 问题全在"围绕富文本的管道"：构建配置、迁移生命周期、
进程监管、失败上报、UI 收尾。

---

## 四、复核更正记录（防误修存档）

本节记录经**对抗裁决**后被推翻或降级的结论。**请勿按原严重度修复**。

| 原结论 | 裁决 | 依据 |
|---|---|---|
| `payload_limit` 是真实上限的 64 倍，属纵深防御缺口 | **REFUTED → Informational** | 本实现**从不发送** `CompositePayload`：`peer/delivery/executor.rs:520-522` 注释 "Always use bounded chunks"，Portable 分支走 `chunks::encode` + `CompositeChunk`（`:536`），Legacy 分支改写为 `TextPayload`/`ImagePayload`（`:504-510`）。32 MiB 是**分片前的排队/准入上限**（`delivery.rs:199/241/396`），不是线上帧上限；唯一处理它的主接收循环（`platform-network-server.rs:750`）**恰好**应用了 512 KiB 上限。属冗余的纵深防御 |
| 字节配额永不驱逐 composite，用量可无限期超限 | **PARTIALLY REFUTED → 🟢 Low** | `db/storage.rs:234` 存在 `"composite" => !self.composite_part_payloads(&[id])?.is_empty()` 分支，由 `reserve_for_file_batch`（`:181`）按物理 `bulk_storage_size` 执行，composite 新增（`db/composite.rs:296`）与网络文件批次（`platform-network-server.rs:405`）都会触发；且 `trim("image")` 的计数分支（`lifecycle.rs:140-151, 218`）本来就会驱逐 composite 行。真实问题只是"降配额后 UI 短暂显示 used>quota" |
| `size_bytes = 0` 导致两个 API 结论矛盾（Medium） | **PARTIALLY CONFIRMED → 🟢 Low** | `get_preview_metadata` 的唯一生产调用点（`tailsync-runtime/src/history.rs:483`）被 `:475-481` 短路到 `prepare_composite_image`；IPC 帧用的是载荷长度（`preview.rs:79`）→ 该 0 在生产路径**不可观测**。真实问题是"无注释的承重哨兵值" |
| 孤儿文件泄漏（Medium） | **CONFIRMED → 🟢 Low** | 内容寻址、重试会复用（`file_storage.rs:141-147`）、1–1.5h 内被 `sweep_orphan_payloads` 回收 → 有界磁盘暂留，非永久垃圾 |
| M0/M7 标 PASS 属过度宣称 | **PARTIALLY CONFIRMED → 🟢 Low（文档卫生）** | 见 I-6：文档限定充分，站得住的只有"违反计划 `:252` 自己的标注规则" |
| `540 = 540` 两次 Core 结果相同，疑似掩盖 | **合法，非掩盖** | 评审人与证据审计各自确认：composite 测试**未被 cfg 门控**（`shared/rust-core/src` 内 0 处 feature cfg），两份日志测试名列表逐行相同（各 544 项，含全部 32 个 composite 测试）；552 = 544 + 12 iroh。对称性真实但**无信息量** |
| `include!` 共享源码是"脆弱设计" | **不采纳为缺陷** | `CONTEXT.md:57-59` 明文把 `shared/platform-clipboard-{transfer,tests}.rs` 定义为 canonical include 源，仓内已有 8 个同类文件；新增文件完全遵循既有约定。建议长期收敛为真正的共享 crate，但不计入缺陷 |
| helper 路径可被劫持（PATH/签名） | **不成立** | `platform-clipboard-composite.rs:152-170` 按 `current_exe()` 父目录解析、无 PATH 查找、无 shell 插值、载荷走 stdin；命名 pasteboard 被限制在 `tailsync-probe.` 前缀（`clipboard-helper.swift:7-9, 96-98`），合成 harness 无法指向 general |
| chunk inspection 也会调 `validate()` | **评审人笔误** | `composite/chunks.rs:24` 的 `inspect()` 只解析头部；但装配完成时确实会调（`chunks.rs:240`），净效果比原描述更糟 |
| `digest()` 的 32 MiB 拷贝是 Medium | **→ Low–Medium，且 `digest()` 整体冗余** | `composite_receiver.rs:41` 的 `bundle.digest()` 恒等于 `blake3(&envelope.content)`（因 `decode` 只接受规范编码，`encode(decode(x)) == x`），而 `envelope.content` 在 `:33` 已 clone 保留；发送端 `executor.rs:562` 只做相等比对。直接哈希可省掉一次 `validate()`、一次 32 MiB 重编码拷贝，并消除唯一的异步上下文长耗时调用 |

---

## 五、修复方案（分阶段闭环）

### 第一阶段：阻断项（发布前必须，预计工作量小）

| # | 动作 | 对应 |
|---|---|---|
| 1 | `macos/build-mac.sh` 空数组展开改 `${ARR[@]+"${ARR[@]}"}` | §2.A |
| 2 | 决定 v12 策略：门控迁移，或把不可逆升级 + 回滚流程写进发布说明与 Go/No-Go | §2.B |
| 3 | `composite.rs:188` 把 `from_utf8` 移出循环；`:190` 用预计算前缀比较；给 bindings 加显式上限 | §2.C |
| 4 | `composite_receiver.rs:41` 直接 `blake3(&envelope.content)`，删除冗余 `digest()` | §2.C |
| 5 | `Rejected` 且携带剪贴板内容时落 `sync_warning`；assembly 预算耗尽改为可重试背压 + warning；message_id 变更视为抢占 | §2.D |
| 6 | helper 失败/超时不得静默：启动探测 helper、连续失败退回 legacy 并上报；**写入前先登记回执**（或在超时后补 token 探测读） | §2.D、§2.E |
| 7 | legacy 入站路径改 `spawn_blocking` 并先释放 engine guard | §2.F |

### 第二阶段：应处理（合并前）

| # | 动作 | 对应 |
|---|---|---|
| 8 | 补 `.item-copy-format` 样式（含 hover / focus-visible），或复用 `.icon-button` | G-1 |
| 9 | 修多图恢复语义：候选 >1 时报错或提供逐图选择并回报实际发布内容 | G-2 |
| 10 | `Partial` 结构化上报 + 专用本地化提示，不转成硬 `Err` | G-3 |
| 11 | `native_only` 时直接拒绝 `restore_format` | G-4 |
| 12 | macOS 菜单补 `accessibilityLabel` + `accessibilityAction` 和/或键盘命令 | G-5 |
| 13 | 恢复失败走稳定错误码 + Loc key，不直出 Rust 英文串 | G-6 |
| 14 | 把编码后的 chunks 缓存在 `PendingFrame`（身份已固定），避免重试重新压缩 | G-7 |
| 15 | 对齐事件时间窗与 TTL，并把过期归类为 `Expired` | G-8 |
| 16 | 把 `raw_len` 计入 peer 字节预算或限制压缩比 | G-9 |
| 17 | receipt 日志损坏时隔离/重建，并合并写入 | G-10 |
| 18 | 区分"revision 不可用"与"被抢占" | G-11 |
| 19 | `legacy_content` 至少回退到 text/html 摘要，或回可重试错误 | G-12 |
| 20 | `FINISH_LOCK` 改为按 `(source, message_id)` 加锁 | G-13 |
| 21 | 把"原生副作用至多一次"的取舍写入文档 | G-14 |
| 22 | 清理 H-1（死代码 + 不可达分支）、H-4（N+1）、H-7/H-8（helper 路径统一） | §2.H |
| 23 | 补测试：默认构建不得迁移 v12、备份 ENOSPC 行为、v12 DDL 原子性、UI 按钮渲染、Swift composite 测试 | I-7 |

### 第三阶段：文档与流程

| # | 动作 | 对应 |
|---|---|---|
| 24 | `check_cross_platform_sync.mjs` 注册新 include 文件 | I-1 |
| 25 | 同步 `CONTEXT.md` / `README.md` / `README.zh-CN.md` 的 schema 版本号；新增 `docs/features/composite-clipboard.md`；考虑 ADR 记录三条接缝的取舍 | I-2、I-3 |
| 26 | 放开证据日志忽略（`!artifacts/composite-evidence/logs/**`）或记录可复现构建配方 | I-4 |
| 27 | `verification.json` 的 `warnings` 改为日志派生 | I-5 |
| 28 | M0/M7 改为 PARTIAL，或把计划 `:252` 的标注规则放宽 | I-6 |
| 29 | 补跑默认（feature 关闭）路径的**脚本级**打包门禁，堵住 §2.A 的盲区 | §2.A |

### 需要单独决策的架构项（不在本次修复范围）

三条接缝值得作为独立工作项：**迁移开关**、**平台写入契约的异步/超时/不确定结果词汇**、
**帮助进程监管**。它们是本次多数问题的共同上游，但不建议在验收修复中顺带重构。

---

## 六、真机验收清单（保留 UNKNOWN）

以下全部**未经真机验证**，不能由本地合成证据推导。清单沿用
`COMPOSITE-CLIPBOARD-NATIVE-ACCEPTANCE-2026-10-03.md` 的编号，评审人认可其 UNKNOWN 状态：

- **APP-01 ~ APP-08**：Chrome / Word 来源 → 微信、Notes、纯文本编辑器、图像编辑器的实际粘贴；
  Windows DIB/DIBV5 透明与方向；大型 Excel/Word 表格。
- **REL-01 ~ REL-07**：10,000 次**跨设备交替远端写入**（本次只跑了本地 AppKit 回路）；
  50ms/500ms/1s/29s 重新复制；慢图文重组中本机复制；缺片/重复片/ACK 丢失/重连/双 peer；
  撤销配对后帧拒绝；各阶段进程退出；LAN/Tailscale/Iroh/睡眠切换。
- **NET-01 / NET-02**：Iroh 冷启动配对与重连的 5 秒预算；邀请配对的不同出口/强制 relay/近过期。
- **HIS-01 ~ HIS-03**：历史重启、搜索、整组与显式恢复；共享大图、收藏、删除、quota、满盘、
  外部目录迁移；v11→v12→隔离回退。
- **UI-01**：两端界面、中文 IME、辅助功能（含本次发现的 macOS 键盘/VO 缺口）。
- **COMPAT-01**：新↔旧、构建 feature 关闭、运行时关闭。

Windows 侧另有独立风险：**sidecar 从未在真实 Windows 上运行**——DIBV5 的 stride/alpha/方向只对
自家编码器验证过；`OpenClipboard` 争用与 250ms 重试是否足够、helper 退出后剪贴板所有权、
CF_DIBV5+PNG 在 Explorer/Word/微信的实际粘贴结果，均未知。

---

## 七、复现命令

```bash
# 0) 隔离环境（每次调用覆盖 TAILSYNC_DATA_DIR / STORAGE_DIR / V1_DATA_DIR）
#    以下全部用 node scripts/run-isolated-tests.mjs -- <命令>

# 1) Core（3 扩展，排除 iroh）
cargo test --locked --offline -p tailsync-core \
  --features 'protocol-composite-clipboard protocol-file-sliding-window protocol-image-compressed-chunks' \
  --lib -- --skip iroh_transport

# 2) Core 默认 feature / Protocol / Runtime / 平台 crate
cargo test --locked --offline -p tailsync-core --lib -- --skip iroh_transport
cargo test --locked --offline -p tailsync-protocol -p tailsync-runtime \
  --features tailsync-core/protocol-composite-clipboard
cargo test --locked --offline --manifest-path macos/src-tauri/Cargo.toml \
  --features protocol-composite-clipboard --lib
cargo test --locked --offline --manifest-path windows/src-tauri/Cargo.toml \
  --features protocol-composite-clipboard --lib

# 3) Swift / React / 契约 / 跨平台漂移
swift test --package-path macos/swift-ui
npm --prefix windows test
node scripts/check-local-contracts.mjs
node windows/scripts/check_cross_platform_sync.mjs \
  --win-root windows --mac-root macos --core-root shared/rust-core

# 4) Windows helper 类型检查（无 MSVC 汇编工具的主机）
cargo check --locked --offline -p tailsync-clipboard-helper \
  --features portable-check --target x86_64-pc-windows-msvc

# 5) 原生合成探针（只用命名 pasteboard，不碰通用剪贴板）
python3 tools/clipboard-probe/probe.py \
  --helper artifacts/TailSync-Composite.app/Contents/MacOS/clipboard-helper --self-test
cargo run --locked --offline -p tailsync-core --example composite_origin_probe \
  --features protocol-composite-clipboard -- \
  "$PWD/artifacts/TailSync-Composite.app/Contents/MacOS/clipboard-helper" 10000

# 6) 复现 §2.A（默认路径构建中断）
/bin/bash --version            # 期望 3.2.57
# 在 set -euo pipefail 下展开空数组：
/bin/bash -c 'set -euo pipefail; A=(); echo before; printf "%s\n" "${A[@]}"; echo after'
# 期望：before 后报 "A[@]: unbound variable" 且不打印 after
```

### 证据复算

```bash
# 源码指纹：按 source-fingerprint.json 的 hash_method
#   SHA256(sorted "path\0sha256hex\n"); 排除 docs/ artifacts/ *gen/schemas/
# 候选包：按 verification.json 的 package.files 逐文件 sha256 + 聚合
# 日志：verification.json 每项 log 的 sha256 与 log_sha256 比对
```

---

## 附录：分区审查范围与归属

| 分区 | 覆盖范围 | 主要贡献 |
|---|---|---|
| A 协议 / 安全 | `tailsync-protocol/src/{composite,native_clipboard,bitmap,lib}.rs`、`secure.rs`、`handshake.rs`、`inbound_source.rs`、`platform-network-server.rs` 命令分发、两端 `api/routes.rs` | §2.C、G-9、H-12 边界核查；确认"先检查后分配"、协商双向且认证、无授权绕过 |
| B Core 可靠性 | `composite/**`、`peer/delivery*`、`composite_receiver.rs`、`composite_receipts.rs`、`sync*`、两端 `clipboard_change.rs`/`sync_adapter.rs` | §2.D、§2.F、G-7~G-14；确认两阶段回执、分片幂等、supersede 分层 |
| C 存储 / 迁移 | `db/composite.rs`、`db/**` | §2.B、H-1~H-4；确认 AEAD 静态加密、无 use-after-free、配额失败诚实、SQL 全参数绑定 |
| D 原生 I/O / 打包 | `clipboard-helper.swift`、`platform-clipboard-composite.rs`、两端 `clipboard.rs`、`tools/clipboard-helper/**`、构建脚本 | **§2.A（Critical）**、§2.E、H-5~H-11；确认 IPC 分帧、GMEM 所有权、DIB stride/alpha、打包断言 |
| E UI / 契约 | `tailsync-runtime/src/history.rs`、`shared/schema/**`、Swift/React UI | G-1~G-6、H-15~H-18；确认契约生成链自洽、无强制解包、无重复发布 |
| F 证据审计 | `docs/**`、`artifacts/composite-evidence/**` | §1.2、I-1~I-6；独立复算指纹/包/日志，确认 540=540 合法 |
| 裁决 1 | 存储类结论（4 条） | §四：3 条降级、1 条部分推翻 |
| 裁决 2 | 协议类结论（5 条） | §四：1 条 REFUTED、2 条降级、2 条确认（其中 §2.C 被加重） |
| 评审人 | 全部证据复现 + 基线对比 | §1、§3.2/§3.3/§3.4 的基线证据、§2.E 的顺序倒置、§四 的最终裁定 |
