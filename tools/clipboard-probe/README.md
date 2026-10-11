# 剪贴板格式探针

探针只输出格式、长度、稳定性和摘要，不输出正文、不读取历史、不联网。使用当前工作树编译的 helper：

```sh
swiftc -O -target arm64-apple-macos13.0 macos/src-tauri/clipboard-helper.swift -o /tmp/tailsync-composite-helper
python3 tools/clipboard-probe/probe.py --helper /tmp/tailsync-composite-helper --self-test
```

`--self-test` 使用独立命名 pasteboard，覆盖文字/HTML/PNG/RTF、整组发布、回执、过期 revision 和发布前拒绝。不会修改 general pasteboard。

真实应用复制后，可运行 `probe.py --helper … --source-app Chrome --source-version …` 读取当前 general pasteboard 的受支持格式元数据。它不执行复制或粘贴，也不代表目标应用验收。保留来源版本、操作系统、实际粘贴结果；微信、Word、Notes 和 Windows 原生结果单独记录。未提供对应环境时保持 UNKNOWN。

当前限制：不采集任意未知/OLE 格式；读取超大 native 格式仍由 helper 进程级超时保护。多 native items 不会静默合并。图片存在但读取失败会显示格式错误。
