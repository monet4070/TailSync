# ADR（草案）：Windows clipboard helper protocol 2 → 3

状态：PROPOSED — 等待 G1/人类确认；W05 生产切换以前置条件引用本记录。
日期：2026-11-05。
范围：仅本机 sidecar 协议；与线协议 v4、主历史 schema、产品版本无关。

## 背景

Windows 生产采集将从 `--read-snapshot`（CNS1）切换到 `--read-archive`（CNA1 platform 2），
以分离“原件”和“派生 PNG”，并在 helper 内先执行 Windows 隐私控制格式检查。
旧 helper 不实现 `--read-archive`，也不执行隐私门禁。若新 daemon 误用旧 helper，
会把 CNA1 命令失败解释为普通采集失败或回退到 legacy 读取，从而绕过隐私门禁。

## 决策

1. Windows helper 自检版本改为 `protocol=3`；macOS 保持 `protocol=4`。
2. daemon 的 `HELPER_PROTOCOL_VERSION` 在 Windows 目标为 3，macOS 仍为 4。
3. 新 daemon 遇到旧 helper（`protocol=2`）时判定 `VersionMismatch`：
   - 不调用不健康的 helper 读取剪贴板；
   - 不切到 legacy 文本/图片/文件读取；该事件按不可采集失败处理并记录固定故障类别；
   - 保留历史库与已有 v13 数据可读性。
4. 旧 daemon + 新 helper（helper 3，daemon 期望 2）：同样判定版本不匹配，不自动降级调用。
5. helper 缺失或不可执行：沿用既有健康门禁报告；legacy 路径仍必须经过 daemon 侧隐私预检。
6. 功能关闭态（`TAILSYNC_DISABLE_COMPOSITE_CLIPBOARD` 或服务不可用）不调用 helper，
   但 legacy 路径仍执行隐私预检。
7. macOS 发布包不携带 Windows helper，协议 4 与既有 QQ/Qt/RTFD 行为不变。

## 兼容矩阵

| 组合 | 结果 |
| --- | --- |
| 新 daemon 3 + 新 helper 3 | 正常：`--read-archive` / `--read-snapshot` / 发布 |
| 新 daemon 3 + 旧 helper 2 | `VersionMismatch`；不读取、不回退到 legacy |
| 旧 daemon 2 + 新 helper 3 | `VersionMismatch`；旧 daemon 不使用新 helper |
| 缺 helper | `Missing`；legacy 读取仍受隐私预检 |
| helper 不可执行/自检超时 | `ProbeFailed`；不绕过门禁 |
| 功能关闭 | 不调用 helper；legacy 受隐私预检 |
| 既有 v13 数据库 | 只读/写入语义不变 |
| macOS helper 4 | 不变 |

## 未决事项

- 本轮只实现 W00–W05；自动更新/安装包一致性由 W09 负责。
- G1 前不得把本 ADR 标记为 ACCEPTED；若人类否决协议升级，W05 必须回退为不切换生产路径。