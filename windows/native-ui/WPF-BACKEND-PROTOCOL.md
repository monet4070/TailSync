# WPF 本地后端传输

WPF 使用现有 Rust 同步、数据库、剪贴板、配对、主题和存储服务。独立后端仍由 Tauri 的原生事件循环承载，但 WPF 模式不会创建 WebView 窗口，不启动 localhost API 监听器。

构建 `windows/src-tauri` 的 `tailsync.exe` 后可将其作为 `TailSync.Backend.exe` 与 WPF 宿主一起发布。当前后端来自本工作树，支持 `main@b61a0a0` 前端命令的超集。

## 启动

宿主创建 32 个十六进制字符的随机 pipe 后缀，以及 32 字节 CSPRNG 会话 secret。使用子进程专有环境变量传入 secret，不放进命令行或日志：

```text
TAILSYNC_WPF_SESSION_TOKEN=<64 hex>
TailSync.Backend.exe --wpf-backend --pipe=TailSync.Wpf.<32 hex> --parent-pid=<actual parent PID>
```

后端启动前验证所传 PID 确实是实际父进程，随后持有进程句柄监控退出。Windows named pipe 为 `\\.\pipe\TailSync.Wpf.<32 hex>`，受保护的 DACL 只授权当前用户 SID，拒绝远程客户端，并通过 first-instance 标志拒绝已有名称。子进程读取会话 secret 后立即从环境移除，避免其后启动的进程继承。

宿主应等待 authenticated `handshake` 成功后显示运行态。成功结果包括 `protocolVersion:1`、`backendVersion`、`frontend:"wpf"`、`transport:"windows-named-pipe"`、`isolated` 和真实后端 capabilities。启动错误仍写入数据目录中的 `startup-error.log`；普通运行日志为 `tailsync.log`。

当前原生界面还要求 `nativeUiContractVersion:2`、`settingsContract:"field-patch-v1"`，并读取 `connectionModes` 和 `networkEnabled`。模式按钮使用后台返回的已提交设置快照；旧后台即使产品版本号相同，也会在启动时明确拒绝混用。WPF 与 Backend 必须来自同一个发布目录。

`--wpf-isolated` 仍禁用真实网络与连接方式修改，仅供存储/布局验收。需要验证实际 Iroh 链路时，测试脚本显式组合 `--wpf-isolated --wpf-network-check`：启动独立 Iroh 监听器和身份，允许模式切换/邀请/双端确认，仍禁止剪贴板恢复、系统快捷键与注册表集成。该模式不用于日常运行。

```powershell
python windows/native-ui/check-wpf-network.py <backend.exe> <report-directory>
```

此脚本启动两个真实 Rust 后台，通过命名管道切换全部四种模式、创建与解析邀请、比对两端六位码、验证单侧确认不会完成配对、双端确认后持久化身份。网络故障会失败并保留有限状态诊断，不伪造成功。

## 帧与调用

引导沿用安装范围的 Rust 标记：`onboarding_should_show`、`onboarding_suppressed` 读取状态，`dismiss_onboarding` 普通结束，`suppress_onboarding` 永久关闭。`open_onboarding_window` 产生原生 `tour` 导航事件，由 WPF 打开已有窗口上的引导。样本数据保留在 C# 内存；虚拟 ID 区间 `[8000000000, 9000000000)` 及 `tailsync-demo-batch` 在管道派发前拒绝，包括引导结束后残留的预览和缩略图请求。状态不进入共享设置或同步协议。

每个请求使用一个新连接，支持最多 16 个并发请求；长轮询不会阻塞其他 UI 操作。

```text
request:  u32LE(json byte length) + UTF-8 JSON
response: u32LE(json byte length) + UTF-8 JSON + optional raw binary bytes
```

```json
{"id":1,"token":"<64 hex>","command":"get_history_page","args":{"limit":30,"offset":0,"collection":"all"}}
```

```json
{"id":1,"ok":true,"result":{"entries":[],"total":0,"has_more":false},"binaryLength":0}
```

失败时 `ok:false`、`error` 为已有的稳定错误结构，保留 code/message_key/retryable 等字段；不传出私有后端路径或任意错误字符串。JSON 请求和响应上限均为 1 MiB，原始二进制响应上限为 70 MiB。请求必须在 15 秒内发送，命令有 120 秒超时。调用超时可能代表操作结果未知，宿主不能自动重放写操作。

参数名与原 Tauri 前端相同，采用 camelCase。桥接有显式命令白名单，不允许任意函数名、脚本执行、任意文件读写。主题包、存储迁移和旧存储清理仍由原核心检查包摘要、所有权和迁移授权。

`get_preview` 返回现有 `TSPV` 原始二进制包；`get_document_preview` 返回现有文档二进制格式；主题图片槽返回原图字节。这些响应的 `result:null`，JSON 后紧跟恰好 `binaryLength` 字节，不再额外添加长度前缀。宿主可直接接收 byte[]，无需 Base64。

## 后台事件与生命周期

`wait_runtime_snapshot` 保持原历史/传输进度/通知协议。额外的 `wait_wpf_events` 接收 `{sinceRevision,waitMs}`，返回：

```json
{"revision":5,"events":[{"revision":5,"action":"open","window":"settings"}]}
```

`action` 可为 `open`、`close`、`toggle`、`tour`、`refresh` 或 `quit`。窗口名为 history/favorites/settings/connections；refresh 的目标为 settings 或 theme。托盘、全局快捷键、深链接和系统内的设置/主题变化均转给 WPF。队列保留最近 128 条事件，宿主应保存 revision 并持续长轮询。托盘退出先发 quit，由 WPF 发送退出确认；5 秒后后端会兜底关闭。

宿主退出前调用 `shutdown_wpf_backend`，后端响应后发起原有有界清理。父进程退出或 45 秒没有任何已认证请求也会停止后端，防止孤儿进程。宿主需要处理后端自行退出并结束/恢复自身状态。

WPF 宿主管理当前用户 tailsync URL association，并将冷启动/二次启动链接通过 `queue_remote_pairing_link {link}` 交给真实 Rust parser；随后监听连接窗口导航并通过 `take_pending_remote_pairing_link` 消费已验证链接。WPF 模式不再把 URL association 注册到 Rust sidecar。

## 隔离验证

使用新建的绝对临时目录 `TAILSYNC_DATA_DIR`，并加 `--wpf-isolated`。该模式加载真实加密数据库、设置和主题存储，但不注册深链接、全局快捷键、托盘或系统通知，不启动剪贴板监控、网络同步或自动更新。真正的剪贴板恢复、快捷键变更、主动配对/网络测试、存储迁移及更新安装请求会返回明确错误；设置补丁不能变更 connection_mode。所有测试数据保留在指定目录。

`seed_wpf_test_history {count}` 只在此模式下可用，且数据库必须为空，count 为 1–100。它通过真实生产加密/索引写入固定合成记录，包含中英文文本、网址、代码、命令、JSON、小图片与文件，返回 `{count,ids}`。生产模式始终拒绝该命令。隔离模式 get_peers 即时返回真实本机身份、已配对设置和明确的“网络发现已禁用”原因，不伪造在线设备，也不等待不存在的健康监控 worker。

```powershell
python windows/native-ui/check-wpf-backend.py <absolute-backend.exe> <absolute-result-directory>
```

脚本测试真实握手、错误 secret 拒绝、白名单、设置持久化与受保护字段、65 条加密记录的分页/收藏保护/清空/删除、原始二进制预览和缩略图、主题解析/选择、原生窗口事件和正常退出，结果写入 `backend-checks.json`。

## 发行边界

开机启动应由 WPF 宿主管理并指向自身，不能调用原 Tauri autostart 插件把仅后端 exe 注册为登录启动程序。

现有更新源安装的是 Tauri 产品。WPF 模式拒绝 `install_update`，直到发行流程能够将 WPF 宿主和 Rust 后端作为一套原子升级包发布。`get_update_status` / `check_for_update` 可供显示已有发布信息，但不能把这个源当成兼容的 WPF 安装包。资源测量必须统计 WPF 与后端整个进程树，原 WPF 纯示例内存数字不代表接入生产同步服务后的总量。
