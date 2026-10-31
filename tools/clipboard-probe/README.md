# 剪贴板格式探针

探针只输出格式、长度、稳定性和摘要，不输出正文、不读取历史、不联网。使用当前工作树编译的 helper：

```sh
swiftc -O -target arm64-apple-macos13.0 macos/src-tauri/clipboard-helper.swift -o /tmp/tailsync-composite-helper
python3 tools/clipboard-probe/probe.py --helper /tmp/tailsync-composite-helper --self-test
```

`--self-test` 使用独立命名 pasteboard，覆盖文字/HTML/PNG/RTF、整组发布、回执、过期 revision 和发布前拒绝。额外核对 helper 退出后的 CNA1 原件集合、RTF 粗体、6 种富文本格式顺序、多项目 RTF 与显式 UTF-16 原字节，防止短生命周期发布者留下不可读的转换承诺。不会修改 general pasteboard。

真实应用复制后，可运行 `probe.py --helper … --source-app Chrome --source-version …` 读取当前 general pasteboard 的受支持格式元数据。它不执行复制或粘贴，也不代表目标应用验收。保留来源版本、操作系统、实际粘贴结果；微信、Word、Notes 和 Windows 原生结果单独记录。未提供对应环境时保持 UNKNOWN。

当前限制：不采集任意未知/OLE 格式；读取超大 native 格式仍由 helper 进程级超时保护。多 native items 不会静默合并。图片存在但读取失败会显示格式错误。

## 历史与预览格式矩阵（macOS，可选验收）

`prepare_format_fixtures.py <输出目录>/fixtures` 生成当前 Swift 文件格式清单的全部扩展名样本。
Python 环境需提供 python-docx、python-pptx、reportlab、openpyxl；图片使用仓内公开测试素材与系统 ImageIO 编码。
旧 PPT 使用 [Apache POI 的真实样本](https://raw.githubusercontent.com/apache/poi/trunk/test-data/slideshow/basic_test_ppt_file.ppt)，
先保存为 `<输出目录>/fixtures/slides.ppt`，并记录下载摘要与 Apache-2.0 来源。

把待验收 App 的 `Contents/MacOS/clipboard-helper` 放到 macOS Cargo 测试程序所在的 `debug/deps/clipboard-helper`，
再通过隔离入口执行，`TAILSYNC_FORMAT_MATRIX_DIR` 为**绝对输出目录**：

```sh
node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
  --manifest-path macos/src-tauri/Cargo.toml --features protocol-composite-clipboard \
  --lib format_history_restore_preview_matrix -- --ignored --nocapture --test-threads=1
```

显式执行两个 `#[ignore]` 的平台测试：原生格式走真实采集事务；文件走系统文件列表读取、共享批次准备与持久化。
两者均重开加密历史的新连接，验证恢复后的原生格式或文件字节，并输出实际 TSPV/TSDC 预览帧。
仅操作 `tailsync-probe.*` 私有粘贴板；不启动生产 daemon，也不向真实设备发送样本。
原生矩阵遇到默认恢复被拒会保存该行的失败，再检查其余格式，最终以非零退出保留验收失败。

构建 Swift 测试后，在同一环境变量下执行
`xctest -XCTest TailSyncTests.HistoryFormatRoundtripTests <TailSyncPackageTests.xctest>`，
让实际预览解析器、图片解码器、PDFKit、Markdown、图文排版和 SVG WebKit 消费上述帧。
Office 再用从历史预览帧取出的字节生成系统 Quick Look 缩略图，避免把“签名检查通过”等同于“渲染成功”。
完整记录见 [2026-10-23 验收](../../docs/acceptance/clipboard-format-matrix-2026-10-23.md)。
