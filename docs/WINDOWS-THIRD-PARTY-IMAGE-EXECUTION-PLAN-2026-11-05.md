# Windows 第三方应用图片链路：可交接执行方案

日期：2026-11-05（Asia/Shanghai）。

状态：**待执行，不代表任何实现、原生验收或应用兼容性已经完成。**

面向执行者：DeepSeek v4.1 Flash 1017，或其他接手此仓库的编码代理。

源码基线：`bc5ff769dbf0f969977c50b2d30dbe856909087e`。编写时工作树存在大量其他任务的未提交修改；本文是基于该工作树的增量执行设计，不是对干净提交的完整复验。

前序材料：[2026-11-04 架构分析计划](WINDOWS-THIRD-PARTY-IMAGE-ANALYSIS-PLAN-2026-11-04.md)。前序文档保留为分析记录；本次实施的任务顺序、约束、验收和停止条件以本文为准。若执行时更高优先级的人类指令、`AGENTS.md` 或已接受 ADR 与本文冲突，先报告冲突，不擅自修改既有约束。

> 本文包含交给后续执行者的实施指令。编写本文本身只新增文档，不授权或声称已经运行真实剪贴板采样、修改产品代码、构建安装包或部署应用。

## 0. 执行摘要

### 0.1 最终目标

Windows 的目标不是复制 macOS 的 AppKit、RTFD、容器缓存授权机制，而是接入同一套事实保留与便携投影架构：

```text
系统观察与隐私检查
  → Windows 类型化采集
  → CNA1(platform=2)：原生有效载荷 + 未读取事实 + 明确来源的派生物
  → from_archive
  → 本地加密历史 / 共享内容分析 / 便携同步
  → 目标端预览与实际粘贴
```

必须分别回答四个问题：**采到了什么、能预览什么、能同步什么、能恢复什么**。任何一个问题的 PASS 都不能代替另外三个。

### 0.2 第一轮交付边界

第一轮只执行 **W00–W05**：建立取证工具、隐私门禁、最小 CNA1、DIB 派生准入、回声回归和生产接线。

第一轮明确不包含：QQ/微信缓存读取、任意私有格式批量解码、OLE 虚拟文件落地、EMF/WMF 渲染、新设置页、Windows 原件去重域扩展。

无 Windows 环境时仍可完成纯模型、共享准入、夹具和宿主检查；W01/W05 中的原生验证标为 UNKNOWN。**不得因此把 W06–W09 提前实现，也不得把第一轮标成原生验收完成。**

### 0.3 执行顺序

```text
W00 基线与台账
  → W01 元数据取证 / 无系统 I/O 的纯模型
  → W02 Windows 隐私门禁
  → W03 最小 CNA1 与 DIB 派生
  → W04 文件分类、恢复与回声接缝
  → W05 Windows 生产接线和第一轮验收
  → G1 复核：第一轮是否允许进入下一轮
  → W06 完整元数据枚举 / 分类型读取
  → G2 样本决策：是否需要 OLE、额外图片派生、具体应用适配
  → W07 按证据扩展通用能力（可不做）
  → W08 逐应用适配与必要设置（可不做）
  → W09 Windows 打包、目标应用、双端验收
```

一个任务包完成后才进入下一个。允许准备不依赖原生结果的夹具；不允许用准备工作绕过 G1/G2。

## 1. 开工前必须接受的修正

### 1.1 格式数量不是 item 数量

Win32 的 `EnumClipboardFormats` 枚举格式表示，并不提供 macOS pasteboard item 枚举。文本、RTF、HTML 可以是同一份内容的不同表示，不能计为三个 item。（MS1、MS5）

- 第一轮对普通 Win32 一次复制使用一个语义 item；空、忙、抑制等状态按既有协议表达。
- 一段 HTML 里有三张图，不代表三个原生 item。
- `CF_HDROP` 内有多个文件，不应直接映射为多个复合图文 item，仍交文件管线处理。
- `FileGroupDescriptorW` / `FileContents` 所表示的多对象可能涉及 OLE 的 `IDataObject`、`FORMATETC.lindex`、`TYMED_ISTREAM` 等，需要独立设计。（MS5、MS6）
- 没有证明对象边界时，保留描述符和未知状态；不猜数量、不截取第一张、不把格式列表展开成多个 item。

### 1.2 “可枚举”“可读”“可保存”“可回放”相互独立

当前 Windows helper 的 `read_format` 把 `GetClipboardData` 返回值作为 `HGLOBAL` 使用。这种实现不能直接推广到所有格式。`CF_BITMAP`、`CF_ENHMETAFILE` 分别是 GDI 类型的句柄；OLE 数据也不必使用全局内存。（MS2、MS3、MS5）

第一轮规则：没有明确的字节载荷契约，就只记录存在，不读取；没有明确的恢复适配，就不扩大 `windows_publishable`。

### 1.3 “原件”不是进程句柄或任意内存快照

Windows 原件是**该格式定义的、可安全复制的有效载荷**，不是地址、COM 接口指针、GDI 句柄值或未经定义的分配填充。

- 每种准入格式必须说明有效长度、终止符和边界校验规则。
- `GlobalSize` 仅是内存对象大小，不能单独证明有效正文长度或格式可序列化。
- 不通过解码再编码替换原件；转换结果写入 `derived`。
- 现有 CNS1 里的 DIB 生成 PNG 不得原封不动冒充 CNA1 的系统原始 PNG。
- 不将 Windows 系统可合成的格式一律称为“应用主动提供的格式”。Win32 枚举包含系统可转换表示，应用来源证据与系统可见格式要分开表述。（MS1、MS4）

### 1.4 共享主干已接通，不等于全部下游能力已对等

内容分析、显示文档和网络图片归档有共享实现，但具体格式仍有准入差异。例如当前 RTF 扫描器识别 PNG 图片附件，其他 blip 类型不能直接视为可用附件。

允许在共享层增加**受来源、平台、转换器与格式约束的能力**；禁止为 Windows 降低现有验证强度，或把失败变成“成功但静默丢图”。

## 2. 不变项与决策表

### 2.1 不变项

1. 不修改身份/基础线协议版本、数据帧版本、主历史 schema、effect journal schema、local contract 版本或产品版本。
2. CNA1 继续使用既有编码版本，Windows `platform=2`；异平台原件回放继续拒绝。
3. macOS helper protocol 仍为 4，原有 QQ/Qt/RTFD 行为与安全门禁不能被放宽。
4. 第一次结构接入不修改自动网络图片获取、首次事件冻结、缺图占位、ACK 或历史事务语义。
5. 不把 NativeOnly 本地原件发送给异平台，也不新增跨平台原生句柄回放。
6. 不改名、格式化、还原或顺手修复其他任务的文件；不提交 Git commit，不自动创建分支。
7. 不操作生产数据库、真实 Keychain 或其他用户数据目录；测试遵守 `CONTEXT.md` 的隔离要求。
8. 不自动执行需要写真实剪贴板的测试；该步骤需要 Windows 测试机和人类明确确认。

### 2.2 本计划的具体选择

| 决策 | 第一轮选择 | 允许扩展的条件 |
| --- | --- | --- |
| Windows helper protocol | 建议 2 → 3；W00 先建立独立兼容决策记录，获确认后实施 | 若编号已占用，先报告并调整决策；不得改 macOS 的 4 |
| `--read-archive` | 新增，生产采集改用它 | 必须通过平台校验与捕获接缝测试 |
| `--read-snapshot` | 保留现有用途，不擅自全局改成“归档原件投影” | 调整任何读取调用点前先验证 Partial 回读与来源匹配 |
| Win32 item | 普通复制为一个语义 item | OLE 多对象必须经 G2 决策 |
| 归档完整性 | 第一轮 `enumeration_complete=false` | W06 仍默认 false；升级需明确 Windows 完整性契约，不能只看枚举循环结束 |
| Windows 原件去重 | 不扩展；预计仍无原件 dedupe key | 另立身份与兼容性决策，不复用 macOS 闭合性假设 |
| 未知格式正文 | 默认不读取、不解码 | 有样本与载荷类型证明后逐项准入 |
| 回放白名单 | 保持现有集合 | 独立安全与原生消费证据，不因归档扩容而放宽 |
| 隐私标记 | 尊重本节 2.3 的保守整事件抑制策略 | 将来细分“只存本地/不发送”须另做产品决策 |
| 图形依赖 | 第一轮不新增解码器 | W07 单独论证许可、包体积、像素预算和原生安全性 |
| 第三方设置 | 第一轮不新增 | 仅确有需要且有可执行适配器时增加 |

“独立兼容决策记录”是新增可复核文档，不等于执行者可自行宣布一个 ADR 已接受。若仓库流程要求人类确认，停在该门禁，继续完成不依赖批准的测试准备即可。

### 2.3 Windows 隐私标记的第一轮策略

Windows 对本机历史和云同步分别提供控制格式；以下是 TailSync 计划采用的**更保守策略**，不是声称这些标记对第三方软件形成系统强制权限。（MS4）

| 观察结果 | TailSync 动作 |
| --- | --- |
| 存在 `ExcludeClipboardContentFromMonitorProcessing` | 整事件抑制，不读取其他正文、不入历史、不发送、不触发图片获取 |
| `CanIncludeInClipboardHistory` 的规范 DWORD 值为 0 | 整事件抑制；接受因此同时禁止同步的保守行为 |
| `CanUploadToCloudClipboard` 的规范 DWORD 值为 0 | 整事件抑制；接受因此同时不记本地历史的保守行为 |
| 上述 DWORD 值为 1 | 继续检查其他门禁，不构成“绕过其他限制”的授权 |
| 标记值无法读取、长度异常或值不在 0/1 | 当前 revision 暂不采集，按受限重试/失败处理，不回退到文本/图片/文件读取 |
| 没有这些标记 | 继续既有策略 |

具体约束：

- 名字存在型标记应在其他正文读取前处理；值型标记只允许有界读取其固定控制载荷。
- 隐私判断的纯规则只维护一份；helper 和 daemon 的系统 I/O 可以是薄适配器，不能各自复制判断分支。
- helper 在实际采集的同一次打开事务内再检查，不能只信较早的 daemon 预检。
- legacy 与文件路径也必须通过门禁；helper 缺失/不健康不能成为绕过隐私标记的理由。
- daemon 预检需要稳定 revision；检查后 revision 改变则丢弃结果并重新观察，不能把旧许可用于新复制。
- 控制检查失败不发送正文、不默认放行；日志只保留固定错误类别，不包含内容、路径或任意异常字符串。
- 被抑制事件的业务输出为空；若 CNA1 使用 `SnapshotState::Suppressed`，遵守协议的零 item、零 format、零 derived 限制。

## 3. 开工时的源码地图

下表是阅读入口，不是允许整文件重写的清单。行号会漂移，以符号为准。

| 位置 | 先读的符号或内容 | 用途 |
| --- | --- | --- |
| `CONTEXT.md` | 分层、开发门禁、测试隔离 | 仓库约束 |
| `tools/clipboard-helper/src/main.rs` | `analysis_command`、平台入口 | CLI 分发与跨平台纯测试入口 |
| `tools/clipboard-helper/src/windows.rs` | `Clipboard`、`read_format`、`snapshot`、`publish`、`run` | Win32 I/O、已有转换、发布语义 |
| `tools/clipboard-helper/src/analysis.rs` | `analyze` | RTF/RTFD 支持边界 |
| `tools/clipboard-helper/tests/self_check.rs` | `startup_probe_succeeds_without_clipboard_input` | 协议探测测试 |
| `tools/clipboard-helper/tests/publish_receipt.rs` | 回执、退出后 revision、显式 ignored 测试 | 原生发布回归；不得默认运行 |
| `shared/platform-clipboard-composite.rs` | `read_archive`、`read`、`classify`、`classify_native`、`write_native_cancellable`、`HELPER_PROTOCOL_VERSION` | canonical 平台接线、健康、回读、来源 |
| `shared/tailsync-protocol/src/native_clipboard.rs` | `NativeSnapshot`、`FormatState`、`SnapshotState`、限制常量 | 本机快照契约 |
| `shared/tailsync-protocol/src/native_archive.rs` | `NativeArchive`、`DerivedFormat`、converter 常量 | 归档与派生契约 |
| `shared/tailsync-protocol/src/native_policy.rs` | `windows_publishable`、`control` | 采集与回放边界，不扩大 legacy 存储 |
| `shared/schema/native-capture-policy.json` | 当前策略源 | 改生成策略时必须从源生成 |
| `shared/rust-core/src/composite/mod.rs` | `from_native`、`from_archive` | 文件判定、便携投影、派生准入 |
| `shared/rust-core/src/composite/fingerprint.rs` | `requires_original_domain`、来源指纹 | 完整枚举后可能触发的来源域变化 |
| `shared/rust-core/src/composite/identity.rs` | `archive_dedupe_key` | 原件完整性与 dedupe 资格 |
| `shared/rust-core/src/composite/restore.rs` | `originals`、`compatibility`、`native_plan` | 平台拒绝、发布前准备、格式清单 |
| `shared/rust-core/src/db.rs` | `production_migration_policy` | 健康与 v13 门禁；不是平台 `db.rs` 文件 |
| `shared/rust-core/src/db/composite.rs` | `add_native_archive`、`add_native_archive_with_images` | 加密存储与事务；编写时已有其他任务修改 |
| `windows/src-tauri/src/clipboard.rs` | `clipboard_loop`、`CaptureEnvironment`、`process_composite_capture` | 生产状态机与可注入接缝 |
| `macos/src-tauri/src/clipboard.rs` | 对应归档捕获接缝 | 参考事务、内存、来源与 NativeOnly，不整段复制 |
| `shared/tailsync-protocol/src/content_analysis.rs` | `scan_rtf`、图片 blip 分支 | 附件能力的真实范围 |
| `shared/tailsync-runtime/src/content_analysis.rs` | 分析编排 | 不重写共享主干 |
| `windows/src/preview/renderers/DocumentPreview.tsx` | 文档与占位消费 | 用能力矩阵验证，不默认改 UI |
| `windows/scripts/build-composite-helper.ps1` | `Target`、sidecar 输出 | 原生 helper 构建 |
| `windows/scripts/package-windows.ps1` | `CompositeClipboard`、构建与输出目录 | 默认态/开启态包装验证 |
| `shared/schema/generate-settings.mjs` | 从 Rust 导出 schema 再生成 TS | 若未来新增设置，以真实生成入口为准 |

不要照抄旧计划里的 `windows/src-tauri/src/db.rs` 或 `scripts/generate-settings.mjs`：当前实际入口分别在 shared Core 和 `shared/schema/`。

## 4. 执行与汇报协议

### 4.1 工作树保护

W00 开始先保存以下信息到新执行台账，不保存正文或敏感运行数据：

```sh
git rev-parse HEAD
git status --short
git diff --stat
```

同时检查适用的 `AGENTS.md`、既有 ADR 和本文源码入口。对已有修改文件，只补本任务必需的小范围 hunk；无法分离就报告，不执行 `git reset`、`checkout --`、`clean`、stash 或大范围自动格式化。

### 4.2 新执行台账

开工时新增 `docs/WINDOWS-THIRD-PARTY-IMAGE-EXECUTION-LOG-<实际日期>.md`。本文所有 `PENDING` 都是计划状态，不可直接改成 PASS。

每个任务包用同一模板：

```text
任务：Wxx
状态：PENDING / IN_PROGRESS / CODE_READY / VERIFIED / BLOCKED / NOT_APPLICABLE
基线：HEAD + 已有 dirty 文件摘要
改动文件：仅本任务的文件
先验失败：测试名、原始失败类别、命令和退出码
完成内容：实际行为变化
验证：命令、退出码、执行平台、命中测试数量、证据路径
未验证：缺少哪种环境/样本，不得空白
下一步：进入哪个任务，或等哪项确认
```

`CODE_READY` 只代表源码与对应非原生检查就绪；`VERIFIED` 必须注明验证层级。编译、宿主测试、Windows API 类型检查、Windows 原生 I/O、目标应用粘贴、双设备分别记录，不能折叠成一个 PASS。

### 4.3 增量工作规则

1. 一次只实现一个任务包；先写最小复现，再实现，再运行受影响的精确测试。
2. 新回归测试统一采用 `wcap_` 前缀，便于筛选；这是新测试命名约定，不要求重命名旧测试。
3. 过滤测试前先 `-- --list` 核实存在；“0 tests”不算通过。
4. 不删断言、不改 expected failure、不用 ignore 掩盖新回归。
5. 既有不相关失败单独记录；不顺手修复，也不以此省略本任务验证。
6. 新接口优先纯输入/输出；系统剪贴板、时间、revision、失败和关闭动作都要能注入。
7. 测试辅助能力不能绕过生产的校验路径；不能只测一个生产从未调用的 mock 转换函数。
8. 任务完成后汇报再继续同一轮；到 G1/G2 和明确停止条件时必须停下等待决策。

## 5. W00：锁定事实与兼容决策

**目标**：建立可恢复的执行上下文，不做产品行为改变。

**步骤**：

1. 建立执行台账，记录 HEAD、dirty 清单、操作系统、可用工具链、是否具备 Windows 原生测试机。
2. 核实 `HELPER_PROTOCOL_VERSION`、helper `--self-check`、schema 与生成链路，记录实际值。
3. 新增一份 Windows helper 2 → 3 的兼容决策草案，写清本机 sidecar 协议与网络协议无关。
4. 决策矩阵至少覆盖：新 daemon + 旧 helper；旧 daemon + 新 helper；缺 helper；macOS 4 不变；功能关闭态与既有 v13 的可读性。
5. 核实 `read_archive` 不仅有 `platform==1` 校验，还有 macOS 条件编译；后续要同步处理函数可用性与平台验证，不能只改一个数字。
6. 记录 W01–W05 的范围。若人类暂未确认 helper 协议变更，可先进行不切换生产协议的纯模型与测试准备。

**验收**：台账和兼容矩阵存在；没有修改产品版本、网络版本或主历史 schema；没有覆盖其他任务修改。

## 6. W01：取证工具与可测试的采集模型

**目标**：先知道系统暴露了什么；把危险系统 I/O 与纯规则分开。

**拟修改范围**：helper CLI、helper 下新增的纯采集模型/诊断模块及测试。可在 `src/windows/` 下按 I/O 职责分文件，但不要求重命名现有 `windows.rs`。

### 6.1 默认诊断命令

建议接口为 `--dump-formats`；输出版本化 JSON，默认只输出元数据：

```json
{
  "diagnosticVersion": 1,
  "scope": "win32-visible-formats",
  "before": 100,
  "after": 100,
  "stable": true,
  "win32EnumerationComplete": true,
  "truncated": false,
  "formats": [
    {
      "id": 13,
      "name": "CF_UNICODETEXT",
      "nameKind": "standard",
      "available": true,
      "readAttempted": false,
      "byteLength": null,
      "captureClass": "known-hglobal"
    }
  ]
}
```

上例只是字段契约示例，不是真实采样。实现时新增 `errorKind` 等字段可以，但须固定枚举值、版本化并补解析测试。

要求：

- 使用 `EnumClipboardFormats`；区分 `0 + ERROR_SUCCESS` 的结束与真正失败。（MS1）
- 标准格式用固定名称表；`GetClipboardFormatNameW` 只适用于注册格式，不能把标准格式返回 0 误记为“格式不存在”。（MS8）
- 无名字的私有数值格式使用明确的诊断显示名，不能伪称其名称在机器/应用之间具有稳定身份；第一轮不准入其正文。
- 名称失败、过长、截断、非法控制字符要有显式结果；不得把两个截断后的名称合并。
- 默认禁止 `GetClipboardData`、`GlobalSize`、正文摘要和十六进制前缀；`byteLength=null` 是正确结果，不虚构 0。
- 可选正文探测另设显式参数，例如 `--probe-known-bytes`，只读取已准入类型，不提供任意格式 ID 的通用读内存入口。
- 前 16 字节也可能包含正文；第一轮不输出。若以后确需样本，应只用人工合成内容、明确同意并另记证据。
- 输出流只有 JSON；stderr 不输出剪贴板正文、账号目录、标题、文件路径或未脱敏异常。
- 控制诊断条目数量与输出大小；诊断条目上限建议 256、输出上限 256 KiB，均属本机诊断限制，不修改 CNA1 的限制常量。
- 锁持有期间只做枚举或有界复制，不做解码、网络、磁盘扫描。读取字节可能触发延迟渲染，因此进程仍必须有可终止期限。（MS7）

### 6.2 纯模型与测试

拟引入的 seam 至少能模拟：格式枚举序列、名称解析、数据类型、控制值、读取错误、预算和关闭动作。纯规则应在非 Windows 宿主也能执行，不能全部放进 `#[cfg(windows)]` 后只剩零测试。

最少测试：

- `wcap_metadata_dump_never_requests_payload`
- `wcap_standard_name_does_not_require_registered_name_lookup`
- `wcap_enum_error_is_not_successful_end`
- `wcap_duplicate_or_truncated_names_do_not_alias`
- `wcap_many_formats_report_truncation_without_unbounded_allocation`
- `wcap_multiple_formats_do_not_create_multiple_items`

### 6.3 真实取证矩阵

只用人工准备的非敏感文字与图片，由人类在应用内复制。执行者不得自行读取真实聊天记录或批量扫描账号。

首批优先：QQ、微信、Word、Chrome、资源管理器；企业微信为下一优先级；钉钉、飞书、Teams 按实际可用环境补充，不假定已安装。

每个应用尽量覆盖：纯文字、单图、文字夹两图、仅多图、文件复制；不适用的动作记 `NOT_APPLICABLE`，不是 PASS。

证据表字段：Windows 版本/架构、应用准确版本、动作、人工合成样本编号、格式 ID/名字/顺序、是否只枚举、读取状态、revision 稳定性、是否疑似 OLE、下一步结论。

`clipboard-owner-test.ps1` 仅用来制造剪贴板忙的对照，不把“锁住剪贴板”当成应用形状取证。

**验收**：纯模型测试通过；有 Windows 时保存脱敏元数据证据；没有 Windows 时明确 UNKNOWN，并继续 W02–W04 的纯测试部分。

## 7. W02：隐私优先与失败关闭

**目标**：新增归档不能扩大敏感内容采集，也不能让关闭/回退路径绕过门禁。

**步骤**：

1. 将第 2.3 节策略实现为共享纯判断，不把 Windows 标记名字粗暴加入 macOS `SUPPRESSED` 名单代替值判断。
2. helper 实际读取入口先枚举必要控制信息；抑制时不得读取正文，也不得生成 derived。
3. daemon 在进入 composite、legacy 文本/图片、文件路径前统一进行有界预检；预检与后续读取绑定 observed revision。
4. 测试门禁在 composite 功能关闭、helper 版本不匹配、helper 缺失时仍适用；不为获得门禁而调用一个已判不健康的 helper。
5. 隐私预检忙或失败，保持现有失败/有限重试语义；当前事件不回退到另一路径“试着读”。
6. 抑制事件不建立历史行、不广播、不排网络图片任务；不产生携带正文的故障日志。
7. 检查失败可产生固定类别告警；不得把抑制等同于“剪贴板为空”后错误覆盖用户剪贴板。

**测试**：三种标记、0/1/非法值、长度错误、读取失败、revision 变化、helper 不健康、功能关闭、文件事件、图片事件，均断言各类正文读取调用数、历史写入数和广播数。

**验收**：至少有一个测试证明“抑制标记存在时其他正文读取调用数为 0”；所有回退路径有相同保证。

## 8. W03：最小 CNA1 与 DIB 派生闭环

**目标**：把已支持内容接入归档，同时不丢图片、不伪造原件。

**步骤**：

1. helper 新增 `--read-archive`，编码 `NativeArchive { platform: 2, enumeration_complete: false, ... }`。
2. 第一轮保留既有标准格式采集范围；不把采样到的未知正文自动加入白名单。
3. 为各已准入格式定义有效载荷规则；保证保存的字节可通过现有格式校验，且不把重编码的表示塞回 original。
4. 可先记录未读取的额外描述符，但不得为了填状态读取任意未知类型。
5. 把现有 DIB→PNG 从快照合成物改成带来源的 `DerivedFormat`。
6. 建议新增全局唯一常量 `CONVERTER_WINDOWS_DIB_TO_PNG = "tailsync.windows-dib-to-png.v1"`；若仓库已有同义常量，复用而不重复发明。
7. 在 `from_archive` 中新增精确准入：`platform=2`、来源是 `CF_DIB/CF_DIBV5`、converter 相符、输出名明确、输出是符合现有预算的静态 PNG；推荐输出名为 Windows 便携映射已识别的 `PNG`。
8. 来源格式必须在 original 中以 `Captured` 存在；不能凭 converter 字符串接纳任意字节。
9. 已有可用静态原始 `PNG` 优先，不用 derived 替换它。若原始 PNG 损坏或为动画，第一轮保留失败/不支持，不新增隐式“拿另一张图替代”规则。
10. 所有转换必须在关闭系统剪贴板后运行；DIB 读取字节在关闭前复制完成，不保留系统句柄。（MS2）
11. 原件先满足保留预算；派生物预算不足时放弃派生，不删原件腾空间、不标成完整便携成功。
12. RTFD 的 HTML+Text 才有成对原子准入需求；单张 PNG 派生不机械套用“两项必须成对”。

### 8.1 预算与状态

- 当前 `NATIVE_LIMIT=40 MiB`、`ITEM_LIMIT=16`、`FORMAT_LIMIT=64`；W00 重查后引用真实常量，不复制魔法数字替代。
- 编码总预算包含 original、derived、条目名字和 framing；不能只算 payload。
- `OPAQUE_LIMIT=4 MiB` 不是任意句柄可安全读取的证明，也不是所有已知图片的统一上限。
- `Absent`：观察到格式，但按策略未读取/延后，不携带字节。
- `Failed`：已获准并实际尝试读取，但失败，不携带字节。
- `TooLarge`：因明确单项/总预算拒绝，不携带字节。
- `Captured`：类型与边界校验通过、字节已复制；它不自动证明外部依赖闭合。
- 没被枚举到的白名单名字不要伪造为 `Absent`；该状态需要存在事实。
- 忙、不稳定、失败、抑制等状态按现有编码约束处理，不能附带稳定成功的 derived。

### 8.2 必须先建立的回归

| 测试 ID | 断言 |
| --- | --- |
| D01 | 原始 PNG 保持 original，纯图便携发布仍成功 |
| D02 | 仅 DIB 输入：DIB 字节保留，PNG 只在 derived，`from_archive` 得到正确像素 |
| D03 | 仅 DIBV5 输入：同上，包含 alpha 和方向样本 |
| D04 | 伪造 source/converter/platform/name 任一项被拒绝 |
| D05 | 原件不存在或未 Captured，不准入派生 |
| D06 | 原件静态 PNG 不被派生覆盖；损坏/动画不假成功 |
| D07 | 派生超过像素或总预算：保留原件与明确不可便携状态 |
| D08 | 编码/解码往返保持 original/derived 分离 |
| D09 | 持锁期间转换调用数为 0；关闭动作先于解码 |
| D10 | macOS TIFF、RTFD、静态封面及 QQ 派生准入不变 |

**验收**：D01–D10 对应测试存在并通过；不能只测 helper 生成 PNG，而漏掉共享层实际拒绝它的问题。

## 9. W04：文件分类、来源回声和恢复接缝

**目标**：在切换生产路径前，先证明不会丢文件、产生回声或错误恢复。

### 9.1 文件语义

当前 composite 采集发生在 legacy 文件/文本/图片路径之前；只有返回 `FilesFallback` 才进入后面的文件处理。后面的 legacy 路径确实先检查文件，但这不能替代 composite 的正确分类。

`from_native` 当前仅接受很窄的纯文件格式集合。全量枚举加入 Shell 辅助格式后，存在被判成 NativeOnly 的风险；这是需构造夹具验证的风险，不是已观察到的现场故障。

实施要求：

- 先测试 `CF_HDROP + Preferred DropEffect + Shell IDList` 等文件辅助形状。（MS6）
- 在 Windows 类型化识别层建立“已证明为文件选择”的规则，向既有 `SnapshotState::FileSelection` 或等价接缝输出，不扩大到“只要有 CF_HDROP 就是纯文件”。
- `CF_HDROP` 与真实正文/图片混合时，必须有单独规则和样本；第一轮保守保留 NativeOnly，不静默丢弃正文或读取引用文件。
- 纯文件选择在复合 item/正文预算前识别；大量文件仍交既有文件管线，不为制造 CNA1 而读取文件内容。
- 不把 `FileContents` 当 `CF_HDROP` 路径列表；虚拟文件保持待决。

### 9.2 回声和原件域：新增硬门禁

必须先读 `fingerprint.rs`：未知格式可能触发 `requires_original_domain`。全量枚举新增 `CF_TEXT`、`CF_OEMTEXT`、`CF_LOCALE`、GDI 或 OLE 辅助信息后，不能假定原来的来源指纹仍相同。

第一轮明确：

1. 存储用完整归档，来源判断用可证明正确的观察输入；两者不能为了匹配方便而互相篡改。
2. 不直接用“有 derived 就对便携封面算指纹”的通用规则。要对照现有 macOS 接线，测试 DIB 原件恢复、便携 PNG 发布、退出后回读三种路径。
3. 保留现有 token + revision + 内容指纹/intent 证明；不能只看来源标记 token 就判 SelfWrite。
4. 同一内容被用户再次复制，仍是新事件；不能借内容去重吞掉它。
5. Partial 发布继续用实际稳定回读确认，不能改用最初意图或便携包声称成功。
6. Windows 生产枚举暂不扩展到会改变来源域的额外格式；W06 开启前必须通过完整来源矩阵。
7. 若确需 Windows 专属观察归一化，只能针对已证明的系统表示关系实施，并同时覆盖写入准备、读回和捕获；未知私有格式不可以“忽略后就算相等”。
8. 若需要改变持久化来源域、fingerprint 版本或旧 intent 恢复语义，**停下建立独立兼容决策，不在本任务顺手升级**。

必须覆盖：便携 text/HTML/PNG 自写无回声；DIB 原件恢复；可合成表示出现；伪造来源标记但内容不同；同内容新 revision；未知格式新增；读取失败；Partial；回执丢失；helper 崩溃；重启后 unresolved intent。

### 9.3 恢复与身份

- 本机原件恢复仍受平台、完整性、依赖与 `windows_publishable` 共同控制。
- 新归档因为 `enumeration_complete=false` 而不能提供完整原件恢复时，UI 应如实呈现可用的便携路径；不把便携副本标成“完整原件”。
- macOS ↔ Windows 的原件回放必须拒绝；便携兼容恢复独立测试。
- 存在 `Absent/Failed/TooLarge` 不得宣称完整原件或生成原件去重资格。
- 当前 `archive_dedupe_key` 对 Windows 格式没有自动闭合性证明，第一轮不扩展该名单。
- 原件哈希、legacy hash、portable digest、dedupe key、来源指纹不能混用。

**验收**：新增测试先复现风险再通过；任一回声或错误 SelfWrite 仍失败时不得进入 W05 生产切换。

## 10. W05：Windows 生产接线与第一轮交付

**前置**：W02–W04 对应纯测试和接缝测试通过；helper 版本兼容决策已确认。真实 Windows 结果没有时，不伪装完成原生门禁。

**步骤**：

1. 同步更新 Windows helper `--self-check`、`HELPER_PROTOCOL_VERSION`、健康解析/匹配测试；macOS 4 不变。
2. `read_archive` 在 Windows 可用，按编译目标严格接受对应 platform；不是同时无条件接受 1 和 2。
3. 将 Windows `CaptureEnvironment` 的生产采集依赖切到可注入的 `read_archive`，保留 revision、时钟、故障记录接缝。
4. `process_composite_capture` 用 `archive.original` 做 observed revision 匹配；转换改为 `from_archive(&archive)`。
5. 成功便携捕获存 `add_native_archive[_with_images]`；NativeOnly/UnsupportedItems 分支存归档，不再降为 `add_native_snapshot`。
6. `Files` 继续 `FilesFallback`；Empty、Suppressed、Busy、Unstable、Failed 按既有分支处理，不落成假成功归档。
7. 保留存储不可用、网络图片归档失败、观察票据、capture generation、supersede、恢复 generation 和重试耗尽行为。
8. 内存预算覆盖 helper stdout、original 的必要副本、derived、PNG 解码、HTML 和历史写入；按实际数据流核算，不机械把 204 MiB 改成 240 MiB 了事。
9. `retained_bytes` 计入 derived，所有并发工作继续持有同一 pipeline lease；失败/取消/过期分支释放 lease。
10. 历史提交成功之前不 ACK、不广播；旧 observed 的晚到结果不能 ACK 新复制。
11. 健康失败时只按既有关闭/legacy 路径运行，并受 W02 门禁；运行中的一次归档超时不是立即改用文本的理由。
12. 保留 `read()`、`--read-snapshot` 的 Partial 回读等用途；若确需修改，先补对应行为测试。

### 10.1 接缝回归清单

| 类别 | 必须断言 |
| --- | --- |
| 成功 | PNG、DIB-only、HTML、RTF 各保留正确归档；便携结果符合现有能力 |
| NativeOnly | 不支持表示仍保留原件与事实，不广播伪造便携内容 |
| 文件 | 正常及大量文件到 FilesFallback，混合形状不静默丢部分 |
| 取消/竞态 | revision 改变、系统唤醒、recovery generation 改变均丢旧结果 |
| 存储 | DB 不可用/事务失败时不 ACK、不广播、不假记已保存 |
| 网络图片 | 自动归档失败仍保源；设置关闭时不新增网络访问 |
| 来源 | 自写不回声、相同内容新复制不被吞、Partial 不假确认 |
| 内存 | reserve 失败、转换失败、保存失败和超时均释放 lease |
| 健康 | 新旧 helper 错配、缺 helper、功能关闭、已有 v13 可读性 |
| 隐私 | 全部开启/关闭/回退分支受 W02 门禁 |

### 10.2 G1：第一轮复核

交付以下材料后停下复核，不自动进入 W06：

- 实际改动文件与 W00–W05 状态；新旧 helper 兼容决策。
- DIB 派生、FilesFallback、SelfWrite、Partial、抑制与事务失败的测试证据。
- helper CLI 与解析契约，已执行的命令、退出码、命中数量。
- Windows 原生尚未验证的项目和需要人类操作的最短步骤。
- 明确结论：`CODE_READY` / `WINDOWS_VERIFIED` / `NO_GO`；不得只说“全部完成”。

## 11. W06：完整元数据枚举与类型化读取

**目标**：扩大可观察事实，而不是扩大任意内存、缓存与回放权限。

**前置**：G1 通过；生产前有 Windows 原生环境；W04 来源矩阵覆盖扩展后的真实系统表示。

### 11.1 格式处理表

| 类型 | 元数据 | 第一版正文读取 | 转换/恢复 |
| --- | --- | --- | --- |
| 已支持文本、HTML、RTF、PNG、DIB | 记录 | 类型化、有界 | 延用既有规则，DIB 用已验证派生 |
| 已确认契约的注册 HGLOBAL 格式 | 记录 | 逐名/逐形状准入；明确有效长度 | 默认不增加回放 |
| 未知注册格式 | 记录 | 默认 Absent | 无 |
| 无名字的私有数值格式 | 记录数值存在 | 默认 Absent | 不承诺跨进程/跨会话身份 |
| GDI 句柄格式 | 记录 | 未实现专用提取前 Absent | 不保存句柄值；W07 单独设计 |
| `FileContents`、OLE 流/存储 | 记录已观察描述符 | 默认 Absent | W07 决策，不当内存数组 |
| 文件选择与辅助格式 | 用于类型化分类 | 不读取引用文件正文 | 文件管线 |
| 隐私/来源控制 | 仅按控制契约 | 必要的固定控制载荷 | 不作为普通可发布内容 |

### 11.2 完整性与限制

1. 完整保留能表达的已观察格式及其读取状态；未知格式未读取仍有证据。
2. 如果超过 `FORMAT_LIMIT`，为预算保留的条目应确定、有优先级；不得任意截断后声称完整。
3. 诊断可以报告更多元数据；生产 CNA1 仍遵守 64 条 original+derived 的共同上限。
4. `win32EnumerationComplete=true` 仅证明一次 Win32 元数据枚举正常结束；不能自动升级 CNA1 的全源完整性、OLE 覆盖或可回放承诺。
5. 本轮 `enumeration_complete` 默认仍为 false；只有独立复核明确其 Windows scope、遗漏处理、序列化语义和 OLE 边界后才能改为 true。
6. 即使将来元数据完整，某格式 `Absent` 仍阻止完整原件声明；不能用“已枚举”替代“已捕获”。
7. 同一标准类型可能有系统可合成表示；不能据出现顺序武断断言哪个由应用显式设置。

### 11.3 G2 决策产物

新增 `docs/WINDOWS-CLIPBOARD-SHAPE-EVIDENCE-<实际日期>.md`，逐应用给出四轴结果，并明确：

- **A：标准字节已足够** → 不写应用适配器。
- **B：需额外通用派生** → 进入 W07 的单项设计。
- **C：需 OLE 多对象** → 另立 OLE 子方案；未批准前只保留描述符。
- **D：需读取应用缓存** → 先做安全与授权设计，再考虑 W08。
- **E：缺样本或不可证明** → UNKNOWN，不能排期成“即将支持”。

**验收**：生产与诊断在相同 revision、相同 scope、相同截断规则下的格式事实一致；差异有明确原因，不要求不同复制瞬间的列表机械相等。

## 12. W07：按证据增加通用能力（条件任务）

本任务不是一张“全部实现”的清单。每一项必须由 G2 的真实样本触发、独立测试和复核。

### 12.1 GIF/WebP/JPEG 等静态封面

- 仅支持已有或获批准的解码器；明确帧、像素、尺寸和输出字节上限。
- 动画原件不变，静态封面必须声明为派生，不能声称动画已跨端保真。
- `image::is_image_format_name` 支持某个名字，不代表 `from_archive` 已接受相应派生组合；补齐实际准入测试。
- 仅因转换失败不得丢失可安全保留的原件。

### 12.2 EMF/WMF 或 GDI 图像

- 先决定如何获得可序列化的格式字节，再讨论如何有界渲染 PNG；不能把句柄整数写入归档。
- 专用读取、系统句柄生命周期、COM/GDI 资源释放、超时、渲染尺寸与依赖处理都必须有设计。
- 不默认执行外部引用、嵌入对象或任意本地文件访问。
- 系统 API 和新增 Rust 依赖均须评估；`image` 的普通位图 feature 不代表能解析 EMF/WMF。
- 未完成专项安全和原生消费测试时，保持 Absent/Unsupported，不以“Word 常见”作为实现依据。

### 12.3 RTF 附件

分别验证 RTF 顶层、PNG 附件、JPEG 附件、EMF/WMF 附件及对象附件。附件可见性、UTF-16 位置、缺失占位和样式分析分别验收。

扩展时走已定义的保存分析/内容分析契约，不把 RTF 偷换成 RTFD，不从 RTF 中任意路径提取文件。`--analyze-rtfd` 在 Windows 继续明确拒绝。

### 12.4 OLE 多对象

只有样本证明 Win32 基础路径不足时才做，子方案至少写清：

1. `OleGetClipboard` / `IDataObject` 的获取、COM apartment、线程与初始化要求。
2. `EnumFormatEtc`、`FORMATETC`、`lindex`、`tymed`、descriptor 数量与有界 GetData。
3. `STGMEDIUM` 和接口的释放、超时可终止性、来源退出后的失败状态。
4. 虚拟文件是否归入文件管线，还是只生成便携图片；不把它擅自解释为已有文件路径。
5. 多对象到 CNA1 item 的精确定义；现有模型若不能无损表达，就单独讨论协议，不强塞。
6. 大对象数量、流大小未知、同格式多 lindex、失败后的不完整性与防路径穿越。

未获批准前，不新增 OLE 内容读取，也不以格式数量填 `items`。

## 13. W08：逐应用适配与必要设置（条件任务）

### 13.1 一个应用一个小闭环

每次只支持一个已证明的应用版本/剪贴板形状，按以下顺序交付：

1. 脱敏形状证据和失败复现。
2. 识别器正例、近似形状反例、版本变化反例。
3. 只解析当前剪贴板中该应用的表示，不扫描聊天历史。
4. 生成有来源的便携 HTML/Text/PNG；图片顺序和文本间位置明确。
5. 缺图原位占位；图片数、可用数、缺失数可核对，不能静默减图。
6. 关闭适配器后，不读缓存、不访问网络、不留下仍运行的异步任务。
7. Windows 原生采集、Windows 预览、另一端消费和目标应用粘贴分别有证据。

不因“名字相似”就放开 `qq_projection` / `qt_image` 的 macOS 平台条件。只有数据结构、source、converter、item 关系和依赖语义均经证明后，才考虑共享识别器的平台允许列表。

### 13.2 缓存读取的最低安全条件

“消息里明确引用某个路径”不是授权边界；剪贴板输入不可信。（MS2）

- 必须是用户明确授权的应用缓存根，不扫描账号目录、不解析数据库、不读取正文历史。
- 路径定位只能来自当前样本中的已验证引用；不得从文件名猜测或枚举全盘。
- 约束最终打开对象位于允许根内，并处理重解析点、junction、符号链接、设备路径、UNC、相对逃逸和替换竞态。
- 不允许任意 URL 获取；公开图片仍受既有网络归档策略，应用适配器不新增绕过通道。
- 限制文件数、单文件大小、总字节、像素和时间；只接受明确的普通图片文件。
- 授权失效、关闭、权限不足、路径不安全或格式不符时留占位；不能提升权限或改读其他目录。
- 不要求管理员权限、不注入、不安装钩子、不做 UI Automation 抓取来绕过格式不足。

安全边界无法证明时停在 Unsupported/UNKNOWN，不实现“临时可用”的宽松读取。

### 13.3 设置生成与 UI

仅当适配器已存在且确需用户控制时新增设置。无缓存读取的标准字节支持不制造虚假授权开关。

新增设置的真实顺序是：核实 Rust Settings 定义及 schema 导出器 → 修改源模型及默认值/patch 验证 → 运行 `shared/schema/generate-settings.mjs` → 检查 schema 与 TS 输出 → 接线 Windows 设置、双语文案和测试。

不要只手改 `settings.schema.json` 或 `.generated.*`。跨平台共享字段必须验证 macOS 读取旧/新设置的兼容性；不为 Windows 引入一份平行设置模型。

## 14. W09：原生、包装与双端验收

### 14.1 Windows 原生环境

记录：准确 Windows 版本/补丁、CPU 架构、Rust/Node 版本、源码 HEAD+dirty 摘要、helper 和 app 文件路径/哈希、协议探测输出、安装/便携形态、测试应用准确版本。

原生写剪贴板测试必须：停止可能广播测试内容的 TailSync 实例及其他测试进程，使用专用非敏感样本，人工确认可以覆盖当前剪贴板，串行执行。

不要承诺用旧白名单快照“完整恢复此前剪贴板”。未知格式/虚拟对象可能无法重建；应使用专门准备的测试剪贴板，而不是冒险备份再覆盖真实工作内容。

### 14.2 四轴结果矩阵

| 场景 | 采集/归档 | 本地预览 | 跨端同步 | 恢复/实际粘贴 |
| --- | --- | --- | --- | --- |
| Unicode 文本 | 原文与控制边界正确 | 正确 | 双向内容正确 | 无回声、换行正确 |
| HTML + 两张公网图 | 原件及资源绑定可核对 | 图文顺序、缺图占位 | 双向位置与已存图片正确 | 有效便携内容，不假称原件 |
| PNG | 原始 PNG 在 original | 像素正确 | 双向正确 | 目标应用能消费 |
| DIB-only / DIBV5-only | 原件 + 明确 PNG 派生 | 方向、alpha 正确 | 静态 PNG 正确 | 便携粘贴；原件仅在实际允许时测试 |
| RTF + PNG 附件 | 原件保留 | 附件与位置正确或明确部分支持 | 按实际便携能力 | 不伪造 RTFD |
| 普通/大量文件 | FilesFallback | 既有文件历史 | 既有文件批次 | 不重复发送文件路径文本 |
| 未知私有格式 | 存在事实与 Absent | 不宣称有可用图片 | 无伪造内容 | 原件回放不越权 |
| 虚拟文件 | 描述符和未知边界 | 不伪装本地文件 | 未实现记 UNKNOWN | 未实现不得 PASS |
| 隐私标记 | 不读取/不入历史 | 无新增内容 | 不发送 | 不改剪贴板 |
| 应用适配样本 | 逐应用、逐版本 | 图片数和顺序 | 双向可核对 | 目标应用真实消费 |

每格使用 `PASS / FAIL / UNKNOWN / NOT_APPLICABLE` 并附证据；不能只在每行填一个“支持”。

### 14.3 故障、回退与安装矩阵

至少覆盖：helper 旧/新错配、缺失、不可执行、超时、崩溃、Busy、Unstable、64 格式边界、总预算边界、DB 不可用、旧库升级、已有 v13 关闭态、存储迁移、睡眠恢复、应用退出、同内容再次复制、发布回执丢失和 Partial。

包装时默认态与 composite 开启态分别构建；dev resolver、便携包、安装后 resolver 均检查。正式记录不可使用 `SkipChecks` 或 `SkipSmokeTest`。

## 15. 验证命令与执行分层

以下命令由执行者按改动范围执行。本文编写阶段没有运行它们，也没有承诺它们在未来环境中均已通过。

### 15.1 纯模型与共享层（仓库根目录）

先列出测试，确认新增 `wcap_` 回归存在，再跑最小过滤；无命中不能计为通过。

```sh
node scripts/run-isolated-tests.mjs -- cargo test --locked -p tailsync-protocol -- --list
node scripts/run-isolated-tests.mjs -- cargo test --locked -p tailsync-clipboard-helper -- --list
node scripts/run-isolated-tests.mjs -- cargo test --locked -p tailsync-protocol wcap_
node scripts/run-isolated-tests.mjs -- cargo test --locked -p tailsync-clipboard-helper wcap_
node scripts/run-isolated-tests.mjs -- cargo test --locked -p tailsync-core --features protocol-composite-clipboard wcap_
```

然后执行相关完整套件和默认/开启态：

```sh
node scripts/run-isolated-tests.mjs -- cargo test --locked -p tailsync-protocol
node scripts/run-isolated-tests.mjs -- cargo test --locked -p tailsync-clipboard-helper
node scripts/run-isolated-tests.mjs -- cargo test --locked -p tailsync-core
node scripts/run-isolated-tests.mjs -- cargo test --locked -p tailsync-core --features protocol-composite-clipboard
node scripts/run-isolated-tests.mjs -- cargo test --locked -p tailsync-runtime
node scripts/run-isolated-tests.mjs -- cargo test --locked -p tailsync-runtime --features tailsync-core/protocol-composite-clipboard
```

`tailsync-runtime` 没有名为 `protocol-composite-clipboard` 的本包 feature，因此上面使用依赖 feature `tailsync-core/protocol-composite-clipboard`。开工时仍需核实 package 名和 feature。若某测试放在另一个已存在 crate，调整到真实目标并在台账记录；不通过随意加依赖来适配本文命令。

### 15.2 格式、生成与契约

```sh
cargo fmt --all -- --check
cargo fmt --manifest-path windows/src-tauri/Cargo.toml --all -- --check
cargo fmt --manifest-path macos/src-tauri/Cargo.toml --all -- --check
node windows/scripts/check_cross_platform_sync.mjs --win-root windows --mac-root macos --core-root shared/rust-core
node scripts/check-local-contracts.mjs
node scripts/generate-native-capture-policy.mjs --check
git diff --check
```

只有相关 schema 源确实变化时才运行相应生成器；生成结果必须逐项审阅。若改变设置：

```sh
node scripts/run-isolated-tests.mjs -- node shared/schema/generate-settings.mjs
node scripts/run-isolated-tests.mjs -- node shared/schema/generate-settings.mjs --check
```

Clippy 对实际受影响 crate 和默认/开启态执行 `--all-targets -- -D warnings`。不擅自加 `allow` 掩盖新问题；宿主裁剪产生的已知平台警告按仓库既有检查方式处理。

### 15.3 Windows Rust 与前端（Windows 本机）

```powershell
node scripts/run-isolated-tests.mjs -- cargo test --locked --manifest-path windows/src-tauri/Cargo.toml --lib
node scripts/run-isolated-tests.mjs -- cargo test --locked --manifest-path windows/src-tauri/Cargo.toml --lib --features protocol-composite-clipboard
npm --prefix windows test
npm --prefix windows run build
npm --prefix windows run lint
powershell -NoProfile -ExecutionPolicy Bypass -File windows/scripts/build-composite-helper.ps1
```

这些常规测试不得隐式写真实剪贴板。显式 ignored 原生测试另行获得确认后，设置仓库已有的测试开关并串行执行；先读测试源码确认数据安全，不在本文给一条可误触发的自动写剪贴板命令。

宿主 Windows crate 检查使用仓库实际脚本 `scripts/check-windows-host.sh`，先读使用方式；`portable-check` 只提供类型检查证据，不替代 Windows 链接和原生 I/O。

### 15.4 包装（Windows 本机）

在确认磁盘位置和依赖已就绪后，使用新的输出目录，避免覆盖已有制品：

```powershell
$RunId = Get-Date -Format 'yyyyMMdd-HHmmss'
powershell -NoProfile -ExecutionPolicy Bypass -File windows/scripts/package-windows.ps1 -BuildDirectory "target-package-wcap-default-$RunId" -OutputDirectory "release-wcap-default-$RunId"
powershell -NoProfile -ExecutionPolicy Bypass -File windows/scripts/package-windows.ps1 -CompositeClipboard -BuildDirectory "target-package-wcap-composite-$RunId" -OutputDirectory "release-wcap-composite-$RunId"
```

原生证据的日期与时区记录实际测试机信息，不把执行主机默认时间直接当作本文日期。哈希应来自实际构建产物，不引用旧包哈希。

### 15.5 macOS 与双端

涉及 canonical shared adapter、共享投影、恢复或来源规则时，按 `CONTEXT.md` 跑受影响的 macOS Rust 默认/开启态检查；涉及显示契约或设置生成时补 Swift 解码/视图测试。

双端至少对文字、HTML+多图、PNG、DIB 派生和文件分别执行两个方向；先停用自动广播进行写剪贴板夹具测试，再在明确的双端场景中开启同步。两类测试不能混跑。

## 16. 必须停下的条件

出现以下任一项，不继续扩大范围：

1. 新增改动无法与用户其他未提交修改安全分离。
2. 需要修改网络协议、主历史 schema、原件身份域、持久化来源域或旧 intent 兼容语义。
3. 需要放宽 `windows_publishable`、跨平台原件拒绝或 macOS 适配器门禁。
4. 需要读取应用缓存、账号目录、数据库、网络路径或未经批准的 OLE 正文。
5. 无法证明隐私标记在 legacy/文件回退路径不会被绕过。
6. 任一自写回声、伪造标记误判、同内容新复制被吞、Partial 假确认仍存在。
7. 新增外部依赖而没有许可、安全、包体积和预算决策。
8. G1/G2 未通过，却需要据此推断应用格式或 Windows 原生行为。
9. 需要写真实剪贴板、启动生产 daemon、读取真实聊天或覆盖安装包，但没有相应的人类确认。

停下时报告：已完成内容、最小复现、需要哪项决定、可继续的不依赖工作。不要以“整体复杂”代替具体阻塞，也不要把 UNKNOWN 改成 PASS。

## 17. 最终交付与完成标准

### 17.1 文件产物

- 本任务的最小代码改动及对应测试。
- helper 版本兼容决策记录。
- 分任务执行台账。
- 脱敏 Windows 形状证据，缺环境时列明确待采样项，不造样本。
- 原生/包装/双端验收矩阵，保留命令、退出码、平台、应用版本、包哈希与证据路径。
- 相关能力文档和版本矩阵更新；封存历史验收文档只追加引用，不改写其旧协议证据。

### 17.2 第一轮完成定义

只有同时满足以下条件，才可称 W00–W05 源码完成：

- 新增采集进入 CNA1 platform 2，原件和派生明确分离。
- 既有 PNG/HTML/文本能力不退化，DIB-only 的派生被共享层实际接纳。
- FilesFallback、隐私、回声、事务、重试和内存约束均有真实生产接缝测试。
- helper 健康/版本/迁移门禁没有被绕过，macOS 行为不退化。
- 没有宣称完整枚举、原件去重、OLE 或第三方应用支持已经自然获得。

Windows 原生、目标应用与双端缺口仍单列；未通过时不得称产品可发布。

### 17.3 给人类的最终汇报模板

```text
本轮范围：Wxx–Wyy
源码结论：CODE_READY / NOT_READY
Windows 原生：PASS / FAIL / UNKNOWN
目标应用：逐应用列版本及四轴结果
双端：PASS / FAIL / UNKNOWN
本轮改变：3–6 条
未改变：协议/schema/权限/回放边界
验证：实际命令、退出码、命中数、证据路径
未验证：明确原因与下一条可执行动作
发布判断：GO / NO_GO / 不在本轮范围
下一门禁：G1 / G2 / 人类确认事项
```

## 18. 可直接交给执行模型的首轮提示词

下面是第一轮任务，不是整份计划的一次性全量实施授权：

```text
请在 TailSync 仓库执行 docs/WINDOWS-THIRD-PARTY-IMAGE-EXECUTION-PLAN-2026-11-05.md 的 W00–W05，并在 G1 停下汇报。

先阅读适用的 AGENTS.md、CONTEXT.md、相关 ADR 和执行方案；保护当前所有已有未提交修改。先建立执行台账，逐任务先补最小回归再实现，测试使用仓库隔离器，记录真实命中数量、退出码和平台。

重点遵守：Win32 格式数不等于 item 数；未知句柄不通用 GlobalLock；隐私门禁覆盖 legacy 与文件回退；CNA1 原件不包含伪造的转换 PNG；DIB 派生必须通过 from_archive 的实际准入；FilesFallback、自写回声、Partial、历史事务和 pipeline lease 不得退化；第一轮 enumeration_complete=false，不扩展 Windows 原件 dedupe key。

helper 版本变更先建独立兼容决策，未获所需确认时继续纯模型和测试准备，不绕过门禁。不修改网络协议、主历史 schema、产品版本或 macOS helper 4。不要实施 W06–W09，不新增缓存读取、OLE 正文、EMF/WMF 依赖或第三方设置。

没有 Windows 原生环境就把对应验收标 UNKNOWN，继续可完成的宿主纯测试，不把交叉检查当原生 PASS。不要自动写真实剪贴板、读取真实聊天、运行生产 daemon、安装应用、创建分支或提交 commit。

最终报告本轮改动、验证证据、未验证项、兼容性和 G1 所需决策。任何新增安全或版本边界按方案的停止条件处理。
```

## 19. 官方依据与适用范围

以下微软官方资料在 2026-11-05 查阅。正文是针对本仓库的设计归纳，不是 Windows 已为 TailSync 提供这些能力或权限的承诺。

- [MS1：EnumClipboardFormats](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumclipboardformats)：枚举顺序、结束/错误区分、系统可合成表示。
- [MS2：GetClipboardData](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboarddata)：输入不可信、句柄所有权、关闭后的使用限制。
- [MS3：Standard Clipboard Formats](https://learn.microsoft.com/en-us/windows/win32/dataxchg/standard-clipboard-formats)：各标准格式实际载荷/句柄类型。
- [MS4：Clipboard Formats](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats)：多表示、系统合成、历史与云同步控制格式。
- [MS5：Shell Data Object](https://learn.microsoft.com/en-us/windows/win32/shell/dataobject)：FORMATETC、STGMEDIUM、lindex、流与存储介质。
- [MS6：Shell Clipboard Formats](https://learn.microsoft.com/en-us/windows/win32/shell/clipboard)：CF_HDROP、虚拟文件描述符、FileContents 与 Shell 辅助格式。
- [MS7：Clipboard Operations](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-operations)：打开/关闭和延迟渲染。
- [MS8：GetClipboardFormatNameW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboardformatnamew)：注册格式名称与截断行为。

若执行时官方资料、当前源码或目标应用样本与本文不同，记录差异并回到对应决策点，不靠猜测补齐。
