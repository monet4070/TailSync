# 本地测试污染设备配置：修复与回归证据

用户在新 App 的设备页发现已配对的 `client / 127.0.0.1`。本机真实配置中的公钥指纹与设备页一致。
共享接收测试预置了 `client` 的信任公钥，并调用真实 `handle_accepted_connection`；认证成功后
`remember_peer_address` 会保存整份 Settings。此前未设置隔离变量的测试会使用生产应用目录，
因此有能力把测试信任和地址写入日常配置。现有日志不能追溯具体是哪次历史测试写入。

本次先备份用户配置，再通过运行中 App 的“撤销配对”删除这个条目。核对确认四个 peer map
中的 `client` 已移除，其余配置值保持一致。备份和真实用户配置均未纳入仓库证据。

## 修复边界

- Core 单元测试以及带 dev-only `test-support` 的 `tailsync_lib` / `tailsync_runtime` 测试进程，
  在没有显式 `TAILSYNC_DATA_DIR` 时自动使用进程独立的私有临时目录。测试依赖识别沿用
  `deps/<crate>-<hex hash>` 的限制；普通应用可执行文件保持生产目录和系统密钥行为。
- 自动选择测试目录时也停止隐式发现 HOME 下的 v1 历史，防止新目录仍导入个人旧数据。
  显式 v1 路径继续支持受控的迁移测试。
- `scripts/run-isolated-tests.mjs` 为一次验证覆盖 data、storage、legacy 三个环境变量，
  结束后删除自己创建的临时目录。保持 HOME、USERPROFILE 和 Keychain 不变。
  CI 整改门禁和 macOS source-check 使用同一隔离器。
- 直接调用 Cargo 的显式路径仍由调用者控制；不能把测试路径手动指向日常应用数据。
  正常库构建、应用运行及测试之外的程序应继续遵循显式环境变量。

## 承重门禁

`inbound_test_peer_does_not_modify_user_configuration` 在没有 data/storage/legacy 覆盖的子进程中，
跑实际 Noise 认证和接收入口，确认测试 `client` 被保存到临时配置、用户配置字节不变。
修复前它在写入前的保护断言上失败：选出的测试目录与用户应用目录相同，见 `inbound-red.log`。

`implicit_test_store_keeps_settings_and_legacy_history_out_of_user_data` 覆盖 Core 单元测试的自动目录、
真实 Settings 保存和隐式 v1 导入。两个 subprocess worker 虽在顶层标成 ignored，均由对应的
常规父测试明确启动并执行，并非跳过回归验证。

Node 子进程测试给继承的三个数据路径放入受控的“用户数据”哨兵，执行真实启动器，验证成功和
失败退出时哨兵未变、临时目录被删除；同时验证并发调用互不影响。

变异验证：恢复隐式 HOME 历史导入，Core 门禁因导入旧数据而失败；恢复继承用户数据环境变量，
三个 Node 门禁全部失败。原始日志与 `mutations.json` 见 [证据目录](test-isolation-evidence-2026-10-01/)。

## 本地验证

- Core：491 个单元测试通过、4 个顶层 ignored；另 2 个 integration 测试通过、1 个 worker ignored。
- runtime：23 通过。macOS Rust：115 通过、3 个顶层 ignored；其他 targets 无测试。
- Node：119 通过。Core / macOS `clippy --all-targets -- -D warnings` 通过。
- 跨平台合约、生产依赖特性、共享依赖解析、整改台账验证通过。
- 上述本地完整测试结束后，真实用户配置 SHA-256 与清理后的基线一致，没有重新产生 `client`。

本地 Rust 验证关闭调试符号和增量缓存以节约磁盘，不改变业务测试断言。源码散列、命令和原始
输出见证据目录；这些记录对应本地验证时的修复源码，远端原生 CI 结果以对应提交的 Actions 为准。
