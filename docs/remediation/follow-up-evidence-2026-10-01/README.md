# 后续复核证据（2026-10-01）

对应 [后续报告](../FOLLOW-UP-2026-10-01.md)，HEAD 914638d，本地未提交源码。这里的记录认证本次执行范围；不认证 GitHub CI、Windows 原生 DACL、最终 app/installer 或双设备行为。

- source-snapshot.json：tracked 与 untracked 源码 SHA256；排除文档、派生 Tauri schemas 和未编译的根目录草稿 test_r1_scenario.rs。
- gate-results/rust-macos：Core/runtime/macOS/Swift 实际执行的具名结果与 tracked 指纹。ledger-execution.log：31 项台账、28 个适用具名门禁核对。
- core.log、runtime.log、mac.log、swift.log：完整当前套件输出。Swift 本次显式开启 Web SVG 测试，211 全部执行通过。
- frontend.log、frontend-build.log、frontend-lint.log、node.log、contracts.log、parity.log：前端/Node/契约/跨平台结果；旧 lint warnings 保留。
- core-clippy.log、mac-clippy.log、windows-host.log：严格 Clippy 与 Windows host 编译。host 编译包含 dev-only 测试支持，不执行 Windows cfg 分支。
- r2-before.log、quota-before.log：修复前真实生产 API 失败。r1-current.log：补强回滚场景；R1 实现进入本轮时已先行修复，通过回退旧策略的变异补证。
- mutations.json、*-mutation-red.log、*-mutation-green.log、mutation-source-snapshots：隔离副本执行，保留当前生产源码与变异的 exact SHA256/字节；red 必须是实际用例失败，不能以编译错误代替。runner 需显式传入新建副本和证据目录，禁止指向主工作树。
- test-environment.json：隔离数据根；不修改 HOME 或真实配置。mutation-workspace.txt 是取证时的临时路径，不是永久工作树。
- worker-baseline.log / worker-baseline.json / performance-run.json / performance-environment.json：修正驱动的性能记录与源码绑定；单个事件 lane 占用指标不能认证签署共享队列水位。

上一轮 checkpoint 编译错误和 pairing 42 例中间日志已准确改名，仍保留在 implementation-evidence-2026-10-01。上一轮 final-* 与 worker-baseline 是历史证据，不能认证本次源码。

最终九轮通过：10800/10800 文本确认，真实拒绝/过期/失败均0，每轮256MiB，p99 46.782–52.256ms。运行前后源码均匹配 source-snapshot，共享水位不认证。详见 verification-summary.json。
