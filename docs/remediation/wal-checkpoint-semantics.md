# WAL checkpoint 结果行的影响核对与忙时语义

> 由 `S5-P2-2` 的修复 PR 附带产出(方案阶段 5 要求)。
> 位置以函数名标识,避免行号随重构漂移。
> 背景:`PRAGMA wal_checkpoint(TRUNCATE)` 在**被读者阻挡**时**返回一行** `busy = 1`,而不是抛错。任何用 `execute_batch` 执行它的地方都会**静默丢弃**该行,于是"没截断"这件事无人知晓。

## 结论表

| 位置 | 场景 | busy 时的语义 | 依据 / 处置 |
|---|---|---|---|
| 删除收尾 `checkpoint_history_wal`(已修) | 删除/清理后的 WAL 截断 | **继续**,但记录警告 | 删除已提交,截断只是回收;`WalCheckpointOutcome::Blocked` 会 `warn!` |
| `clear_all_with`(**仅 `#[cfg(test)]` 辅助**,非生产路径) | 用户主动 clear 后的 `TRUNCATE; VACUUM;` | **继续**;VACUUM 是空间回收的优化 | `clear_all` 语义是"清空逻辑数据",截断/VACUUM 失败不改变结果。真实 SQL 错误已被 `first_error` 收集;busy 不报错,故仅影响空间回收 |
| `migrations` v4 页回收步骤 | v4 图片迁移后的页回收 | **继续** | 紧接着有 `freelist_count` 判断与按需 `VACUUM`;busy 只意味着这次截断没做,后续启动仍可回收 |
| `migrations` v9 明文清除步骤 | v9 明文预览清除后的 `TRUNCATE; VACUUM;` | **重试(不得静默继续)**✅ 已修 | 见下 |
| `migrate_storage_with_rollback` 复制前步骤 | 存储迁移复制 db/WAL 之前 | **继续**;另有独立风险待跟踪 | 见下 |

## v9 明文清除步骤:真实风险,已修

该步骤在 v9"清除明文预览残留"之后执行 `TRUNCATE; VACUUM;`,随后才把 v9 标记为完成。若此处 busy:

- WAL **不会被截断**,第 9 版之前遗留在 WAL 页镜像里的明文**可能留存**;
- 而代码仍会把 v9 标记为完成 —— 但现有设计本来提供了补偿机制:注释写明"Mark v9 complete only after the residual-data cleanup succeeds. If the process exits first, startup repeats the idempotent preparation and vacuum phases."(migration_state 里已有 `vacuum_pending` 阶段)。

**因此这里的正确语义是"重试":** 忙时不得静默继续,应让该步失败,使 v9 不进入完成态,由启动时的幂等流程重做。旧 `execute_batch` 丢弃结果行,绕过了这个重试机制；当前实现已读取并判断结果行。

**处置(已修):** 抽出 `truncate_wal_for_v9_cleanup()`,读取结果行;`busy != 0` 时**返回错误**,使 v9 不进入完成态,由下次启动的幂等流程重做。门禁 `v9_cleanup_wal_truncation_fails_when_a_reader_blocks_it`(读者阻挡 → 报错;释放后 → 成功),已做"忽略 busy"的变异验证。

2026-09-30 补充：只给重试间隔设 50ms 并不能约束 SQLite 自身的忙等待。新实现临时设置每次 busy_timeout 为 50ms，5 次尝试和 4 次 50ms 间隔约束锁等待约 450ms，成功/失败后均恢复连接原超时；这不是整个迁移的硬实时上限。完整迁移门禁 `v9_busy_migration_keeps_pending_state_and_completes_on_reopen` 验证失败时保留 v9 未完成与 vacuum_pending，释放读者后重新打开能完成迁移，同时断言被阻挡时耗时小于 2s。旧代码同一用例实测约 26.2s。

## `storage.rs:234`:结果行本身安全,另有既存风险

该处截断后调用 `copy_bulk_storage_verified`,复制 `history-v2.db`、`-wal`、`-shm`(`bulk_storage_names()`),随后 `verify_sqlite` 重新打开副本(会重放 WAL)。所以**单凭 busy 不会丢失已提交的帧**,丢弃结果行在此不构成数据丢失向量。

但审查同时指出一个**既存**(非本项引入)的风险:在并发写入下对 db/wal/shm 的复制并非原子(`CODE-REVIEW-2026-08-30` 已记录)。该问题与本条目无关,登记为独立跟踪项,不在本 PR 扩大处理。

## 单测成本

`S5-P2-2` 的门禁用双连接制造忙状态。默认 `busy_timeout` 为 5 s,两次被阻挡的截断(直接调用 + `delete`)使该测试耗时约 **10.5 s**,占该 crate 单测总时长的大头。测试内已把 `busy_timeout` 降到 50 ms——门禁只需要观察到"忙"这一结果,不需要等满生产超时。
