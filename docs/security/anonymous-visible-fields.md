# 匿名可见字段矩阵（已裁定的现状）

> 对应审计项 `S3-P1-1`。**裁定结论：接受现状，不做行为改动**，并以本文档 + 三处"固定集合"测试锁定，使将来任何改动都必须是有意为之。
> 基线：`origin/main` = `b61a0a04b88f5828707f7ef64781389944471be8`。

## 结论先说

审计原文写的是"Message 2 未认证明文泄露"。**这不准确**：Message 2 的载荷经 Noise 加密，**被动监听者读不到**。能被读到的是**主动发起握手方**——它在 Message 2 时点尚未通过认证。

更重要的是：设备名与 Iroh 标识**在握手之前就已经通过 mDNS 与 UDP 发现公开放广播**。因此"只收紧握手"几乎不产生隐私收益；要做就得连发现协议一起改，代价是附近设备不再显示真名、配对识别变难。

## 谁能看到什么

| 观察者 | 能获得 | 依据 |
|---|---|---|
| **被动监听者**（不参与握手） | **什么都读不到**（Noise 加密） | `secure/handshake.rs` 经 Noise `write_frame` 发出 |
| **同一局域网的任意人（握手前）** | mDNS TXT：`protocol`、`hostname`、`fingerprint`、`iroh`、`iroh_rtt`；UDP 发现响应：`app`、`version`、`hostname`、`tcp_port`、`iroh_endpoint_id`、`iroh_rtt` | `network/mdns.rs`、`network/lan.rs` 的 `DiscoveryResponse` |
| **主动匿名 Noise XX 发起者**（不打算配对） | 上述全部 **加上** Message 2 的：`hostname`、`tailscale_ip`、`iroh_endpoint_id`、`protocol_version`、`app_version`、`capabilities`、`max_wire_version` | `secure.rs` 的 `PeerIdentity::serialize` + `HandshakeAdvertisement` |

**相对发现阶段，真正的增量只有 `tailscale_ip`**（以及协议/产品版本号）。

## 为什么这是可接受的

1. **不是凭据。** `tailscale_ip` 是 CGNAT 段地址（`100.64.0.0/10` 或 ULA `fd7a:115c:a1e0::/48`）。知道它**不能**连上任何东西：从公网不可路由，要连通必须属于同一 tailnet 且持有有效密钥；也无法反推出 tailnet 名称、账号或公钥。
2. **危害是"可关联"而非"可入侵"。** tailnet 地址每设备固定、跨网络不变，因此同网段观察者可跨网络认出同一台设备；这与设备名本来就提供的可关联性同类，只是多暴露"这台设备在某个 tailnet 里"。
3. **不是唯一来源。** 设备名与 Iroh 标识在发现阶段已公开。要真正收敛必须同时改发现协议，代价落在配对与设备识别的用户体验上。
4. **对照：什么才是真正严重的泄露** —— 私钥、tailnet 认证密钥、tailnet 名称、公钥指纹之外的凭据。**这些一律不发送。**

## 固定集合的测试（防止无意漂移）

| 固定对象 | 测试位置 |
|---|---|
| 握手 Message 2 的身份字段集合 | `shared/rust-core/src/secure/tests.rs::anonymous_handshake_identity_fields_are_pinned` |
| LAN 发现响应字段集合（两端） | `network/lan.rs` 测试模块中的 `lan_discovery_response_fields_are_pinned` |

mDNS 的 TXT 键在 `network/mdns.rs` 内联构造，**目前只有文档记录、没有测试固定**；若要它也被门禁保护，需要先把键表提取成可断言的常量。

## 重新打开条件

- 用户或产品的隐私要求变为"同网段不得看出设备参与某个 tailnet"；
- 或发现阶段开始广播新的身份字段（此时上表与"增量只有 tailscale_ip"的判断都需要重估）。
