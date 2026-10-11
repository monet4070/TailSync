# O03 图片原件与动画：验收文档（2026-10-06）

**结论：O03 本机实现及最终自动验证达到 LOCAL_COMPLETE。DeepSeek/high 首轮实施已由主代理独立复核、二次修复。A12 真实来源／目标应用、Windows 原生、双设备与 hosted CI 仍 UNKNOWN，Release NO-GO。** 本项完成后停止；下一项 O04 未启动。

唯一工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`；分支 `codex/composite-clipboard`；HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。既有改动、日常 App、用户库与历史证据保留，未暂存或提交。

| 阶段 | 源码指纹 | 文件数 |
|---|---|---|
| O03 实施前 | `364fbe7819331b2e28f3eb2477e219f1a32a691dba3e4469fec58493bb984aec` | 771 |
| DeepSeek/high 交接 | `b9642eb1d1cef164a08b10c3a321b78510a8d8b62ccf57e6e3f0e29f43e480f1` | 772 |
| 主代理最终验收 | `92dfd9399876deaf3fdce0148eb66bcc55e58cda80cad8c2c9d8a782b69c7a29` | 783 |

证据目录：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35/artifacts/clipboard-improvement/2026-10-06-o03-image-implementation/`。最终判定采用 `validation-results.json` 与主代理 `review/commands/*-final-v1.json`；首轮源码、草稿与记录单独保留。

## 1. 实际修改效果

- macOS CNA1 原件采集继续保存实际字节。PNG、TIFF、JPEG、GIF、WebP 的通告类型各按 **32 MiB** 准入；GIF/JPEG/WebP 不再受未知 opaque 类型的 4 MiB 上限限制。仍遵守整份 native archive **40 MiB** 及既有格式数／项目数上限；超限记读取状态，不冒称完整保存。
- APNG、GIF、JPEG、WebP 与多页 TIFF 的原件和派生静态 PNG 分开保存。封面带 `source_name` 与 `tailsync.image-static-cover.v1` 角色，原件字节不被封面替换。
- 单帧 APNG 仍视为 APNG 容器，走原件身份与封面路线。现有 `png_to_packed` 对 APNG 的拒绝保持不变。
- 静态封面只解码第 0 帧，先核验 **不超过 20 帧、累计不超过 8×1024×1024 像素**。TIFF 按每页实际尺寸累计；无法解码或预算不足时保留已准入原件，不制造封面。
- 原生 `--write-snapshot` 只回放已准入的原始字节，不借用封面解码预算。超过便携像素预算的静态 PNG 使用原件来源指纹域；分类与发布使用同一判断。
- 普通静态 PNG 原样投影；已有静态 PNG 原件优先于 JPEG/TIFF/GIF 等表示派生的封面。封面不能覆盖更完整的静态 PNG，也不能借任意文本来源进入图片投影。
- 图片原件的 MIME、尺寸与帧数由保留字节按需计算。纯图片归档通过**现有加密摘要字段**进入正式历史列表，例如 `image/gif · 4×3 · 2 frames · static PNG cover`；无封面且已知超预算时说明 `cover budget exceeded`。混合文稿保留正文摘要，未增设独立的封面 UI 标记。
- `PortableBundle` 仍只有 text/html/rtf/png。同步兼容结果和现有预览使用静态 PNG；未新增 GIF/WebP/APNG wire MIME，未绕过能力协商，未承诺跨设备保留动画。

## 2. 生产调用链

1. `macos/src-tauri/clipboard-helper.swift --read-archive` 在稳定 revision 中枚举类型；`original` 保存通告字节，`derived` 保存转换结果。`staticCover` 与既有 TIFF 单帧转换共用实际解码路径。
2. `shared/platform-clipboard-composite.rs::read_archive` 解码 CNA1。macOS monitor 调用 `shared/rust-core/src/composite/mod.rs::from_archive`，构建兼容投影；非便携编码原件仍留在归档。
3. `HistoryDB::add_native_archive` 保存加密 manifest、CNA1 和便携表示；`image_summary` 从原件生成历史摘要。`prepare_read_chunk → matching_entries → summary_bytes` 是本轮验证的正式列表读取路径。
4. `get_composite_archive` 在重开后校验归档及原件身份；Runtime `restore_payload` 和已有 RestorePlan 使用 `NativeArchive::publication()` 准备原件快照，经 supervisor、来源指纹与 helper `--write-snapshot` 集中发布。
5. PNG 预览／同步仍沿既有 `prepare_composite_image` 和 PortableBundle 路线。缺封面时不会将动画字节塞入静态 PNG 解码器。

`shared/tailsync-protocol/src/image.rs` 只读容器结构，不解码像素。PNG 的 acTL 记录声明帧数；GIF 计完整描述符；TIFF 检查有终点的 IFD 链并累计各页；WebP 检查 RIFF 范围、ANMF 与图像载荷头；JPEG 读取合法 SOF 尺寸。**这些事实不是整个压缩载荷可成功解码的证明。** 截断、循环、结构不支持、扫描范围或枚举预算不足时返回未知，已准入原件仍保留。

## 3. 独立复核与二次修改

| 首轮问题 | 独立证据／最终处理 |
|---|---|
| 截断 acTL 对缺失正文切片，可能 panic | 首轮复制源码复现越界；改为检查整个块范围后读取，保留截断前缀回归 |
| acTL 声明一帧时误判普通 PNG | 首轮复现；任何合法 acTL 均标 APNG，Rust 与 Swift 判断一致，真实单帧 APNG 固定样本通过 |
| TIFF 循环伪造页数，后续大页面漏算 | 首轮两项复现；访问集合检测循环、逐页尺寸／标量检查、实际累计像素与溢出检查 |
| WebP 空帧、错误 RIFF 长度；GIF 缺 trailer 仍产生已知事实 | 增补结构范围检查，结构不完整返回未知；真实 WebP/GIF 与 malformed 边界回归通过 |
| 200 帧枚举限额和 helper 20 帧解码限额混写 | 分开容器枚举与封面预算；大图片仍有元数据，但不借此准入解码 |
| 静态封面转换器来源过宽，可能替换已有原生 PNG | 限定图片来源及静态 PNG 输出；真实静态原件优先，增加生产投影行为回归 |
| 元数据未进入历史效果；原件回放借用解码预算 | 接入现有加密摘要和列表读取；原件回放与原件指纹使用独立预算边界 |
| 辅助 API／测试价值偏低 | 删除仅被自身测试调用的 `NativeArchive::static_cover()`、仅测试使用的 `declared_matches()`；统一 TIFF／封面转换，移除重复像素解码实现 |
| JPEG/TIFF/WebP 等测试只提供头部，却容易被解读为完整图片证据 | 改用 10 个真实可解码固定样本；生产函数检查与容器结构测试分别报告 |

实现继续复用 NativeSnapshot、CNA1、历史 parts、来源指纹和 RestorePlan。未新增持久化字段、schema 或 helper／本地契约／wire 版本；冻结旧 v2 manifest 的摘要回归保持通过。新增解析集中在一个有界模块，平台像素解码集中在 helper，职责与预算明确。

测试没有为每个字段堆叠同义断言：容器结构／畸形边界、正式 DB 重开／列表／投影、实际 ImageIO 颜色／透明度／封面预算分别覆盖不同风险。删除的辅助测试不作为通过数保留；既有 ignored 注解未增加或放宽。

## 4. 最终验证

以下 18 项检查都以最终指纹执行，执行前后源码一致，退出码均为 0。完整 argv、耗时、原始日志、摘要与计数见 `review/commands`、`review/logs` 和 `validation-results.json`。

| 检查 | 最终结果 |
|---|---|
| Protocol all-features | 48 PASS，0 ignored |
| Core lib：repair-regressions、test-support、composite、两项传输扩展 | 666 PASS，4 既有 ignored |
| Core integration：repair_regressions＋composite_repair_followup | 14 PASS |
| Runtime all-features＋Core composite | 45 PASS |
| macOS Rust lib composite | 118 PASS，3 既有 ignored |
| Windows Rust crate 在 macOS 宿主上运行 | 127 PASS，2 既有 ignored；不代表 Windows 原生验收 |
| workspace Clippy all-targets/all-features `-D warnings` | PASS |
| workspace／macOS／Windows 三份 fmt | PASS |
| helper 图像函数隔离检查 | 57 个实际断言通过 |
| 完整 Swift helper typecheck | PASS；没有运行 general 剪贴板分支 |
| production features、canonical includes、local contracts、native policy 生成一致性 | 全部 PASS |
| capture failure visibility、git diff --check | 全部 PASS |

Windows 宿主日志保留原有 `clipboard_file.rs` 的 Win32／InvalidData 未构造 warning；文件未改，也未删除 Windows 分支或隐藏告警。

关键验收证据：

- 真实 2 帧 GIF 原件 → 封面投影 → 加密历史保存 → 关闭／重开 DB → 原件字节、原件身份、帧数仍一致，正式列表带原图信息和封面角色。
- 真实 APNG 无封面时保留为 NativeOnly，有封面时只将封面投影为 PNG；透明 PNG 原字节透传；JPEG/TIFF 原件与兼容表示分开。
- Rust 全样本截断前缀安全检查、单帧 APNG、TIFF 循环／不同页大小、WebP 错误长度／空帧、GIF 缺 trailer 等边界通过。
- `scripts/test-image-covers.py` 编译 helper 的**实际图像函数段**，不打开 pasteboard。8 种支持样本的封面为单帧 PNG，首帧尺寸一致，透明与不透明采样点的 alpha／RGB 差不超过 1/255；21 帧 GIF、16 Mpixel PNG 拒绝封面解码。本机 ImageIO 的 WebP 样本也通过；不外推到其他系统解码器或任意色彩配置。
- 10 个固定样本的完整解码核验、字节数／SHA-256／帧尺寸登记在 `review/fixture-inventory.json`。Pillow 仅用于生成／核验样本，不是产品或最终测试运行依赖。

失败尝试也保留：首轮复制 parser 的 **5 PASS／4 FAIL** 在 `review/logs/preliminary-parser-red.log`，四项均已修复；主代理第一次摘要测试误把 CBP1 当文本的 **5 PASS／1 FAIL** 在 `core-focused-v1.log`，改用正式列表 API 后通过。DeepSeek 的同名门禁记录存在重跑覆盖，因此最终判定只使用不可覆盖的主代理记录；子代理完整命令／输出历史另存 `agent/thread-record-completed.json`，不把首轮记录当成最终源码证据。

DeepSeek 首轮私有命名板 GIF/APNG/JPEG 探针保留，但绑定交接指纹 `b9642eb1…`。二次修改后未重跑原生 pasteboard 探针；最终 helper 证据限于实际图像函数与完整 typecheck。真实发布、目标应用粘贴和整组动画恢复仍须按下节执行。

## 5. 支持范围与待验收门槛

| 编码 | 已实现的本机处理 | 尚不能据此确认 |
|---|---|---|
| 静态 PNG | 原件、透明通道与兼容投影保持；超像素预算可保留原件 | 任意目标应用、巨图压力与完整 GUI |
| APNG（含单帧 acTL） | 原件与静态封面分开；静态解码门禁不放宽 | 来源实际提供动画、目标播放动画、最终 native 发布 |
| GIF | 原件、帧数与加密历史重开；有界首帧封面 | QQ 等真实来源、关闭应用后的目标粘贴与动画播放 |
| JPEG | SOF 尺寸、原件与 ImageIO PNG 封面 | 任意 ICC／CMYK／MPO 及跨平台色彩一致性 |
| TIFF | 标准 IFD 页数、逐页预算、原件与首帧封面 | BigTIFF、长尾编码／色彩、多页目标应用 |
| WebP | RIFF／帧头事实、原件与本机 ImageIO 封面 | 其他系统解码能力、目标动画播放与跨设备原件传输 |

容器枚举最多 200 帧，扫描范围 4 MiB；未知元数据不推导完整帧数。原始字节与封面分别准入。超过原始字节／总归档上限的格式不会被保存为完整原件；不能把“无封面仍保留已准入原件”推广到已被字节额度拒绝的内容。

A12 自动部分 PASS；完整 A12 **仍待验收**：

1. 同一真实来源和目标，比较直接粘贴与历史恢复的原件摘要、帧数、动画播放、尺寸和透明度；关闭来源、重启 TailSync 后重复。来源只给静态像素时结果应保持静态，不声称恢复动画。
2. 验证最终 helper 的私有发布与真实目标应用行为，尤其单帧 APNG、超封面预算但已准入的原件、已有静态 PNG 的格式优先级。
3. Windows 原生采集／回放、Mac↔Windows 和双设备分别验收。当前 Windows helper 未实现 CNA1 `--read-archive`；动画原件跨 wire 未实施，静态兼容也未有双设备证明。
4. hosted CI、真实内存／截止时间压力，以及长尾颜色配置仍 UNKNOWN。

没有替换或启动日常 App，没有读写 general 剪贴板或用户库；主代理只做源码与隔离验证。10 份历史封存共 8384 个文件摘要复算一致（S0 用既有 `.txt` 归档路径映射），旧 seal 不变。

## 6. 回退与暂停点

本轮相对正式基线共 22 个源码／测试／配置文件新增或变化，零删除；清单见 `changed-source-files.json`。`baseline`、`agent-source`、`final-source`、两份阶段 patch、文档副本与不可覆盖测试记录供审阅和定向回退。不得按 HEAD 整体重置，否则会损失此前阶段和用户未提交改动。

回退 O03 时按本轮增量恢复 helper／policy／projection／fingerprint／summary 与新测试，不修改已有数据库或原件。CNA1 原件及旧 manifest 仍可读取，但旧实现不会使用新封面转换器，GIF/JPEG/WebP 将恢复旧 opaque 额度；回退后展示／兼容／回放能力需要重新核验。冻结 manifest 回归证明的是摘要读取兼容，不能据此承诺旧二进制拥有新图片能力。

最终状态：**O03 LOCAL_COMPLETE；完整 A12 UNKNOWN；Release NO-GO。本轮结束，O04/F04/F06 未启动。**
