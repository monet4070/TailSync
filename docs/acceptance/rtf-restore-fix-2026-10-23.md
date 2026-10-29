# RTF 默认回溯修复（2026-10-23）

RTF 单表示和 text+HTML+RTF 混合样本均已通过采集、历史持久化、新连接读取、默认回溯、字节回读及预览。修复包已替换到 `/Applications/TailSync.app`；历史、设置和引导选择保留。

## 根因与修复

初次矩阵显示：有效 RTF 被保存，但同时声明的 UTF-8/UTF-16 格式在 helper 退出后读不到，导致默认回溯返回 `originals_incomplete`。

进一步用仍存活的写入进程对照后确认：这些纯文本转换依赖写入进程。`NSPasteboardItem`/`writeObjects` 发布 RTF，再加入 TailSync 来源标记等格式，会使隐式转换成为持久声明；短生命周期 helper 退出后，转换数据却未保留。单独等待 200 ms、要求立即提供数据或调用 `PasteboardResolvePromises` 都不能完整解决来源标记组合。不能仅凭格式名称或系统 flags 断言某一描述符可删除。

`--write-snapshot` 现使用系统 `PasteboardCreate`、`PasteboardClear`、`PasteboardPutItemFlavor` 直接发布预先校验的 Data。保留项目边界、原格式顺序、原字节及操作 token；每次写入都提交实际数据，不注册延迟提供者，也不让 AppKit 为 RTF 追加不可兑现的转换承诺。此 API 保留原生格式偏好顺序，并在失去所有权后拒绝旧写入。Swift 自动管理 CF 引用生命周期。

全部输入校验仍发生在 clear 之前。原有 revision 检查、token 回读、Published/Partial/Superseded/Failed 回执和外层有界进程监督保留；原生写入返回错误时停止，并通过回执报告。没有新增自动重试、协议版本、数据库迁移或历史完整性豁免。

根因位于 macOS helper。Core、Runtime 的 `originals_complete` 与 `originals_incomplete` 规则没有修改，真实缺失或读取失败的原件仍拒绝完整恢复。

## 验证

| 检查 | 结果 |
|---|---|
| 旧版 helper 执行新增回归 | 失败：退出后存在不可读的 promised format，保留原始失败日志 |
| 新打包 helper 与安装后 helper 私有板自检 | 各 13 组通过；验证退出后的完整集合、6 种 text/HTML/RTF 排列、粗体、多项目 RTF、显式 UTF-16 不改写、过期 revision 和坏输入不清空 |
| 新包原生格式矩阵 | 24 个样本默认回溯全部通过；恢复后的源指纹与实际回读一致，避免自写误判为新复制 |
| 文件矩阵 | 61 个文件样本入库、重开、单条/整批恢复及预览帧字节检查通过 |
| Swift 实际预览矩阵 | 同一历史记录的 85 帧全部通过，包括 RTF、RTFD、图文、图像解码、PDF、Markdown、SVG WebKit 和 Office 材料化 |
| 原生故障注入 | 最终完整执行中，clear 后卡死、写入后丢回执、启动失败、输出超限及 17 文件分类契约通过；首轮异常见下文 |
| 并发复制 | 旧 RTF 写入在 clear 后停顿，期间发布新复制；旧写入返回 Superseded，新复制字节和 revision 保持 |
| 平台采集回归 | 31 项通过；2 个可选矩阵随后显式执行 |
| Runtime 恢复与历史策略 | O06 15 项、history 11 项通过，包含“不完整原件拒绝默认恢复、显式兼容仍可用”的负向回归 |
| 打包 | 深度严格签名校验通过；arm64，最低 macOS 13.0；daemon 哈希保持，UI 代码段保持，仅签名随 App 封装更新 |

安装后 Settings 窗口正常显示实际设置和存储状态；UI 与 daemon 均运行。没有在生产历史或 general 剪贴板中注入样本，所有格式试验使用独立数据库和 `tailsync-probe.*`。此结论属于本机原生 helper/生产逻辑/真实渲染器验证，不替代每种来源应用的 GUI 粘贴，也不认证 Windows 原生或双设备传输。原有动画静态封面、未知格式摘要及缺图网页的能力限制保持，见原格式矩阵。

首轮故障注入的 `RR-04-contract` 返回 `exited`，剪贴板为空，未满足预期的 `timed_out`，因此首轮结果没有计为通过，原始 `native-faults.json` 保留。追加的 6 次直接调用和 4 次生产监督器调用均按预期超时；新增退出成功状态、输出字节数和耗时诊断后，最终完整执行也通过，记录于 `native-faults-final.json`。首轮原因尚未确定，不能把未复现解释为已修复；汇总结果保留这一待观察项。该诊断只输出状态和计数，不打印剪贴板正文。

## 已存历史

已保存为不完整的历史不会被重写或标为完整。其未知/失败描述符仍保留：可显式选择 RTF 复制，或从来源重新复制。新版发布的有效 RTF 内容不会再因这次 helper 缺陷引入空的转换承诺。

## 证据与复现

- [结果、源码与二进制摘要](../../artifacts/rtf-restore-fix/2026-10-23/results.json)
- [修复前格式矩阵](clipboard-format-matrix-2026-10-23.md)，其 `artifacts/format-roundtrip/2026-10-23` 证据未覆盖。
- [探针及格式矩阵说明](../../tools/clipboard-probe/README.md)。新矩阵使用 `TAILSYNC_FORMAT_MATRIX_DIR=/Users/monet/Documents/TailSync/artifacts/rtf-restore-fix/2026-10-23`。

App 包与可回退旧包位于 `/Users/monet/Downloads/TailSync构建/2026-10-23-RTF回溯修复`，安装过程和来源记录于该目录的 `build-install-manifest.json`。
