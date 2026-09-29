> **历史文档（审计输入，非当前状态）**
>
> 本文件是 2026-09-27 的静态审计记录。其正文称 28 项（4 P0/16 P1/8 P2），但风险矩阵实际列出 31 个 ID（4 P0/19 P1/8 P2）。
> 其三 PR 路线图与 A–I 计划多处已由 PR #67 执行；第 5 节验收样例含已失效断言（例如 6 GB 单批超出 1 GiB 上限，且未计入加密副本空间）。
> **当前状态一律以 `docs/remediation-ledger/` 为准。**

# TailSync 全系统静态代码审计与缺陷修复方案白皮书 (2026-09-27)

> **审计对象**：TailSync 核心代码库（跨平台局域网端到端加密同步系统）  
> **审计范围**：全量 6 大高危切片（网络自省、并发连接池、Noise协议与配额、大文件分块流控、SQLite存储、跨平台IPC）  
> **审查方法**：逐行代码走查、三方库源码溯源（`mdns-sd 0.20.3`, `if-addrs 0.15.0`）、本地实机对撞（macOS/Windows）、并发模型推演与单元测试对齐。  
> **审计结论**：确认真实缺陷 **28 项**（P0 级严重隐患 4 项、P1 级系统缺陷 16 项、P2 级体验/资源缺陷 8 项），同时基于代码与算术实证彻底推翻了 6 个虚假伪问题。

---

## 目录

- [一、缺陷总览与风险矩阵](#一缺陷总览与风险矩阵)
- [二、已核实并排除的虚假假象（避免过度重构）](#二已核实并排除的虚假假象避免过度重构)
- [三、全量缺陷代码分析与手术刀级修复方案](#三全量缺陷代码分析与手术刀级修复方案)
  - [切片 1：网络探测、本地自省与候选池排序](#切片-1网络探测本地自省与候选池排序)
  - [切片 2：多路径并发竞速、连接池与健康心跳状态机](#切片-2多路径并发竞速连接池与健康心跳状态机)
  - [切片 3：Noise 协议握手、配对协商与密钥持久化](#切片-3noise-协议握手配对协商与密钥持久化)
  - [切片 4：剪贴板大文件传输、分块流控与断点续传](#切片-4剪贴板大文件传输分块流控与断点续传)
  - [切片 5：SQLite 历史数据库、本地加密与配额淘汰](#切片-5sqlite-历史数据库本地加密与配额淘汰)
  - [切片 6：跨平台 IPC 契约、前端状态单向流与运行时协调器](#切片-6跨平台-ipc-契约前端状态单向流与运行时协调器)
- [四、生产级分阶段修复路线图 (PR Roadmap)](#四生产级分阶段修复路线图-pr-roadmap)
- [五、回归验收门槛与质量断言](#五回归验收门槛与质量断言)

---

## 一、缺陷总览与风险矩阵

| 缺陷编号 | 严重等级 | 缺陷名称 | 核心危害 | 影响子系统 |
| :--- | :--- | :--- | :--- | :--- |
| **S1-P0-1** | **P0 致命** | `local_ip()` 连接 8.8.8.8 误采 VPN 隧道 IP | 开启代理/VPN时误采虚拟IP，离线报 `0.0.0.0` 假路由 | 网络入口 |
| **S1-P0-2** | **P0 致命** | `source_matches_mode` 模式重叠 | `lan_only` 放行外网 Tailscale ULA，模式安全隔离失效 | 网络策略 |
| **S3-P0-1** | **P0 致命** | 匿名配对槽位抢占 + 失败配额可控消耗 | 局域网攻击者 5 次匿名非法握手即可让配对窗口进入 `Locked` 锁定 | 配对安全 |
| **S6-P0-1** | **P0 致命** | Windows TCP API 预鉴权空连接挂起假死 | 16 个空连接持续 5s 即可将 API 端口占满，阻断正版客户端通信 | 跨平台 IPC |
| **S1-P1-3** | **P1 严重** | 候选池纯 ASCII 字典序排序劫持主候选 | 脏地址/APIPA排在前面并被持久化，UI 显示与通信目标错乱 | 路由排序 |
| **S1-P1-4** | **P1 严重** | IPv6 缺少 Scope ID 必报路由不可达 | 构造套接字恒置 scope=0 导致直连失败；输入 `%en0` 误报 Iroh 错误 | IPv6 适配 |
| **S1-P1-5** | **P1 严重** | Windows 广播枚举缺少网卡状态位检查 | Windows 拔掉网线后误报“探测成功”，断网异常被静默掩盖 | 跨平台网络 |
| **S2-F1**   | **P1 严重** | 迟到重复 ACK 污染读流，误判协议错断连 | 网络抖动重传后，残留 ACK 被误判为协议错误，引发连接反复震荡 | 传输协议 |
| **S2-F2**   | **P1 严重** | 投递失败路径缺少退避（0 延迟重连风暴） | 网络闪断瞬间陷入死循环热重连，CPU 飙升并卡死设置保存 | 连接调度 |
| **S2-F3**   | **P1 严重** | 连接池 Key 包含动态 Target 强杀 Worker | 传大文件时后台网络自省使首选 IP 微变，旧 Worker 遭就地处决重传 | 连接池 |
| **S2-F4**   | **P1 严重** | 候选延迟无条件 sleep 导致 300ms 延迟税 | 首选 LAN 即使 1ms 报错，备用 Tailscale 仍死等 300ms 才能起跑 | 竞速流控 |
| **S2-F5**   | **P1 严重** | 文件控制帧挤占 Priority 即时队列 | 文件元数据等待长达 40s，打满 64 容量队列导致剪贴板文字丢弃 | 队列隔离 |
| **S3-P1-1** | **P1 严重** | 握手 Message 2 过早泄露主机元数据 | 局域网未认证扫描器发一条请求即可收割全网真实设备资产指纹 | 密码协议 |
| **S3-P1-2** | **P1 严重** | 配对单边持久化孤儿 (Asymmetric Orphan) | A 设备显示配对成功，B 设备无记录；两端永久静默不同步 | 配对状态机 |
| **S3-P1-3** | **P1 严重** | Hostname 静默覆盖信任锚与路由投毒 | 换机同名无警告覆盖公钥；未认证广播直接篡改可信设备地址 | 密钥存储 |
| **S3-P1-4** | **P1 严重** | `read_pairing_frame` 的 `select!` 字节错位 | 配对状态变化时打断非取消安全的 `read_exact`，导致底层流错位 | 异步安全 |
| **S4-P1-1** | **P1 严重** | 接收端并发限流漏洞（漏算 pending/inflight） | 限流只查 active，并发 `FileMeta` 冲破上限，每 Peer 达 20 路并发 | 接收引擎 |
| **S4-P1-2** | **P1 严重** | 续传配额预检重复计入已收前缀（双重记账） | 磁盘 `used` 已含已有 `.part`，预检又加全量，满盘时合法续传必被拒 | 配额状态机 |
| **S4-P1-3** | **P1 严重** | 配额预检在 DB 互斥锁内全盘递归扫描 | 每淘汰一条就递归扫盘，霸占 Tokio 线程与 DB 锁，卡死历史查询 | 存储 I/O |
| **S4-P1-4** | **P1 严重** | 每 1MB 分块多次抢全局引擎锁且锁内广播 | 分块处理在锁内触发平台更新与 watch 广播，千兆网速下阻塞网络栈 | 传输流控 |
| **S4-P1-5** | **P1 严重** | `restart_batch` 触发 32 次整批次全盘重哈希 | 接收端返回 `FileResume` 时发送端全量重算 BLAKE3，放大 32 倍读盘 | 续传性能 |
| **S5-P1-1** | **P1 严重** | 配额预检依赖文件系统大小判据抛假错误 | SQLite `DELETE` 不缩容数据库文件，系统误判未释放空间而拒绝传输 | 存储淘汰 |
| **S5-P1-2** | **P1 严重** | `get_image_data` 传入普通文件全量解密 5GB | 未校验类型直接 `get_data`，持 DB 锁将数 GB 明文装入内存致 OOM | 内存安全 |
| **S4-P2-1** | **P2 轻量** | incoming 目录创建使用系统默认 umask | 使用裸 `create_dir_all`，首次创建权限为 0755 而非 0700 | 文件安全 |
| **S4-P2-2** | **P2 轻量** | 孤儿 `.part` 仅在启动时清理，缺乏常驻定时器 | 进程常驻运行期间，异常断开的断点文件 24 小时内不释放 inode | 资源泄漏 |
| **S5-P2-1** | **P2 轻量** | 外部文件物理删除非原子且无目录对账 GC | DB 提交后删文件遇读写冲突静默留存，全库缺少对账孤儿扫掠 | 存储韧性 |
| **S5-P2-2** | **P2 轻量** | `wal_checkpoint` 遇锁冲突以错误外泄 | 提交成功后截断 WAL 遇忙报错，导致前端误判删除失败不刷 UI | 数据库锁 |
| **S6-P2-1** | **P2 轻量** | 通知环形缓冲区 32 条溢出无断层感知 | 缓冲固定 32 条，客户端落后时中间错误提示被静默丢弃无告警 | 运行时协调 |
| **S6-P2-3** | **P2 轻量** | SwiftUI 白名单放行 Windows 导致重启死循环 | 白名单允许 windows，但强制解码 macOS 快照（缺 status），看门狗杀进程 | 跨平台契约 |
| **S6-P2-4** | **P2 轻量** | 后台不可见长轮询破坏性“偷吃” `sync_warning` | 窗口最小化在托盘时，后台轮询 `take()` 消费警告，打开窗口已自毁 | 体验交互 |
| **S6-P2-6** | **P2 轻量** | macOS `wait_runtime_snapshot` 缺断开取消 | 客户端断开后长轮询依然死等 15s 占用连接信号量 | IPC 连接池 |

---

## 二、已核实并排除的虚假假象（避免过度重构）

在审计过程中，以下 6 项被实测与密码学/并发模型彻底否决，**严禁在生产中为这些虚假假象引入无意义的代码改动**：

1. **“mDNS 广播泄露了宿主机 WSL/Hyper-V 的 172.x 虚拟 IP” —— 否决**  
   `mdns-sd 0.20.3` 底层在出包前通过 `valid_ip_on_intf` 严格比对了发送物理网卡的子网掩码，非本网段的虚拟 IP 在物理 Wi-Fi 链路上根本出不去。
2. **“并发竞速 `race_connections` 会被不可路由候选拖慢 1~2 秒” —— 否决**  
   竞速使用 Tokio `JoinSet` 抢占式调度，首个成功的物理直连会在几毫秒内触发 `tasks.abort_all()` 立即杀死其余尝试。
3. **“执行引擎在 `spawn_blocking` 中调用 `block_on` 会引发死锁” —— 否决**  
   实跑单测与推演证实：Tokio 阻塞线程池按需扩容至 512 线程，等待的是纯内存锁，唤醒器跨线程有效，整个链路无环状依赖。
4. **“解密缓冲区 `read_buffer` 存在无界 OOM 膨胀漏洞” —— 否决**  
   单记录受 64KB 限制，单帧受 32MB 命令上限约束，解密缓冲区与网络下发流量严格 1:1，无任何内存放大乘数。
5. **“断点续传存在未校验脏文件落盘被外部读取的 TOCTOU 窗” —— 否决**  
   断点文件 rename 的目标是带 16 位随机十六进制的内部路径，数据库与 UI 在二次全量 Hash 校验通过前对该文件完全不可知。
6. **“本地加密容器存在 Nonce 重用漏洞” —— 否决**  
   头部 Nonce 为 `u32::MAX`，分块从 0 开始最多 5120（5GB/1MB），空间严格隔离；且每个文件使用独立随机 Salt 派生全新密钥。

---

## 三、全量缺陷代码分析与手术刀级修复方案

### 切片 1：网络探测、本地自省与候选池排序

#### 【S1-P0-1】`local_ip()` 连接 8.8.8.8 误采 VPN 隧道 IP
- **精确代码位置**：`macos/src-tauri/src/network/lan.rs:63-71`，`windows/src-tauri/src/network/lan.rs:147-155`，`macos/src-tauri/src/api/routes.rs:81`
- **机理分析**：`UdpSocket::connect("8.8.8.8:80")` 仅查询系统默认路由出口。开启代理或 VPN 时，出口被 `utun` 隧道（如 `198.18.0.1`）接管，导致本机 LAN IP 误显为代理 IP；在隔离 LAN（无默认网关）断网时返回 `"0.0.0.0"`，macOS 缺乏过滤将其包装为在线直连路由。
- **改动建议**：在 `shared/rust-core/src/peer/directory.rs` 新增共享纯函数 `select_local_lan_ip`，平台端改用该函数并废除 8.8.8.8 探针：
```rust
// shared/rust-core/src/peer/directory.rs
pub fn select_local_lan_ip(
    interfaces: impl IntoIterator<Item = (IpAddr, bool /* oper_up */, bool /* p2p */)>,
) -> Option<IpAddr> {
    fn rank(ip: &IpAddr) -> Option<u8> {
        match ip {
            IpAddr::V4(ip) if ip.is_private() => Some(0),
            IpAddr::V4(ip) if ip.is_link_local() => Some(2),
            IpAddr::V6(ip) if (ip.segments()[0] & 0xfe00) == 0xfc00 => Some(1),
            _ => None,
        }
    }
    interfaces.into_iter()
        .filter(|(ip, up, p2p)| *up && !*p2p && !ip.is_unspecified() && !ip.is_loopback())
        .filter_map(|(ip, _, _)| rank(&ip).map(|r| (r, ip)))
        .min_by_key(|(r, ip)| (*r, u128::from(*ip)))
        .map(|(_, ip)| ip)
}
```
并在 `api/routes.rs:81` 补齐 `&& local.tailscale_ip != "0.0.0.0"` 过滤。
- **风险评估**：低风险。纯计算函数，有详尽单测支撑。

#### 【S1-P0-2】`source_matches_mode` 模式重叠（`lan_only` 放行 Tailscale）
- **精确代码位置**：`shared/rust-core/src/peer/directory.rs:42-64`
- **机理分析**：Tailscale IPv6 前缀为 `fd7a:115c:a1e0::/48`，数学上位运算 `0xfd7a & 0xfe00 == 0xfc00`，完全落入 `lan_only` 的 `fc00::/7` 规则中，导致局域网模式下错误放行外部 Tailnet 连接。
- **改动建议**：修改匹配逻辑，使 Tailscale 判定严格优先并排他：
```rust
// shared/rust-core/src/peer/directory.rs
let is_tailscale = match ip {
    IpAddr::V4(ip) => ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]),
    IpAddr::V6(ip) => {
        let s = ip.segments();
        s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
    }
};
if is_tailscale {
    return matches!(mode, "tailscale" | "tailscale_only");
}
// 随后再匹配普通私有 IPv4 与非 link-local 的 IPv6 ULA
```
- **风险评估**：极低风险。保证安全模式边界。

#### 【S1-P1-3】候选池纯 ASCII 字典序排序导致首选地址被劫持
- **精确代码位置**：`shared/rust-core/src/peer/directory.rs:143-151`, `200-203`
- **机理分析**：纯字符串比较使 `"10.x"` 与 `"169.254.x"` 排在 `"192.168.x"` 前面，被赋给 `peer.address` 并持久化，使通信首选锁定在异常地址。
- **改动建议**：引入复合排序键 `(priority, unverified, address_class, numeric_ip)`，确保实测在线优于 remembered，私有物理网段优于 APIPA/回环；且 `:200` 赋值时 `tailscale_ip` 仅从 Tailscale 接口提取。
- **风险评估**：低风险。

#### 【S1-P1-4】IPv6 缺少 Scope ID 必报不可达，带 `%` 误报 Iroh 错
- **精确代码位置**：`shared/rust-core/src/peer/directory.rs:115-123`, `520-525`
- **机理分析**：构造 `SocketAddr` 时丢失网卡索引，`fe80::` 必定报 `EHOSTUNREACH`；用户手动输入带 `%en0` 时掉入 Iroh 解析器报 `Invalid Iroh endpoint ID`。
- **改动建议**：在 `parse_pairing_target` 中显式拦截含 `%` 或 `fe80::` 的地址并给出友好中文提示（请使用局域网 IPv4）；`resolve_candidates` 对 link-local fail-closed。
- **风险评估**：极低风险。

#### 【S1-P1-5】Windows 广播目标枚举缺失网卡状态位检查
- **精确代码位置**：`windows/src-tauri/src/network/lan.rs:103-119`, `164-173`
- **机理分析**：Windows 端未过滤 `!is_oper_up() || is_p2p()`，断网时盲向虚拟网卡发包成功伪装成“探测成功”，掩盖断网故障。
- **改动建议**：统一使用共享纯函数计算定向广播地址，Windows 端对齐 macOS 的断网报错行为。
- **风险评估**：低风险。

---

### 切片 2：多路径并发竞速、连接池与健康心跳状态机

#### 【S2-F1】迟到重复 ACK 污染读流，误判协议错断连
- **精确代码位置**：`shared/rust-core/src/peer/delivery/executor.rs:118-159`，`shared/rust-core/src/peer/delivery/worker.rs:402-416`
- **机理分析**：重传后对端发送了两个 ACK，滞留的 ACK 被下次读帧消费，由于 `sequence` 不匹配被误判为协议错，导致连接断开重连；留在心跳读帧处还会导致心跳假失败。
- **改动建议**：在 `executor.rs` 引入 `read_delivery_response`，跳过 `sequence != pending.sequence` 的残留 ACK；在 `worker.rs` 心跳中校验序列号并放行残留业务 ACK。
- **风险评估**：低风险。消灭网络抖动后的连接震荡。

#### 【S2-F2】投递失败路径缺少退避（0 延迟重连风暴）
- **精确代码位置**：`shared/rust-core/src/peer/delivery/worker.rs:377-385`, `493`
- **机理分析**：连接建立失败有 `reconnect_delay`，但投递失败直接 `continue` 回循环头，毫秒级死循环抢占 Settings 锁，引发重连风暴并卡死前端设置保存。
- **改动建议**：在投递失败分支补齐 `tokio::time::sleep(config.reconnect_delay)` 退避。
- **风险评估**：极低风险。

#### 【S2-F3】连接池 Key 包含动态 Target 强杀 Worker
- **精确代码位置**：`shared/rust-core/src/peer/pool.rs:72-139`，`shared/rust-core/src/peer/delivery/worker.rs:210-230`
- **机理分析**：Key 是 `(target, hostname)`，传大文件过程中只要后台网络自省使首选 IP 发生微小重排，旧 Worker 立即被发送 `request_shutdown()` 强杀，大文件传输回滚，在飞剪贴板事件永久丢失。
- **改动建议**：Key 改为纯 `hostname`，单设备维持单一活动 Worker，通过 `watch::channel` 平滑推送新候选集供重连时采纳，不杀存活传输。
- **风险评估**：中风险。核心调度优化。

#### 【S2-F4】候选延迟无条件 sleep 导致 300ms 延迟税
- **精确代码位置**：`shared/rust-core/src/peer/delivery/race.rs:12-21`, `58-62`
- **机理分析**：首选 LAN 即使在 1ms 内快速报拒绝，备用 Tailscale 仍无条件睡满 300ms 才能起跑，造成固定延迟。
- **改动建议**：优先候选计算仅统计 `candidate.online` 的活跃候选；或引入 `fast_lane` 通道在首选失败瞬间立即唤醒备选协程。
- **风险评估**：低风险。显著提升离线设备在 Tailscale 下的响应速度。

#### 【S2-F5】文件控制帧挤占 Priority 队列导致剪贴板丢帧
- **精确代码位置**：`shared/rust-core/src/peer/pool.rs:46-52`, `19-20`
- **机理分析**：`FileMeta` 走 `priority` 队列，串行等待长达 40s，导致后续剪贴板纯文本/图片在 64 深度队列中超时（5s）被无声丢弃。
- **改动建议**：拆分出独立的文件控制通道（`file_control`），交互剪贴板事件独占 `priority` 队列。
- **风险评估**：中风险。

---

### 切片 3：Noise 协议握手、配对协商与密钥持久化

#### 【S3-P0-1】匿名配对槽位抢占 + 失败配额可控消耗（DoS 漏洞）
- **精确代码位置**：`shared/rust-core/src/pairing/manager.rs:136-182`, `230-264`, `shared/platform-network-server.rs:321-334`
- **机理分析**：入站配对无需任何凭证，对端发起握手后直接挂断即可消耗本端 `failed_attempts`；达到 5 次配对窗口直接进入 `Locked` 锁定关闭。
- **改动建议**：
  1. 未经本地用户在 UI 点击“确认”的匿名连接，断开时只返回 Waiting，**绝不扣减 `failed_attempts`**；
  2. 允许新的配对握手抢占未被本地确认的旧槽位，防止占坑不拉屎。
- **风险评估**：低风险。彻底解决局域网/穿透匿名拒绝服务攻击。

#### 【S3-P1-1】握手 Message 2 过早泄露主机元数据
- **精确代码位置**：`shared/rust-core/src/secure/handshake.rs:260-268`, `shared/rust-core/src/secure.rs:22-37`
- **机理分析**：发起端仅出示临时公钥（未证明身份），服务端在第 2 步回执中即明文附带主机名、Tailscale IP、Iroh ID 与 App 版本，攻击者可全网嗅探资产指纹并借 Iroh ID 发起远程穿透。
- **改动建议**：Message 2 中将敏感元数据置空，仅保留 Hostname；完整数据推迟到发起端认证完成后的传输就绪阶段交互。
- **风险评估**：中风险。需配套微调握手完成后的元数据接收点。

#### 【S3-P1-2】配对单边持久化孤儿 (Asymmetric Orphan)
- **精确代码位置**：`shared/rust-core/src/pairing/manager.rs:414-471`, `shared/rust-core/src/crypto.rs:275-287`
- **机理分析**：本地确认后立即将对端写盘落盘，若此时对端掉电或网络中断，本端失败逻辑没有调用 `forget_peer` 回滚，导致 A 显示已配对，B 无记录，后续每次通信都被 B 坚决拒绝，单向静默死锁。
- **改动建议**：在配对超时或异常退出时，若发现本地已写盘但对端从未进入 Finalizing，自动回滚已落盘的信任记录。
- **风险评估**：低风险。

#### 【S3-P1-3】Hostname 静默覆盖信任锚与未认证路由投毒
- **精确代码位置**：`shared/rust-core/src/crypto.rs:297-298`, `325-339`
- **机理分析**：同名新设备配对无条件覆盖旧设备的公钥 pin；未认证广播直接篡改可信设备的保存地址。
- **改动建议**：`trust_peer_without_save` 遇到同名异钥直接报错拒绝；给地址来源标记 `AddressOrigin`，未认证广播不得覆写经握手认证的地址。
- **风险评估**：低风险。

#### 【S3-P1-4】`read_pairing_frame` 的 `select!` 造成 Future 错位破坏帧边界
- **精确代码位置**：`shared/rust-core/src/secure/handshake.rs:346-355`
- **机理分析**：`read_plain_frame` 基于非取消安全的 `read_exact`，在窗口状态监听循环中被中断重建，底层字节流错位导致配对握手离奇失败。
- **改动建议**：使用 `tokio::pin!` 固定读取 Future，在循环中复用，禁止中途抛弃重读。
- **风险评估**：极低风险。

---

### 切片 4：剪贴板大文件传输、分块流控与断点续传

#### 【S4-P1-1】接收端并发限流漏洞（漏算 pending/inflight）
- **精确代码位置**：`shared/rust-core/src/sync/receive_engine.rs:281-296`
- **机理分析**：限流统计只查 `active_receives`，而打开磁盘在锁外执行（处于 `pending_receives`），并发发起的 `FileMeta` 全部看到计数为 0，限流形同虚设。
- **改动建议**：限额计数严格统计 `active + pending + inflight` 之和。
- **风险评估**：极低风险。

#### 【S4-P1-2】续传配额预检重复计入已收前缀（双重记账）
- **精确代码位置**：`shared/platform-network-server.rs:536-546`, `shared/rust-core/src/db/storage.rs:158`
- **机理分析**：续传时磁盘 `used` 已含已有 `.part` 体积，预检又加了一遍全量 `total_bytes`，导致接近配额时续传必被拒绝，并多淘汰无辜历史。
- **改动建议**：预检时通过检查已有 `.part` 文件长度，只计算并预留未收增量字节数（`total - 已收`）。
- **风险评估**：低风险。彻底解决满盘大文件续传失败的顽疾。

#### 【S4-P1-3】配额预检在 DB 互斥锁内全盘递归扫描
- **精确代码位置**：`shared/rust-core/src/db/storage.rs:157-188`, `shared/platform-network-server.rs:499, 538`
- **机理分析**：`reserve_for_file_batch` 每淘汰一条就全量递归扫描整棵存储树，持锁期间霸占 Tokio 线程与 DB 锁，卡死历史查询。
- **改动建议**：预检前只扫一次磁盘基线，淘汰循环内直接依据 SQLite 记账扣减内存 `used`。
- **风险评估**：低风险。

#### 【S4-P1-4】处理 1MB 分块 4~5 次抢全局引擎锁且锁内广播
- **精确代码位置**：`shared/rust-core/src/sync/receive_engine.rs:488-495`, `macos/src-tauri/src/api.rs:176-206`
- **机理分析**：处理分块持锁调用 `set_receive_progress` 并一路触发 watch 广播，千兆传输下单流每秒触发上千次全局抢锁，拖慢整体吞吐。
- **改动建议**：锁内仅提取进度快照，将平台更新和 watch 广播移到释放锁后执行。
- **风险评估**：低风险。显著提升局域网分块传输速度。

#### 【S4-P1-5】`restart_batch` 触发整批次 32 次重复全盘重哈希
- **精确代码位置**：`shared/platform-clipboard-transfer.rs:680-722, 829-838`
- **机理分析**：断点重探时发送端重启循环，对批次内所有文件重新读取并计算 BLAKE3，1GB 批次最多导致 32GB 的本地无用读磁盘。
- **改动建议**：增加布尔向量缓存，同一传输会话中已验证过的文件跳过重复全盘读哈希。
- **风险评估**：极低风险。

#### 【S4-P2-1 & S4-P2-2】incoming 权限默认 umask 与孤儿文件缺常驻定时器
- **精确代码位置**：`shared/platform-network-server.rs:686`，`macos/src-tauri/src/lib.rs:351`
- **改动建议**：incoming 目录创建改用 `create_private_dir_all` 施加 0700；挂载每小时一次的后台孤儿清理定时器。
- **风险评估**：极低风险。

---

### 切片 5：SQLite 历史数据库、本地加密与配额淘汰

#### 【S5-P1-1】配额预检依赖文件系统大小判据抛假错误
- **精确代码位置**：`shared/rust-core/src/db/storage.rs:178-187`, `shared/platform-network-server.rs:540`
- **机理分析**：SQLite 在 WAL 模式下 `DELETE` 文本条目不会缩小物理 `.db` 文件体积。淘汰前后调用 `bulk_storage_size` 大小相等，触发 `used_after >= used_before` 误报 `"reclaimed no storage"` 并永久拒绝入站传输。
- **改动建议**：废除以文件系统尺寸判断清理效果的逻辑，改以 unpinned 条目严格递减为终止保证。
- **风险评估**：低风险。彻底解决大文件入站假错误拒传。

#### 【S5-P1-2】`get_image_data` 传入普通文件全量解密 5GB 进内存
- **精确代码位置**：`shared/rust-core/src/db/entries.rs:137-144`, `macos/src-tauri/src/commands/settings.rs:37-45`, `windows/src-tauri/src/commands/preview.rs:8-25`
- **机理分析**：未前置校验类型直接调用 `get_data`，对普通大文件全量解密进内存（最高 5GB），且持 DB 锁，造成 OOM 崩溃或界面卡死。
- **改动建议**：缩略图命令改走 `preview_async` 管道，在解密前严格校验 `kind == Image`，限制在 64MB 硬门槛内。
- **风险评估**：极低风险。

#### 【S5-P2-1 & S5-P2-2】外部文件删除容错与 WAL 截断降级
- **精确代码位置**：`shared/rust-core/src/db/lifecycle.rs:387, 402-404`
- **改动建议**：物理删除循环单文件容错（消除 `?`）；提交后的 WAL 截断错误降级为 `warn!`，不影响已成功提交的删除事务，防止前端误判失败。
- **风险评估**：极低风险。

---

### 切片 6：跨平台 IPC 契约、前端状态单向流与运行时协调器

#### 【S6-P0-1】Windows TCP API 预鉴权空连接挂起导致服务假死
- **精确代码位置**：`windows/src-tauri/src/api/transport.rs:40-62`, `windows/src-tauri/src/lib.rs:603-608`
- **机理分析**：16 个连接信号量在读取第一行前即被占满，Token 校验在读取之后，且单次读取超时长达 5s。本地任意无凭证进程开 16 个空 TCP 连接即可瘫痪 API 接口，阻断后续正版通信。生产环境下随机 Token 从不下发，监听该端口纯属白送攻击面。
- **改动建议**：生产环境若未设置 `TAILSYNC_API_TOKEN` 环境变量，默认不启动该 TCP 端口；若保留端口，隔离预鉴权槽位并施加 1 秒超时。
- **风险评估**：低风险。彻底关闭 Windows 本地无凭证攻击面。

#### 【S6-P2-3】SwiftUI 握手白名单放行 Windows 导致死循环重启
- **精确代码位置**：`macos/swift-ui/Sources/TailSync/Services/ApiClientRuntime.swift:26, 85-89`
- **机理分析**：白名单放行了 `"windows"`，但解码器强制要求 `status` 字段（Windows 快照无此字段）。一旦连上 Windows 守护进程反序列化必失败，看门狗每 3 秒杀死守护进程并重启，引发死循环。
- **改动建议**：白名单收敛为纯 `"macos"`。
- **风险评估**：极低风险。

#### 【S6-P2-4】Windows 后台不可见长轮询破坏性“偷吃” `sync_warning`
- **精确代码位置**：`windows/src-tauri/src/commands/platform.rs:45`, `shared/rust-core/src/sync_warning.rs:59-65`
- **机理分析**：后台长轮询直接调用破坏性单次读取 `sync_warning::take()`。窗口最小化在托盘时，后台轮询持续清空告警，用户点开窗口时告警已过 4.5s TTL 自毁。
- **改动建议**：快照查询改用无破坏性的 `peek()`，仅在前端组件真正可见时显式消费。
- **风险评估**：极低风险。

#### 【S6-P2-1 & S6-P2-6】通知溢出自动注入警告与长轮询支持断开取消
- **精确代码位置**：`macos/src-tauri/src/api.rs:61-75`，`macos/src-tauri/src/api/transport.rs:247-248`
- **改动建议**：客户端请求水位落后于缓冲最旧 ID 时，自动合成一条警告通知提示滚出；长轮询命令加入断开即取消白名单，及时归还连接许可。
- **风险评估**：极低风险。

---

## 四、生产级分阶段修复路线图 (PR Roadmap)

为确保代码合入的安全、可审与零回退，建议严格拆分为 **3 个批次（PR）** 逐步推进：

```
                              TailSync 生产级重构路线
  ┌────────────────────────────────────────────────────────────────────────┐
  │ PR 1: 安全漏洞封堵与网络防风暴 (P0 核心修复，改动 < 100 行，优先合入)    │
  │  - [S3-P0-1] 配对未确认会话不扣失败预算，允许新会话抢占未确认槽位      │
  │  - [S3-P1-2] 配对超时/失败时，自动回滚已落盘的单边孤儿记录            │
  │  - [S6-P0-1] Windows 生产默认不启动无凭证的 TCP API 监听器             │
  │  - [S1-P0-1] local_ip 改用非 p2p 物理枚举，排除 VPN 隧道与 0.0.0.0    │
  │  - [S1-P0-2] 修正 source_matches_mode，排除 lan_only 对 Tailscale     │
  │  - [S2-F1]   executor.rs 读流跳过陈旧 ACK，worker.rs 心跳校验序列号    │
  │  - [S2-F2]   worker.rs 投递失败补齐退避休眠，阻断死循环重连风暴        │
  └───────────────────────────────────┬────────────────────────────────────┘
                                      │
                                      ▼
  ┌────────────────────────────────────────────────────────────────────────┐
  │ PR 2: 存储健全性与大文件传输解耦 (彻底解决断点续传误拒与 OOM 隐患)       │
  │  - [S5-P1-1] reserve_for_file_batch 废除文件系统尺寸判据，改用行数保证  │
  │  - [S5-P1-2] get_image_data 走 preview_async，64MB 门槛前拦截非图片文件│
  │  - [S4-P1-1] 接收端并发限流统一累加 active + pending + inflight        │
  │  - [S4-P1-2] 续传配额预检扣除已有 .part 尺寸，只预留增量未收字节数    │
  │  - [S4-P1-3] 配额预检只扫一次磁盘基线，淘汰过程改用 DB 内存扣减        │
  │  - [S4-P1-4] 分块处理仅在锁内抓取进度快照，平台回调与 watch 广播移至锁外│
  │  - [S4-P1-5] 发送端缓存源文件校验状态，消除断点重探时的 32 倍重哈希风暴│
  │  - [S2-F3]   连接池 Key 剥离 Target 改为纯 hostname，无杀伤更新路由    │
  └───────────────────────────────────┬────────────────────────────────────┘
                                      │
                                      ▼
  ┌────────────────────────────────────────────────────────────────────────┐
  │ PR 3: 体验交互加固与跨平台边缘修补 (P2 级体验与环境整洁)                │
  │  - [S6-P2-4] Windows 快照改用 peek()，托盘后台不再吃掉同步告警         │
  │  - [S6-P2-3] SwiftUI 握手白名单收敛为纯 macos，防止看门狗死循环杀进程 │
  │  - [S1-P1-3] 候选按（验证状态 > 地址等级 > 数值IP）加权排序           │
  │  - [S1-P1-4] 优雅拦截无 Scope 的 IPv6，消除误导性 Iroh 错误            │
  │  - [S1-P1-5] Windows 广播枚举补齐状态位检查                           │
  │  - [S2-F4]   竞速未探活的 LAN 候选不再享受 300ms 排他特权             │
  │  - [S2-F5]   拆分文件元数据通道与即时剪贴板队列，彻底消灭丢帧          │
  │  - [S3-P1-3] 公钥去重拒绝静默覆盖同名设备，标记未认证广播防路由投毒   │
  │  - [S3-P1-4] read_pairing_frame 使用 tokio::pin! 确保取消安全          │
  │  - [S4-P2-1/2] incoming 目录施加 0700 权限，挂载每小时孤儿清理定时器   │
  │  - [S5-P2-1/2] 外部物理文件删除循环消除级联打断，WAL 截断错误降级为警告│
  │  - [S6-P2-1/6] 通知环形缓冲增加溢出丢条提示，长轮询纳入断开取消白名单 │
  └────────────────────────────────────────────────────────────────────────┘
```

---

## 五、回归验收门槛与质量断言

每个 PR 合入前，必须在本地及 CI 严格通过对应的自动化断言测试：

1. **安全与 DoS 防御断言（验证 PR 1）**：
   - 模拟攻击者向处于开启状态的配对窗口连续发起 10 次恶意握手并立即断开，断言窗口依然保持 `PairingPhase::Waiting`，`failed_attempts == 0`，正版客户端随后发起配对必须 100% 成功。
   - 开启全局 TUN 代理，断言 `select_local_lan_ip` 稳定返回局域网物理 IP，绝对不得返回 `198.18.x.x` 或 `0.0.0.0`。
   - 断言 `source_matches_mode("fd7a:115c:a1e0::1", "lan_only") == false`。
2. **大文件与存储配额断言（验证 PR 2）**：
   - 设定存储配额为 10GB，插入 100 条小文本条目，申请接收 6GB 文件批次，断言 `reserve_for_file_batch` 返回 `Ok(())`，不报 `reclaimed no storage`。
   - 创建一条 500MB 的文件记录，调用 `get_image_data` 传入其 ID，断言函数在 1ms 内返回类型错误，进程内存绝对不得增长超过 10MB。
   - 模拟已有 500MB `.part` 文件的 1GB 批次续传，断言在剩余磁盘空间仅 700MB 时预检依然通过。
3. **交互体验与跨平台断言（验证 PR 3）**：
   - Windows 端最小化到托盘，触发一次传输停滞告警，运行长轮询 10 秒后打开窗口，断言前端成功捕获并展示告警 Toast。
   - 断言 `ApiClientRuntime.swift` 在连接非 macOS 平台时提前友好拒绝，不触发看门狗反复杀守护进程。
   - 断言候选池在面对 APIPA、内网 VPN 和物理 Wi-Fi 时，物理 Wi-Fi 永远稳居第 0 项。
