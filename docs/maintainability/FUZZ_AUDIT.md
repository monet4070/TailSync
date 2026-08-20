# TailSync Maintainability Refactor — FUZZ CANDIDATE AUDIT (T440)

> 生成时间：2026-08-15（T440，R020 输入，T420 后实测）
> 依据：T000 BASELINE §10（fuzz 现状 0）+ 本文件候选审计为 **T440 实测**。
> 只读交付。R020 现状：**0 fuzz target**、无 proptest/arbitrary 依赖（Cargo.toml 实测）、无 fuzz/ 目录。

---

## 1. 候选表（按风险收益排序，不凑数）

| 候选 | 位置 | 现有测试 | 风险面 | 建议 |
|---|---|---|---|---|
| `Frame::decode`（wire 帧解码） | protocol.rs:487（Frame + 长度前缀） | protocol.rs 共 10 测试 | 跨网络 untrusted 输入；帧头/载荷长度组合爆炸 | **最高优先**（cargo-fuzz target 或 proptest） |
| `Command::decode` / `FrameEnvelope::decode` | protocol.rs:98/143/190 | 同上 | 命令枚举 + 载荷结构 | 与 Frame 同 target 覆盖 |
| `parse_pairing_target` | peer/directory.rs:101 | directory 19 测试 | 地址字符串解析（TCP/IPv6/iroh endpoint） | 次优先（纯字符串解析，proptest 友好） |
| `validate_incoming_file_meta` | sync/prepare.rs（T352 后类型化） | 4+ 测试 | 元数据校验路径（名称/hash/大小） | 低（边界已枚举，T107 测试充分） |
| JSON-lines 导入载荷 | import.rs | 10 测试 | base64 块/哈希解析 | 低（有长度上限 + 校验） |

## 2. R020 判定

- 候选按风险收益排序如上；**不新增 fuzz 基建**（无 fuzz/ 目录、无 CI fuzz job——R015 不动门禁）。
- 若执行 T441：仅 Frame::decode proptest（dev-dependency proptest，`cargo test` 内运行，无新 CI 门禁）；其余候选保持现状并登记。
- K005 约束：iroh 相关 ignored 测试不触碰。

## 3. 实现状态（T441，2026-08-15）

- **proptest = "1" 已入 core dev-dependencies**（Cargo.lock 已同步，`--locked` 验证通过）；无新 CI 门禁（R015 不动）。
- 新增 6 个属性测试（protocol.rs proptests 模块）：
  1. `frame_round_trips_through_encode_and_decode` —— 24 个 Command 变体 × 任意 flags/sequence/payload(≤256B)：encode→decode 后 command/flags/sequence/payload 全等 + consumed == encoded.len()。
  2. `frame_decode_never_panics_on_arbitrary_bytes`（≤2048B 任意字节）。
  3. `frame_decode_consumes_at_most_the_input_length`（Ok 时 consumed ≤ data.len()）。
  4. `file_chunk_payload_decode_never_panics`、5. `file_offset_decode_never_panics`、6. `event_envelope_decode_never_panics`。
- 实测：core **279 测试全绿**（273 + 6，3.93s）；clippy 0 warning；fmt 干净；macOS 61、Windows host check、漂移检查 PASS。
- 附带修正：api/imports.rs（W/M byte-identical 钉扎面）两端 rustfmt 归一化差异被发现并同步（漂移检查重新 PASS）—— 钉扎面任何 fmt 需两端同跑。
