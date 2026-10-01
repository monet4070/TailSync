# 独立复核证据

对应 [独立复核报告](../INDEPENDENT-REVIEW-2026-09-30.md)，生产源码基线 `914638d125e651c9d08052d20a8cc9ad77deda9a`。

这些材料是**反例复现与审查证据**，不是已完成修复，也没有加入产品 CI。2026-10-01归档整理：独立探针Cargo清单字节原样保留为Cargo.toml.txt，避免带test-support的历史研究包被生产manifest门禁误作产品依赖扫描；复现时复制到仓库外临时工程。

| 文件 | 用途 |
|---|---|
| `verification-summary.json` | 本次四组原有测试的源码身份、退出码/解析计数；下载的第二次 CI 运行中每个平台的命令、源码身份、31 条台账适用 gate 的匹配结果 |
| `rust-probe.log` | 原始临时目录中的 Rust 程序实际输出 |
| `probe/Cargo.toml.txt`、`probe/src/main.rs` | 调用当前生产 Core API，以 Noise+duplex 会话复现配对，使用默认 DB 配置复现删除问题 |
| `frontend-cooldown-repro.patch` | 在原有 History 测试文件中增加一条反例；仅修改测试翻译夹具以区分各个 peer 和添加反例测试 |
| `frontend-probe.log` | 前端反例实际运行结果 |

## Rust 复现

需要仓库现有 Rust 依赖。程序必须传入**新建的临时目录**，它会将 `TAILSYNC_DATA_DIR` 和 `TAILSYNC_V1_DATA_DIR` 指向该目录，实际写入测试配置/sidecar/SQLite；不会使用用户真实数据目录。原复现使用已缓存依赖、`--offline`，原独立包锁文件位于 `/tmp/tailsync-remediation-review.9AflZB/probe/Cargo.lock`；此处未复制 4605 行锁文件，后续依赖解析的精确复现需保留原锁文件或仓库对应缓存。

从仓库根目录执行：

```bash
review_probe_root="$(mktemp -d /private/tmp/tailsync-review-probe.XXXXXX)"
mkdir -p "$review_probe_root/src" "$review_probe_root/data"
cp docs/remediation/review-evidence-2026-09-30/probe/src/main.rs "$review_probe_root/src/main.rs"
python3 - "$PWD" "$review_probe_root" <<'MANIFEST'
from pathlib import Path
import json, sys
repo, probe = map(Path, sys.argv[1:])
text = (repo / 'docs/remediation/review-evidence-2026-09-30/probe/Cargo.toml.txt').read_text()
text = text.replace('"../../../../shared/rust-core"', json.dumps(str(repo / 'shared/rust-core')))
(probe / 'Cargo.toml').write_text(text)
MANIFEST
CARGO_TARGET_DIR="$PWD/target" cargo run --offline \
  --manifest-path "$review_probe_root/Cargo.toml" -- "$review_probe_root/data"
```

它会为独立包生成自己的 Cargo.lock。程序使用生产 `PairingManager::new`（持久化开启），延迟的是传输 shutdown，未篡改生产配对逻辑；遗忘场景调用与三个适配器一致的 settings/forget_pending 顺序，不直接执行 Tauri/Unix route。公钥冲突场景在生产配对路由也可安装会话，普通数据准入仍会拒绝错误 pin。

收藏载荷反例需要大小写不敏感文件系统；在大小写敏感卷上两个名字可代表不同文件，不能据此说该问题已修复。LAN 输出只证明选择器的结果，不代表已执行 OS 网卡/VPN 端到端测试。

预期在审查基线上观察到：

```text
finalize_cancel phase=Cancelled trust=true disk_trust=true
after_forget pending=0 trust=false
late_ack_after_forget phase=Paired trust=true
known_key_conflict persisted_frame_sent=true phase=Waiting old_pin_retained=true ...
failed_note_write is_error=true memory_records=1
delete_with_live_reader ok=true elapsed_ms=5205
delete_case_alias same_file=true favorite_still_readable=false favorite_path_exists=false
```

5205 ms 是本次观测值，不是精确承诺。程序当前断言有问题的执行路径能够走到，再输出后果；整改后应把它们转换为生产回归测试，而不是为了让此反例仍运行通过保留错误行为。

## 前端反例

在仓库外建立 windows 目录副本，复制 `src`、package/config 文件，复用原有 `node_modules`；保持生产组件和 hooks 不变。进入副本后应用补丁：

```bash
patch -p1 < /Users/monet/TailSync/TailSync-remediation/docs/remediation/review-evidence-2026-09-30/frontend-cooldown-repro.patch
./node_modules/.bin/vitest run src/pages/History.test.tsx -t 'review probe'
```

反例先确认 101、102 的 peer 文本实际出现在 DOM，8 秒预算用尽后确认 103 被 ack 且 102/103 都不在 DOM。测试的 passed 表示当前坏行为被证实，不能作为修复通过。

## CI 与完整本地验证

核对的远端运行：[36709267509](https://github.com/monet4070/TailSync/actions/runs/36709267509)，SHA `046b2a0`。`verification-summary.json` 中每条 gate 包含在真实结果中匹配到的通过名称；Windows 成品 gate 使用 `integration:S6-P0-1`，其来源是包装记录的 integrations 字段，而非伪造单元测试名。

原下载材料：`/tmp/tailsync-remediation-review.9AflZB/ci-046b2a0/`。可在产物保留期内重新下载：

```bash
gh run download 36709267509 --repo monet4070/TailSync \
  --name remediation-tests-macos --dir /path/to/new/macos-evidence
gh run download 36709267509 --repo monet4070/TailSync \
  --name remediation-tests-windows --dir /path/to/new/windows-evidence
```

本次完整本地 Rust/Swift 原始记录：`/tmp/tailsync-remediation-review.9AflZB/results/rust-macos/`。原始 Node/parity 日志：`/tmp/tailsync-review-node.log`、`/tmp/tailsync-review-parity.log`；原有前端测试日志：`/tmp/tailsync-remediation-review.9AflZB/frontend.log`。临时目录不保证长期存在，关键复现源码/输出及验证摘要已经保存到本目录。

本地 Core 单元测试日志为 472 passed，两个集成套件各 1 passed；执行记录解析器按测试名称合并后为 473 个 passed 名称，不能把该去重计数替代各二进制原始测试计数。Swift 是 209 executed、3 skipped，即 206 passed。
