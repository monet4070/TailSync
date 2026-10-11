# R2-C：网络图片补全同步执行方案

日期：2026-10-08。状态：R2-C 本机实现、父代理二修及最终本地门禁完成；真实设备验收仍 UNKNOWN。详见 [验收文档](RICH-TEXT-R2C-ACCEPTANCE-2026-10-08.md)。

实际工作树：`/Users/monet/.codex/worktrees/composite-clipboard/TailSync-rc-phase35`，HEAD `f719d581a1aa5198a1b3f2483222df5c69e3dfc0`。R2-B 已封存，当前基线含 1193 个非 artifacts 文件、308 项既有 dirty。禁止混用环境默认 checkout。

## 目标与不变项

原始本机复制已经通过现有便携复合事件发送给可信接收端时，之后自动或手动保存的公开网络图片应补充到接收端的同一条历史。接收端不再联网，不更改系统剪贴板，不新建一条“补全”历史，不抢焦点；已保存资源支持现有离线预览/恢复路径。

- 关联必须是认证发送者加原始事件 ID，再验证原始便携载荷 digest；绝不按显示名、内容 hash 或本地 capture ID 查找类似历史。
- 捕获原件、原正文、NativeArchive、capture/content/dedupe 身份及历史 data_hash 不变，只增加派生资源与原图保存。
- 删除、清空或不存在的接收端历史不被补全复活。新的复制与恢复不会被后台补全改写。
- 重试同包幂等；同远端 revision 的不同内容是碰撞；旧 revision 不覆盖新内容。本地已有下载与来源上下文不被抹掉。
- 复用 R2-B 的原图、像素、派生、加密与配额预算，以及现有传输 admission/时间窗口；不扩大限额。
- 不做完整历史回填、任意历史合并或多写者协同。未收到原始事件的设备不能仅凭补全创建记录。远端接收条目不作为本机原创重新转发或自动下载。

## 1. 明确的补全载荷

新增 Core `composite::archive_sync::ArchiveUpdate`：原始事件 ID、基线 digest、发送端 archive revision，以及按源 HTML 图片序号排列的原始图片 body 和 PNG/JPEG/WebP 格式标签。

`ARU1` 全大端：64 字节 header（magic、事件 ID、digest、revision、count、reserved），每条 8 字节记录头（图片序号、format、reserved、length）加原 body。数量 1–8、单图 8 MiB、累计 16 MiB；总载荷不超过 `64 + 8*8 + 16 MiB`，不发送另一份派生 PNG，避免重复扩大网络资源。

结构校验拒绝未知 tag、非零 reserved、空 body、重复位置、截断、尾随数据及超限。revision 取 2–9007199254740991。派生时证明基线 digest、原 HTML 真实公开 HTTP(S) 位置、静态解码与累计预算，再复用资源绑定规则得到 PNG 和本地实际 resource ID。不通过网络再次验证或下载；拒绝动画、SVG、blob、私网、带凭据或无可靠地址上下文的位置。

## 2. 事务与持久关联

复用 `history_composite_events` 元数据表，不增加历史 schema 或 manifest 字段：

- 本机原始广播在入队前绑定已保存历史、确切事件 ID 和基线 digest，使用内部保留 source 命名空间。绑定失败不能靠 hash 替代。
- 接收端原始事件已有 `(authenticated source, event_id)` 映射。补全先查该映射；不存在返回可重试的 BaseMissing，删除 tombstone 返回终态 Superseded。
- 一个 `archive:<base-event-id>` 元数据记录保存最新远端 revision 与完整补全载荷 digest，不为每个 revision 增加无界记录。
- 远端 tracker、派生 archive layer、原图 sidecar、manifest 与 archive identity 在同一 SQLite CAS 事务提交。配额、取消、旧身份、SQL 或文件错误不能只提交 tracker 或部分载荷。
- 从原始 base 派生入站图片，再按序号合并到当前派生内容；已绑定位置/原 body 冲突显式拒绝。重复内容不增加本地 archive revision，但可事务推进远端 tracker。
- 活历史的原事件关联不能因超过当前十分钟 replay 窗口被清理。仅过期且 history_id 为 NULL 的 tombstone 可清理，保留既有 4096 元数据 cap；容量不足明确拒绝，不扩预算或覆盖旧关联。

## 3. 独立能力协商与可靠传输

增加独立 `archive_resource_updates` authenticated capability，缺字段视为不支持，并依赖 composite 服务健康及 wire-v5。

规划独立命令 `ArchiveUpdatePayload` / `ArchiveUpdateChunk` / `ArchiveUpdateAck`，代码点 `0x0111` / `0x0112` / `0x0113`。补全绝不降级成 Text/Image/Composite 新剪贴板事件。旧客户端继续接收其支持的原始复制；不支持补全时明确终止该补全，不冒充成功。

chunk 复用现有有界压缩、身份、digest、ACK、expiry、每认证身份和全局 admission，使用独立 chunk kind/magic，防止与普通剪贴板 assembly 混淆。接收完成只进入归档 DB 路径，不进入 native publish 或普通 remote receive。

ACK 绑定补全消息 ID、wire digest、chunk 序号及命令。Saved/幂等重复与 Superseded 为对应的终态；BaseMissing 可在原有可靠窗口内重试。不得把后台 DB 保存描述为已写入系统剪贴板。授权撤销、sync pause、错误 peer 或失去 capability 必须 fail closed。

## 4. 下载后生产接线

自动队列与两端手动保存成功后调用同一补全发送接缝；仍先通知本机历史/预览刷新，传输失败不回滚已经成功的本机保存。

补全发送只读当前保存 stamp 和原事件映射，有界排队/合并，避免一次复制产生无界后台任务或保留正文副本。入队前检查记录身份、DB authority、是否仍存在及 sync/peer 权限；不把远端接收事件重新归类为本机复制。

原始事件绑定与初次发送须先于自动下载补全发送；补全早到则只重试现有记录关联，不能创建历史。当前可靠传输时间窗不是无限离线历史同步的承诺，超过窗口或缺失原事件必须如实报告，不能偷偷生成新的原始复制。

## 5. 分工与顺序

1. Codec 代理：`composite/archive_sync.rs` 和单行模块注册，完成有界 codec/基线派生及专项回归。
2. DB 代理：`db/composite.rs` 和 `db/archive_sync_tests.rs`，完成持久关联、合并、幂等及同事务 tracker。
3. 主代理：独立 capability/commands、chunk/delivery、认证接收、下载/广播生产接线与端到端本地回归，不与子代理写集重叠。
4. 两代理先 ACK 后 START；Cargo 由主代理串行发 GO_TEST。完整交付后主代理独立审查、真实复现必要缺陷、二修并重新冻结源码验证。

兩名均内置 Multi-Agent、`cline-pass/deepseek-v4.1-flash`/`high`、完整明文、`fork_context: false`。无第三名代理、独立 chat 或后台 CLI agent。

## 6. 必须提供的证据

- Codec：三格式原字节、图片顺序、PNG 去重；每个截断边界、tag/reserved/重复/预算；base mismatch、已有绑定、危险来源、动画及像素/派生预算。
- DB：错误来源/event/digest、BaseMissing、删除/迟到、重复/碰撞/倒序、本地部分补全及来源 proof 保留、重启、SQL/配额/取消原子回滚、live 映射保存和容量边界。
- 传输：缺能力不发送、不降级；认证撤销/错误 ACK/跨 kind/chunk 乱序与预算/expiry；同包重试不重复压缩或重复提交。
- 生产接缝：原始事件→本机下载→真实可靠帧→可信接收→同条历史；三格式、部分失败再试、两端 API/自动队列接入、通知刷新；断网模拟下已保存的接收端预览与恢复，不调用接收端网络或 native publisher。
- 主代理最终门禁：Core/Runtime、Clippy、两端 adapter/test、Windows UI/build/lint、Swift、生成契约、格式与 diff；所有最终结果绑定同一产品指纹。失败与重试日志不覆盖。
- 基线和封存保护：全部既有工作、HEAD、暂存区、旧 App/候选包、R2-A/R2-B 证据均保护；不读用户 DB/系统剪贴板、不停止日常 App，不创建重复 Rust/Swift 缓存。

本地模拟收发不能证明真实两设备、Windows 原生、目标编辑应用粘贴或发布准备。缺失实机证据仍 UNKNOWN，Release NO-GO，整体方案保持 active。
