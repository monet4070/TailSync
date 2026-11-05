# Windows 第三方应用图片链路：执行台账

日期：2026-11-05（Asia/Shanghai）。
执行范围：W00–W05。G1 之前未进入 W06–W09。
执行方案：`docs/WINDOWS-THIRD-PARTY-IMAGE-EXECUTION-PLAN-2026-11-05.md`。

## W00 基线与工作树保护

状态：VERIFIED（文档/事实层）
基线：HEAD `916855c964c10ce6cdcf5363402688fcd94489d9`
平台：Windows x64，Rust 1.91.0 x86_64-pc-windows-msvc，Node v24.18.0。
已有未提交修改：`shared/rust-core/src/composite/html.rs`、`shared/rust-core/src/composite/mod.rs`、`windows/src-tauri/src/clipboard.rs`。全部保留并在其上增量修改；未执行 reset/checkout/stash。

核对结果：

- helper 自检（旧运行时）：`CWH1 protocol=2 analyze=1`，退出码 0。
- 新 helper 自检：`CWH1 protocol=3 analyze=1`，退出码 0。
- macOS helper 协议保持 4。
- `NATIVE_LIMIT=40 MiB`、`ITEM_LIMIT=16`、`FORMAT_LIMIT=64`。
- `read_archive` 执行前仅 macOS 可用且只接受 platform 1；现在 macOS 接受 1、Windows 接受 2，各自按编译目标校验。
- 运行中的产品来自本工作树分支：`F:/TailSync/wpf-network-aligned-20261031`（2.3.2，WPF 宿主 + `TailSync.Backend.exe` + helper）。
- 兼容决策：`docs/adr/ADR-WINDOWS-HELPER-PROTOCOL-3-2026-11-05.md`（PROPOSED，G1 决策项）。

## W01 元数据取证

状态：CODE_READY；真实第三方应用形状 UNKNOWN
改动：`tools/clipboard-helper/src/format_model.rs`（纯模型）、`windows.rs` 的 `--dump-formats`、`main.rs` 模块注册。
内容：

- `EnumClipboardFormats` 区分正常结束与错误。
- 标准格式固定名称表；`GetClipboardFormatNameW` 只用于注册格式。
- 无名字私有格式使用 `private-format-0xNNNN` 诊断显示名，不承诺跨进程身份。
- 只输出元数据；`readAttempted=false`、`byteLength=null`，上限 256 条/256 KiB。
- 多个 Win32 格式仍只表示一个语义 item（`itemCount=1`）。

验证：

- `cargo test -p tailsync-clipboard-helper`：10 passed、0 failed（含 6 个 `wcap_`）；1 个原生写剪贴板测试保持 ignored。
- 本机 `--dump-formats`：退出码 0，输出 `diagnosticVersion=1`、`stable=true`、`win32EnumerationComplete=true`，当次剪贴板为 `CF_HDROP`。
- QQ/微信/Word/Chrome/资源管理器等应用形状：未由执行者人工采样，UNKNOWN。

## W02 隐私门禁

状态：CODE_READY；真实 Windows 标记 UNKNOWN
改动：`shared/tailsync-protocol/src/native_policy.rs`、helper `read_archive`、`shared/platform-clipboard-composite.rs` 的 `privacy_gate`、`windows/src-tauri/src/clipboard.rs` legacy 预检。
内容：

- `ExcludeClipboardContentFromMonitorProcessing` 存在即整事件抑制。
- `CanIncludeInClipboardHistory` / `CanUploadToCloudClipboard` 必须恰为 4 字节 DWORD 且值为 0/1；0 抑制，非法值失败关闭。
- 抑制发生在正文枚举/读取前；抑制事件零 item、零 format、零 derived。
- helper 与实际采集在同一次打开事务内检查。
- legacy 文本/图片/文件路径与 composite 关闭态在 daemon 侧再次预检；revision 改变则丢弃本次许可。
- 控制检查失败记录固定故障类别，不回退到另一读取路径。

验证：

- `tailsync-protocol wcap_`：2 passed（保守值判断、抑制时 body read 计划为 0）。
- Windows lib `suppressed_decision_has_no_side_effects`：通过。
- 真实第三方应用控制标记：本轮未构造原生样本，UNKNOWN。

## W03 最小 CNA1 与 DIB 派生

状态：CODE_READY；真实 DIB/DIBV5 剪贴板 UNKNOWN
改动：`tools/clipboard-helper/src/windows.rs`（`--read-archive`）、`native_archive.rs` 常量、`composite/mod.rs` 派生准入。
内容：

- Windows helper 新增 `--read-archive`，编码 CNA1 `platform=2`、`enumeration_complete=false`。
- 保留既有标准格式采集范围；未知格式正文不读取。
- DIB/DIBV5 原件保留在 `original`；PNG 仅写入 `derived`，converter 为 `tailsync.windows-dib-to-png.v1`。
- 已有 `PNG` 原件优先；损坏/动画 PNG 不替换。
- `from_archive` 精确准入：platform 2、来源 `CF_DIB`/`CF_DIBV5`、converter、输出名 `PNG`、静态 PNG；来源必须是已捕获原件。
- 原 PNG 存在时忽略 DIB 派生。
- 派生超预算时保留原件、不扩展原件去重域。

验证：

- `tailsync-core` 新增 8 个 `wcap_` 测试，全部通过：DIB-only、伪造来源/converter/platform/name、PNG 优先、损坏 PNG、DIB+HDROP、纯 HDROP、自写回声。
- 本机 `--read-archive`：退出码 0，输出以 `CNA1` 开头；未打印正文。
- 真实 DIB/DIBV5 复制样本：未执行（需要人工复制），UNKNOWN。

## W04 文件分类、回声与恢复接缝

状态：CODE_READY
内容：

- 新增 `PNG + CF_HDROP` 与 `CF_DIB/V5 + CF_HDROP` 的图片判定，避免把带临时文件的图片当作纯文件选择。
- 纯 `CF_HDROP` 仍走 FilesFallback；正文/图片混合不静默丢部分。
- 自写判定继续使用 token + revision + 内容指纹；DIB 派生写入后回读仍识别 SelfWrite，同像素的新 revision 仍为 External。
- 异平台原件回放继续拒绝；便携 PNG 独立投影。
- `enumeration_complete=false`，不扩展 Windows 原件 dedupe key。

验证：

- 核心 `wcap_` 8 项通过。
- Windows 生产接缝 `wcap_dib_derived_png_reaches_history_and_broadcast` 通过：原始 CF_DIB 存 CNA1，PNG 只在 derived，历史与广播均发生。
- 现有 FilesFallback、Suppressed、NativeOnly、Partial 和事务失败回归保持通过。

## W05 Windows 生产接线与第一轮交付

状态：CODE_READY；WINDOWS_VERIFIED（helper/backend resolver/只读采集）；目标应用 UNKNOWN
改动：`shared/platform-clipboard-composite.rs`、`windows/src-tauri/src/clipboard.rs`、`windows/native-ui/build-wpf.ps1`、能力文档。
内容：

- Windows `HELPER_PROTOCOL_VERSION = 3`，macOS 保持 4。
- `read_archive` 在 Windows 可用并按 platform 2 校验。
- `CaptureEnvironment` 生产依赖切换为 `read_archive`；`process_composite_capture` 用 archive.original 匹配 revision，转换使用 `from_archive`。
- 成功便携捕获存 `add_native_archive[_with_images]`；NativeOnly 存 CNA1 而不是 CNS1。
- FilesFallback 分支保留；Empty/Suppressed/Busy/Unstable/Failed 不落假成功归档。
- 内存 reserve 为 204 MiB，`retained_bytes` 计入 derived；pipeline lease、ACK、广播、事务和恢复 generation 语义未改。
- WPF 构建/发布脚本的 helper 协议检查更新为 3。
- 文档版本矩阵更新为 Windows helper 3。

验证：

- `cargo test -p tailsync-protocol --lib`：75 passed。
- `cargo test -p tailsync-clipboard-helper`：11 passed、1 ignored。
- `cargo test -p tailsync-core --lib`：1021 passed、3 ignored；`--features protocol-composite-clipboard`：1024 passed、3 ignored。
- `cargo test -p tailsync-runtime --lib`：92 passed、1 ignored；带依赖 feature 同样 92 passed、1 ignored。
- Windows `cargo test --manifest-path windows/src-tauri/Cargo.toml --lib`：199 passed、3 ignored；带 `protocol-composite-clipboard`：200 passed、3 ignored。
- `npm --prefix windows test`：63 files / 522 tests passed；`npm --prefix windows run build` 通过；`npm --prefix windows run lint` 退出码 0（仅既有 warning）。
- `cargo fmt --all -- --check`、Windows manifest fmt、`check_cross_platform_sync.mjs`、`check-local-contracts.mjs`、`git diff --check` 均通过。`generate-native-capture-policy.mjs --check` 在生成器写出 LF 时通过；本机 `core.autocrlf=true` 会把该生成文件恢复为 CRLF，随后字节比较只因行尾报告 stale，`git diff` 内容为空（既有 Windows 行尾问题，未顺手修改生成器或 `.gitattributes`）。
- clippy：protocol/helper 通过；core 与 Windows 在本机 rustc 1.91 上先暴露两个既有问题（`db/storage.rs` type_complexity、`clipboard.rs:45` needless_borrow），加上对应 `-A` 后本任务代码 0 warning。未顺手修改既有问题。
- Release 构建：`windows/src-tauri` + helper 构建成功。
- 阶段目录 `F:/TailSync/wcap-stage-20261105`：
  - `TailSync.Backend.exe` SHA256 `6017AEF53081EA70B39CF9AF2A7520ECC83088CBF61D061151512B4AD4C660C5`
  - `tailsync-clipboard-helper.exe` SHA256 `2B96AE173F444CCDEA05EF12B5D41AE5E1A4C3AAFDF5D8ADC0BB98B5F7B59741`
  - backend `--tailsync-clipboard-helper-check`：healthy=true、protocol=3、composite_compiled=true。
- 未替换运行中的 `F:/TailSync/wpf-network-aligned-20261031`；未启动生产 daemon，未写真实剪贴板。

## 运行目录替换与原生验证

时间：2026-11-05 17:06（Asia/Shanghai）。
备份：`F:/TailSync/wpf-backups/third-party-image-20261105-170553`，含旧 backend/helper、`BUILD-PROVENANCE.old.json` 与旧 `history-v2.db`（13 行，schema 13）。

已替换运行目录 `F:/TailSync/wpf-network-aligned-20261031`：

- `TailSync.Backend.exe` SHA256 `6017AEF53081EA70B39CF9AF2A7520ECC83088CBF61D061151512B4AD4C660C5`
- `tailsync-clipboard-helper.exe` SHA256 `2B96AE173F444CCDEA05EF12B5D41AE5E1A4C3AAFDF5D8ADC0BB98B5F7B59741`
- WPF 宿主、产品版本、历史 schema 未替换。

启动与健康：

- `TailSync.Wpf.exe` PID 44712，主窗口“剪贴板历史”，Responding=true。
- `TailSync.Backend.exe` PID 56768，Responding=true。
- `TailSync.Backend.exe --tailsync-clipboard-helper-check`：`healthy=true`、`expected_protocol=3`、`composite_compiled=true`、helper 路径为运行目录。
- helper `--dump-formats`：退出码 0，`stable=true`、`win32EnumerationComplete=true`、`readAttempted=false`。

端到端 DIB-only 验证（人工合成 2×2 非敏感图，不来自聊天）：

- 通过 helper `--write-snapshot` 写入仅 `CF_DIBV5`（无 PNG）的剪贴板；receipt=`published`。
- 5 秒后历史从 13 行变为 14 行；最新行 `id=1364`、`type=composite`、`category=image`。
- `history_parts`：`representation`（PNG）80 bytes + `archive` 400 bytes；说明 PNG 仅进入便携表示，原件归档保留。
- `history_composite_identity`：`content_id` 为 `hcn1:2:...`，`archive_revision=1`。
- 再等待 4 秒历史仍为 14 行，未出现回声/重复行。
- 验证后已把剪贴板恢复为原先两个计划文件（`CF_HDROP + DataObject`）；历史行数保持 14。

未验证：QQ/微信/Word/Chrome 的真实 DIB/HDROP 混合形状、Mac↔Windows 双端粘贴、目标应用实际粘贴；均保持 UNKNOWN。
## 恢复与同步补充验证（2026-11-05 17:33–17:50）

修复：Windows 第一轮归档的 `enumeration_complete=false` 不能再阻止默认恢复。
`prepare_composite_for_target` 现在对 `platform=2`、`native_archive_complete=Some(false)` 的条目回退到便携投影；
macOS 的 O01 严格行为保持不变。新增测试：

- `wcap_windows_incomplete_originals_restore_the_portable_projection`：通过。
- `o01_incomplete_originals_refuse_default_but_keep_explicit_compatibility`：保持通过。

运行目录第二次替换：

- 备份：`F:/TailSync/wpf-backups/third-party-image-restore-fix-20261105-173326`（旧 backend + 旧历史库）。
- `TailSync.Backend.exe` SHA256 `9A5C1A1161289D41C4615554B2DF4EA69538729100678D195EFE1E96EF7FEB46`。
- 最终运行进程：WPF PID 54860，backend PID 61128，均 Responding。

恢复实测：

- 在历史列表中选中 1.9 MB（400×600）图片并按 Enter：剪贴板出现 `PNG + CF_DIBV5 + org.tailsync.clipboard-source`，恢复成功。
- 选中 15.6 MB（931×1920）图片并按 Enter：同样出现 `PNG + CF_DIBV5 + source`，大图恢复成功。
- 恢复后历史行数未因自写增加（19 行保持）。

同步实测（临时 `RUST_LOG=debug`，验证后已正常重启）：

- `delivery session ready` 至 `monetdeMacBook-Air.local`（iroh，1391 ms）。
- 合成 DIBV5 图片捕获日志：`composite capture provenance ... formats: ["png"], completeness: "complete"`。
- `delivery completed ... command: CompositePayload`，Mac 端完成接收 ACK。
- 说明 Windows 便携复合图片已进入真实发送队列并由对端接收；目标端历史/粘贴的可见性仍由 Mac 端确认。

当前配置恢复：剪贴板已恢复为原先两个计划文件的 `CF_HDROP + DataObject`；debug 日志环境只用于本次验证，最终进程已正常重启。
## G1 复核

状态：WAITING_FOR_HUMAN
源码结论：CODE_READY
Windows 原生：PASS（helper 自检、metadata dump、read-archive 命令、backend resolver）；写剪贴板与真实 DIB 采集 UNKNOWN
目标应用：QQ / 微信 / Word / Chrome / 资源管理器 / 企业微信等：UNKNOWN（无人工采样）
双端：UNKNOWN（本轮未做 Mac/Windows 实机双端）
本轮未改变：网络协议 v4/v5、主历史 schema、产品版本 2.3.2、macOS helper 4、原件去重域、回放白名单、第三方设置
发布判断：NO_GO（目标应用与双端未验收；不得把 CODE_READY 当作可发布）
下一门禁：

1. 人类确认 helper protocol 2 → 3 的兼容决策。
2. 是否允许把阶段目录中的 backend/helper 替换到运行目录并重启验证。
3. 人工用非敏感样本复制 QQ/微信/Word/Chrome 图片，记录格式形状与四轴结果。

## R7 实测问题复盘与修复（QQ 图片无法回溯与同步，2026-11-05）

### 实证诊断结论（基于 `history-v2.db` 行 `1365..1378`、`history_composite_events` 与 `tailsync.log`）

1. **根因 1（P0，QQ 图片 100% 无法跨端同步）**：
   - 实证：`history-v2.db` 中行 `1366, 1367, 1368, 1372, 1375, 1376` 均为 QQ 图片复合记录，其 `CNA1` 原件格式为 `CF_DIBV5 + CF_DIB + CF_HDROP + HTML Format`，衍生格式为 `PNG`（`source_name = "CF_DIBV5"`）。但在 `history_composite_events` 表中，这 6 条 QQ 图片记录**全部缺失**（仅行 `1378` 纯文本生成了同步事件），`tailsync.log` 每次捕获 QQ 图片均在 `save_portable_history` 后输出 `warn: composite capture provenance is unknown; keeping local history without peer broadcast`。
   - 机理：`windows/src-tauri/src/clipboard.rs` 对 `archive.original` 调用 `env.classify(&snapshot, &bundle)`。`shared/rust-core/src/composite/fingerprint.rs` 的 `requires_original_domain` 对包含 `CF_HDROP` 的快照返回 `true`（`requires_original_fingerprint("CF_HDROP") == true`），将 QQ 附带临时文件路径的图片分流进 `origin_fingerprint_from_capture(&snapshot)` → `origin_fingerprint_from_native(&snapshot)`。而 `origin_fingerprint_from_native` 对 `DEFERRED` 集合中的 `"CF_HDROP"`（以及 `> 4 MiB` 的 `CF_DIBV5`）调用 `macos_publishable` 直接返回 `Err(ProtocolError::InvalidComposite)`，导致 `OriginTracker` 将其判为 `Origin::UnknownMismatch`，进而在 `shared/platform-clipboard-capture-publish.rs:138` 阻断 `broadcast_composite_to_peers`。
2. **根因 2（P0，回溯粘贴至 QQ 失败 + QQ `CF_DIBV5` 解码首行 3 像素偏移）**：
   - 实证：`history-v2.db` 行 `1370, 1371` 显示 helper 写入 `CF_DIBV5` 后，Windows 合成 `CF_DIB` 失败（`GetClipboardData(CF_DIB)` 返回 `NULL`，记录为 `FormatState::Failed`）；同时行 `1376`（1200×1560 QQ 图片）的 `CF_DIBV5` 总长为 `7,488,136 = 124 (BITMAPV5HEADER) + 12 (3×DWORD bitfields masks) + 7,488,000 (pixels)`。
   - 机理：
     - `encode_dib_v5` 先前写入负高度（top-down）且 `bV5Compression = 3 (BI_BITFIELDS)`，但未附带 12 字节掩码表（总长仅 `124 + pixels`），导致 Windows `win32k` 无法将其合成为 `CF_DIB` / `CF_BITMAP`，依赖 `CF_DIB` 的目标应用（如 QQ）在回溯后无法粘贴。
     - `decode_dib` 仅在 `header == 40` 时将像素偏移加 `12`；面对 QQ 的 `header == 124` + `BI_BITFIELDS` + 尾部 12 字节掩码表（`[0x00ff0000, 0x0000ff00, 0x000000ff]`）时，把 12 字节掩码当成前 3 个像素读取，造成整张图片像素错位 3 像素。
3. **根因 3（P1，大图 `NATIVE_LIMIT` 40 MiB 预算被重复 `CF_DIB` 挤占）**：
   - 实证：行 `1372`（2560×1440 QQ 图片）在 `CNA1` 中同时采集了 `CF_DIBV5`（14.75 MB）与 `CF_DIB`（14.75 MB），光原件就消耗 29.5 MB；且 `known` 列表中 `"PNG"` 排在 `CF_DIBV5` / `CF_DIB` 之后。一旦单图 RGBA 超过 ~20 MB（仍在 32 MiB `RGBA_LIMIT` 内），重复采集 `CF_DIB` 会耗尽 40 MiB `NATIVE_LIMIT`，导致衍生 `PNG` 无法生成、条目退化为 `native_only`。
4. **根因 4（P2，无 `CF_HDROP` 时的本机 `file://` HTML 包装残留）**：
   - 机理：`from_native` 仅在 `image_with_backing_file` 为真时剥离仅含本机 `file://` `<img>` 的 `HTML Format`；若存在 Windows 位图但未附带 `CF_HDROP`，本机路径包装 HTML 会使 `Completeness` 降级为 `ExternalReferences`。

### 代码修复与回归验证

- `shared/rust-core/src/composite/fingerprint.rs`：`requires_original_domain` 对 `has_backing_file_image(snapshot)` 快照豁免 `"CF_HDROP"` 的原件域强制要求，使其走便携域像素指纹（`origin_fingerprint(bundle)`）。
- `shared/tailsync-protocol/src/bitmap.rs`：
  - `decode_dib` 识别 `header >= 108` 且 `compression == 3` 时紧随头部的 12 字节 `RGB` 掩码表并跳过 12 字节；同时支持 `compression == 0 (BI_RGB)` 且 `header >= 108` 时的 `bV5AlphaMask`。
  - `encode_dib_v5` 改为标准正高度（bottom-up 行序）、`bV5Compression = 0 (BI_RGB)`、`bV5Intent = 4 (LCS_GM_IMAGES)`，保证 Windows 原生合成 `CF_DIB` / `CF_BITMAP` 成功且与 `decode_dib` 精确往返。
- `tools/clipboard-helper/src/windows.rs`：`read_archive` 与 `snapshot` 将 `"PNG"` 移至 `CF_DIBV5`/`CF_DIB` 之前，并在 `CF_DIBV5` 已成功捕获且通过 `bitmap::decode_dib` 校验时跳过冗余的 `CF_DIB` 读取。
- `shared/rust-core/src/composite/mod.rs`：公开 `pub(crate) fn has_backing_file_image`，并在存在 Windows 位图且 `HTML Format` 仅含本机 `file://` 图片而无正文文本时剥离该本机 HTML 包装。
- 回归测试：
  - `tailsync-protocol`：76 passed（含 `dibv5_bitfields_with_trailing_color_table_skips_masks`、`alpha_top_down_round_trip` 2×2 多行验证）
  - `tailsync-clipboard-helper`：11 passed
  - `tailsync-core`：1,026 passed（含 `wcap_dib_with_backing_file_is_an_image_not_a_file_selection` 原件域与 HTML 剥离断言）
  - `tailsync-runtime`：93 passed
  - `windows/src-tauri`：201 passed（含新增 `wcap_qq_backing_file_image_reaches_history_and_broadcast`）
- 运行目录更新（`F:/TailSync/wpf-network-aligned-20261031`）：
  - 备份目录：`F:/TailSync/wpf-backups/qq-image-sync-restore-fix-20261105`
  - `TailSync.Backend.exe` SHA256：`DD433ED056370F33E60A28ABCBD548EF2217052E0B4A32B18D19DD64B9CCF025`
  - `tailsync-clipboard-helper.exe` SHA256：`6A775D9DC929A4DB15E07AD82F33DE2CD06D0990FE8807224ED65624584335D4`
  - 进程状态：`TailSync.Wpf.exe`（PID 68556，Responding=True）、`TailSync.Backend.exe`（PID 35556，Responding=True）。

## R8 WPF 键盘回溯后历史窗口自动关闭与焦点归还修复（2026-11-05）

### 现象与根因诊断

- **用户反馈**：使用键盘回溯之后历史记录窗口不会自己消失。
- **根因分析**：
  1. TailSync.Wpf/MainWindow.xaml.cs 中 Restore 方法签名原定义为 private async Task Restore(bool closeOnSuccess = false)，默认 closeOnSuccess 为 alse。
  2. 在列表项键盘事件 History_KeyDown 中，按回车键原调用 wait Restore();（未传参默认 alse），执行 lse Notify(result.Message); 弹出 toast，但从未调用 Close()。
  3. 双击列表项 History_DoubleClick 同样调用 wait Restore(); 未传参，导致双击回溯后窗口也不关闭。
  4. 当焦点停留在搜索框 SearchBox 中时，Window_KeyDown 未监听 Key.Enter，导致用户搜索到目标后直接敲回车无法触发回溯。
  5. 运行目录 F:/TailSync/wpf-network-aligned-20261031/ 下的 TailSync.Wpf.dll 与 TailSync.Wpf.exe 仍为 2026/10/31 构建，未包含任何窗口关闭逻辑更新。

### 代码修复与验证

- **代码修改**（windows/native-ui/TailSync.Wpf/MainWindow.xaml.cs）：
  - History_KeyDown：if(e.Key==Key.Enter){e.Handled=true;await Restore(true);}，回溯成功且完整后自动调用 Close()。
  - History_DoubleClick：双击时调用 wait Restore(true);。
  - Window_KeyDown：改为 sync void，补充搜索框内回车触发回溯：
    lse if(e.Key==Key.Enter&&!e.IsRepeat&&Keyboard.Modifiers==ModifierKeys.None&&SearchBox.IsKeyboardFocusWithin&&ViewModel.IsQueryCurrent&&ViewModel.SelectedEntry!=null){e.Handled=true;await Restore(true);}；同时增加 Esc 键清空搜索或关闭窗口。
  - 窗口关闭生命周期：Closed 事件中早已实现 ForegroundTracker.Restore()，在调用 Close() 后能准确恢复打开历史记录前的前台活动窗口（如 QQ、Word 等），方便用户直接粘贴。
- **测试与构建**：
  - TailSync.Native.Checks：27 项原生检查全部通过（ExitCode 0）。
  - dotnet build & dotnet publish：使用 .NET 10 SDK（10.0.403）重新编译并自包含发布 win-x64。
- **运行目录部署**：
  - 备份目录：F:/TailSync/wpf-backups/wpf-keyboard-restore-fix-20261105
  - 部署文件：TailSync.Wpf.dll、TailSync.Wpf.exe、TailSync.Wpf.pdb、TailSync.Native.Core.dll、TailSync.Native.Core.pdb 至 F:/TailSync/wpf-network-aligned-20261031/。
  - 运行状态：进程已重启，TailSync.Wpf.exe（PID 43788）与 TailSync.Backend.exe（PID 69620）均处于活跃正常状态。
