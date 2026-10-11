# 复合内容预览只显示摘要：修复与验证（2026-10-07）

## 根因

用户截图中，历史条目的分类是“图片”，并已有缩略图，但预览显示 `composite.txt`、22 bytes 和 `Rich clipboard content`。

这三个字段来自共享核心的旧预览分支：`HistoryDB::prepare_preview` 遇到任何 `type='composite'` 条目都返回清单摘要，并将其当作文本。无正文的图片摘要正是 22 字节的 `Rich clipboard content`。因此故障发生在内容读取层，图片渲染器根本没有收到图片。

真正的图文文稿已有独立的 `get_document_preview` 通路；图片分类使用普通二进制预览通路，之前遗漏了这一分支。

## 修改

`shared/rust-core/src/db/reads.rs` 的 `prepare_preview` 先读取已提交的展示分类。当分类为 `image` 时，转入已有的 `prepare_composite_image`，读取并验证保存的首图，返回图片类型、尺寸和像素。

该路径复用原有身份校验、部件摘要校验、取消检查、首图顺序和内存预算。图片缺失或损坏时返回错误，不用摘要伪装成内容，也不以第二张图片替代缺失首图。原始捕获、存储类型和回溯剪贴板操作不变；无需数据库迁移。

Mac 和 Windows 的预览入口都调用 `HistoryOperations::preview_binary_async`，因此共享修复同时覆盖两端的普通预览。图文文稿仍使用原有文稿渲染路径。

## 验证

| 验证 | 结果 |
|---|---|
| 修复前 Core 回归 | 失败：应返回 Image，实际返回 Text |
| 修复前 Runtime 二进制回归 | 失败：应返回 image，实际返回 text |
| 修复后 Core 与 Runtime 回归 | 2 项通过；检查保存的像素与最终传输像素完全一致 |
| Core 常规全量测试，开启复合剪贴板 | 单元 739 通过，4 个既有辅助/性能入口 ignored；顶层集成 2 项通过，2 个既有子进程 worker 入口 ignored（由父测试启动） |
| Runtime 常规全量测试 | 单元 58 通过；内容分析、展示分类、恢复计划集成共 28 项通过 |
| Swift 预览、文稿、二进制、键盘专项 | 49 项通过，0 跳过，0 失败 |
| Windows 二进制、图片、文稿预览专项 | 16 项通过 |
| Rust fmt 与 Clippy `-D warnings` | 通过 |

Core 回归覆盖纯 PNG 和 HTML 包裹的图片，并通过正式分类提交接口模拟后台分类重建。Runtime 回归从隔离历史库调用生产预览用例，检查 TSPV 类型、条目 ID、请求 ID、尺寸和像素。Swift 回归从 TSPV 解码到实际图片 materialization，确认不进入文本视图；既有图文文稿和上轮窗口遮挡回归也通过。

首次测试脚手架的反序列化类型错误，以及首次 HTML 用例缺少分类提交的问题，均已修正并保留尝试日志；它们不计入通过证据。正式普通全量测试不包含 opt-in `repair-regressions` 或长时性能基准，不替代此前阶段的完整封存矩阵。

测试只使用合成内容和隔离数据目录；Swift API socket 指向不存在的测试路径。本次未读取用户历史库、QQ 内容或系统剪贴板，也未停止用户当前应用。

证据目录：`artifacts/clipboard-improvement/2026-10-07-composite-image-preview-fix-r1/`。最终源码指纹：`c7f40bf6acd894dca0271d9aa4f65a37b070bad453189fa9e6ef8d298c3c841f`。与上轮窗口修复相比，产品代码只修改 `reads.rs`；另有三处回归测试文件修改。既有封存证据保持原样，仅对应其原指纹。

Windows 原生、用户当前历史条目的实际 GUI 操作和双设备验收仍需另行确认。

## 修复包与复查

新包：`artifacts/clipboard-improvement/2026-10-07-composite-image-preview-fix-r1/TailSync-ContentPreviewFix.app`，版本 2.3.2，arm64。Rust 守护进程已按 release、复合剪贴板开启态重新编译；包经过 ad-hoc 签名和严格校验。

Swift 主程序和 clipboard helper 复用上轮窗口修复包。已核对 70 个相关产品源码文件未变。重新签名整个包会改变 Swift 主程序的签名及完整文件摘要；在私有临时副本上移除签名后，新旧 Swift 主程序内容完全相同，helper 的完整文件摘要也保持一致。首次交付清单因误要求重新签名后的 Swift 文件摘要不变而生成失败，该尝试不作为成功证据。最终清单分别记录复用输入、签名输出和去签名内容的摘要。上轮预览窗口自动关闭的修复仍包含在新包中，旧包全部文件仍与旧清单一致，正在运行的旧实例保留。详细构建命令、测试结果、源码和包内文件摘要见同目录 `candidate-manifest.json`。

1. 从 TailSync 菜单退出当前旧实例，打开 `TailSync-ContentPreviewFix.app`。
2. 直接预览截图中原来只显示提示的历史记录，无需重新复制。图片分类条目应显示保存的图片，并出现既有图片缩放/旋转控件。
3. 再预览一条真正的文字加图片文稿，确认文字和图片保持原有阅读顺序。
4. 在历史和收藏中检查空格预览、上下键跟随，以及预览完全遮住宿主窗口时仍保持打开。

构建和测试均使用独立缓存，交付前清理本次 Rust target 和 Swift scratch，保留新包及日志。
