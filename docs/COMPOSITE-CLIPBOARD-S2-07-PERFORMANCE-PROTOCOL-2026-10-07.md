# 复合剪贴板 S2-07 性能报告协议（`tailsync-s2-performance/v1`，2026-10-07）

本协议对应 S2-07「性能与验收封存」中**可本地自动完成**的纯报告部分。配套工具是 [`scripts/s2-performance-report.py`](../scripts/s2-performance-report.py)，单元测试是 [`scripts/tests/test_s2_performance_report.py`](../scripts/tests/test_s2_performance_report.py)。工具复用 [`scripts/validate-s2-06-evidence.py`](../scripts/validate-s2-06-evidence.py) 的 `open_regular` / `read_bounded` 有界只读加载原语（不修改该工具），因此没有重复实现路径解析。

## 0. 边界与不可推断项

- 工具是**离线纯报告器**：不启动子进程、不访问网络、不读写剪贴板、不连接数据库、不做缓存或设备发现。它只读取显式的 `--input`（以及可选 `--baseline`），且这些文件必须位于显式 `--evidence-root` 之下。
- 输出类别只有 `INVALID`（退出码 2）与 `REPORT_COMPLETE`（退出码 0）。`REPORT_COMPLETE` 可以包含大量 `UNKNOWN`，**不是**验收 PASS。
- `gui_acceptance` 恒为 `UNKNOWN`，`release` 恒为 `NO-GO`，`real_a07` / `real_a02` 恒为 `UNKNOWN`，`full_s2` 恒为 `false`。S2-07 不能代表 S2 完整，也不能代表发布放行。
- S2-06 的真实 QQ / A02 现场证据在本阶段**未获授权**，因此缺失；本工具不产生、不继承、不推断任何真实证据。
- 阈值只用于诊断标注（`REVIEW_REQUIRED` / `OK`），**不会**自动改变验收或发布结论。阈值内（≤10%）也不构成发布证明。有限输入的 Decimal 比较使用局部 1024 位精度，避免大整数在默认 28 位上下文中丢失略超阈值的增量；不修改进程全局 Decimal 上下文。
- 若聚合数值超出 JSON 输出运行时可表示范围，返回固定 `INVALID / E_OUTPUT_NUMBER`，不放宽 Python 全局整数转换限制，不产生 traceback 或部分报告。
- `source_sha256` / `helper_sha256` / `fixture_sha256` / `workload_sha256` 只是**申报值**，由外部核对；工具不做产物认证。

## 1. 输入 schema（先给出，便于构造样本）

顶层字段（缺一不可，多一个即 `E_FIELDS`）：

| 字段 | 类型 | 约束 |
| --- | --- | --- |
| `schema` | string | 固定 `tailsync-s2-performance/v1` |
| `report_id` | string | 1–128 个可打印字符；只在内部使用，不进入摘要输出 |
| `measurement_kind` | enum | `headless_saved_bytes` 或 `native_gui` |
| `environment` | object | 见下表 |
| `bindings` | object | 见下表 |
| `scenarios` | array | 非空；每项见下表 |

`environment`（四字段，缺一或多一即 `E_ENVIRONMENT_FIELDS` / `E_ENVIRONMENT_VALUE`）：

| 字段 | 类型 | 约束 |
| --- | --- | --- |
| `os` | string | 1–128 个可打印字符 |
| `arch` | string | 1–128 个可打印字符 |
| `build_profile` | enum | `debug` / `release` / `profiling` / `benchmark` |
| `hardware_id` | string | 固定标识，1–128 个可打印字符；不进入摘要输出 |

`bindings`（四字段，值必须是 64 位小写十六进制）：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `source_sha256` | string | 源码/工作树指纹申报值 |
| `helper_sha256` | string | helper 二进制摘要申报值 |
| `fixture_sha256` | string | 测试夹具摘要申报值 |
| `workload_sha256` | string | 规范化负载定义的摘要申报值；采集脚本字节摘要另行绑定，不能混称 |

`scenarios[]` 每项：

| 字段 | 类型 | 约束 |
| --- | --- | --- |
| `scenario` | enum | 见第 2 节 |
| `status` | enum | `measured` 或 `unknown` |
| `trials` | array | `status=unknown` 时必须为空；`status=measured` 时 20–512 条 |

`trials[]` 每项（缺字段或多余字段即 `E_TRIAL_FIELDS`）：

| 字段 | 类型 | 约束 | 单位 |
| --- | --- | --- | --- |
| `wall_ms` | number | 有限、严格大于 0 | 毫秒 |
| `cpu_user_ms` | number / null | 有限、非负；不可用时为 null | 毫秒 |
| `cpu_system_ms` | number / null | 有限、非负；不可用时为 null | 毫秒 |
| `rss_peak_bytes` | integer / null | 严格大于 0；不可用时为 null | 字节 |
| `decode_count` | integer / null | 非负；不可用时为 null | 次 |
| `convert_count` | integer / null | 非负；不可用时为 null | 次 |
| `cache_hits` | integer / null | 非负；不可用时为 null | 次 |
| `cache_misses` | integer / null | 非负；不可用时为 null | 次 |
| `final_lease_released` | boolean / null | 不可用时为 null | 布尔 |

布尔值（`true` / `false`）不算数字：任何数值字段收到布尔值即 `E_TRIAL_TYPE`。

完整示例（占位符，非真实测量；`headless_saved_bytes` 下八个 GUI 场景只能 `unknown`）：

```json
{
  "schema": "tailsync-s2-performance/v1",
  "report_id": "s2-07-headless-r1",
  "measurement_kind": "headless_saved_bytes",
  "environment": {
    "os": "macos-<version>",
    "arch": "arm64",
    "build_profile": "release",
    "hardware_id": "hw-<fixed-identifier>"
  },
  "bindings": {
    "source_sha256": "<64-hex>",
    "helper_sha256": "<64-hex>",
    "fixture_sha256": "<64-hex>",
    "workload_sha256": "<64-hex>"
  },
  "scenarios": [
    {"scenario": "idle", "status": "unknown", "trials": []},
    {"scenario": "continuous_copy", "status": "unknown", "trials": []},
    {"scenario": "list_scroll", "status": "unknown", "trials": []},
    {"scenario": "first_image", "status": "unknown", "trials": []},
    {"scenario": "fast_switch", "status": "unknown", "trials": []},
    {"scenario": "repeated_document", "status": "unknown", "trials": []},
    {"scenario": "cancellation", "status": "unknown", "trials": []},
    {"scenario": "close", "status": "unknown", "trials": []},
    {"scenario": "helper_analyze_rtfd", "status": "measured", "trials": [
      {"wall_ms": 12.4, "cpu_user_ms": 3.1, "cpu_system_ms": 0.8, "rss_peak_bytes": 41943040,
       "decode_count": 1, "convert_count": 1, "cache_hits": 0, "cache_misses": 0, "final_lease_released": true}
    ]}
  ]
}
```

上面的 `helper_analyze_rtfd` 实际需要 ≥20 条 trial；这里只示意一条，故直接提交会得到 `E_CARDINALITY`。可用 `--template` 打印空 schema（无测量数据，退出码 0）。

## 2. 场景定义（八项必需 + 一项可选）

八个 GUI 场景，缺失即在该行报告 `UNKNOWN`（`reason: ROW_MISSING`），不算 INVALID：

| 场景 | 含义 |
| --- | --- |
| `idle` | 空闲驻留开销 |
| `continuous_copy` | 连续复制的稳态开销 |
| `list_scroll` | 历史列表滚动帧时 |
| `first_image` | 首次图片预览的端到端时延 |
| `fast_switch` | 快速切换选中项 |
| `repeated_document` | 重复文档解析/渲染 |
| `cancellation` | 取消操作收敛时间 |
| `close` | 关闭窗口/面板的退出时间 |

可选行 `helper_analyze_rtfd`：只测量 headless helper 的 RTFD 分析路径（含进程启动）。

## 3. `measurement_kind` 语义（headless 不能冒充 GUI）

- `headless_saved_bytes`：headless helper 的挂钟时间，**包含进程启动**，不含 App/前端路径。此类别下，八个 GUI 场景只能 `unknown`；若某 GUI 行标为 `measured`，判 `E_KIND_MISMATCH`（`INVALID`）。
- `native_gui`：应用路径上的 GUI 采样。此类别下 `helper_analyze_rtfd` 只能 `unknown`；若标为 `measured`，判 `E_KIND_MISMATCH`。
- 无论哪种类别，`gui_acceptance` 都恒为 `UNKNOWN`：headless 数据不能填充 GUI 验收，`native_gui` 也需要真实公开会话与隔离候选才能成立（见第 10 节）。

## 4. 样本、单位与统计

- 每个 `measured` 行至少 20 条、至多 512 条 trial；整个输入 JSON ≤ 65536 字节（超出即 `E_INPUT_TOO_LARGE`）。
- 保留**原始** trial 输入，不制造数值。某指标在任一条 trial 为 null 时，该指标的聚合为 `UNKNOWN`，**不**按 0 处理。
- 百分位采用 nearest-rank：对升序样本 \(v_0 \le v_1 \le \dots \le v_{n-1}\)，

$$
\text{index} = \min(\max(\lceil p \cdot n \rceil - 1,\ 0),\ n - 1)
$$

- `wall_ms`、`cpu_user_ms`、`cpu_system_ms` 输出 `p50` / `p95` / `min` / `max`，单位毫秒；`rss_peak_bytes` 同样输出这些统计，单位字节。CPU 与 RSS 是**每条 trial 的量**，不是 rusage 累计值；不得把子进程 peak RSS 冒充前端/App RSS。
- 统计保留原始样本精度，不把小于 0.000001 ms 的正时延舍入为 0。超过有限数字表示范围的超大整数是 INVALID，而不是 Python traceback。
- 计数器 `decode_count` / `convert_count` / `cache_hits` / `cache_misses` 只在全部 trial 都提供时求和（单位：次）；`final_lease_released` 只在全部 trial 都提供时给出布尔值（全为 `true` 才为 `true`）。任何一条为 null 即 `UNKNOWN`，不从循环次数或退出码推断。

## 5. 输出

摘要为 JSON（stdout），关键字段：

- `class`：`INVALID` 或 `REPORT_COMPLETE`。
- `invalid_codes`：固定错误码集合（见第 7 节）。
- `measurement_kind`、`environment_build_profile`：固定枚举；不回显 `os` / `arch` / `hardware_id` / `report_id`。
- `declared_bindings`：四个申报 hash（十六进制）。
- `coverage`：`measured` / `unknown` / `required_missing` 三个列表，以及 `helper_analyze_rtfd` 的状态（`MEASURED` / `UNKNOWN` / `ABSENT`）。
- `scenarios`：逐场景的指标状态与数值统计。
- `comparison`：基线比较结论（第 6 节）。
- `limits`：本协议的门限常量。
- `gui_acceptance` / `release` / `real_a07` / `real_a02` / `full_s2`：恒为 `UNKNOWN` / `NO-GO` / `UNKNOWN` / `UNKNOWN` / `false`。

退出码：`0` = `REPORT_COMPLETE`；`2` = `INVALID`；`--template` 输出空 schema 并以 `0` 退出。

## 6. 基线比较（可选 `--baseline`）

仅当**两侧都可比较**时才比较：`scenario`、`measurement_kind`、`environment`（`os` / `arch` / `build_profile` / `hardware_id`）、`helper_sha256`、`fixture_sha256`、`workload_sha256` 全部一致，且两侧样本数足够（基线 ≥20）。`source_sha256` 允许不同，但两侧都必须是合法 source ID（64 位小写十六进制）。

- 没有 `--baseline`：`comparison.status = NO_BASELINE`（不是 PASS）。
- 基线文件本身非法：`comparison.status = BASELINE_INVALID`；**不**因此把当前报告判为 `INVALID`。
- 上下文不一致：`comparison.status = NOT_COMPARABLE`，并给出 `reasons`（`MEASUREMENT_KIND_DIFFERS` / `ENVIRONMENT_DIFFERS` / `HELPER_DIFFERS` / `FIXTURE_DIFFERS` / `WORKLOAD_DIFFERS`）。
- 可比较时：按 `wall_ms` 的 `p95` 计算 \(\Delta\% = \frac{\text{current}_{p95} - \text{baseline}_{p95}}{\text{baseline}_{p95}} \times 100\)。\(\Delta\% > 10\) 标 `REVIEW_REQUIRED`，否则标 `OK`；两者都只是诊断，`release` 仍为 `NO-GO`。
- 父二修使用原始 p95 的十进制表示计算阈值判定，不按六位显示舍入结果判断；略高于 10% 不能被四舍五入成 OK。极端比例无法用有限 float 表达时，`delta_percent: null`、`reason: DELTA_OUT_OF_RANGE`，诊断 verdict 仍基于有限的 Decimal 运算；不输出 Infinity/NaN。
- 两侧都没有实际可比较的 measured pair 时为 `NOT_COMPARABLE` / `NO_MEASURED_PAIRS`，不是 `COMPARED` 空字典；缺失基线场景的逐行 UNKNOWN 保留。
- 基线的缺失场景或样本不足：该场景标 `UNKNOWN`（`BASELINE_SCENARIO_MISSING` / `BASELINE_CARDINALITY_LOW`）。
- 构建、探针、环境不同的原始计时**不得**混用；比较前必须先通过上述一致性检查。

## 7. 错误码（固定枚举，不包含输入内容）

底层 syscall 读取失败、root 变化或权限错误同样输出固定 `E_INPUT_READ`，不得向 stdout/stderr 回显本地路径或 traceback。使用安全 POSIX 原语；不支持原语的宿主 fail-closed，不据此声称 Windows 原生已验证。

| 码 | 触发 |
| --- | --- |
| `E_ARGUMENTS` | 参数缺失或 `--template` 与其它参数混用 |
| `E_TOOL_DEPENDENCY` | 无法加载 S2-06 安全加载器 |
| `E_INPUT_ROOT` | `--evidence-root` 不是目录或本身是符号链接 |
| `E_INPUT_PATH` | 输入不在 evidence-root 之下，或相对路径不安全 |
| `E_INPUT_SYMLINK` | 输入路径任一组件是符号链接 |
| `E_INPUT_NOT_FILE` | 输入不是普通文件（含目录） |
| `E_INPUT_MISSING` | 输入不存在 |
| `E_INPUT_CHANGED` | 读取期间文件大小/元数据变化 |
| `E_INPUT_READ` | 其它读取错误 |
| `E_INPUT_TOO_LARGE` | 输入 > 65536 字节 |
| `E_INPUT_JSON` | 非法 JSON 或顶层不是对象 |
| `E_INPUT_DUPLICATE_KEY` | JSON 重复键 |
| `E_INPUT_CONSTANT` | JSON 出现 `NaN` / `Infinity` |
| `E_OUTPUT_NUMBER` | 聚合数值无法按当前运行时限制生成标准 JSON；固定 INVALID，无部分输出 |
| `E_SCHEMA` | `schema` 不匹配 |
| `E_FIELDS` | 顶层字段缺失或多余 |
| `E_TYPE` | 结构类型错误 |
| `E_REPORT_ID` | `report_id` 非法 |
| `E_ENUM` | `measurement_kind` 非枚举值 |
| `E_ENVIRONMENT_FIELDS` / `E_ENVIRONMENT_VALUE` | 环境字段多余 / 取值非法 |
| `E_BINDING_FIELDS` / `E_HASH` | 绑定字段多余 / hash 非 64 位小写十六进制 |
| `E_SCENARIO_FIELDS` / `E_SCENARIO_UNKNOWN` / `E_SCENARIO_DUPLICATE` | 场景字段多余 / 未知场景名 / 重复场景 |
| `E_STATUS` / `E_STATUS_CONTRADICTION` | 状态非枚举 / `unknown` 行却带 trial |
| `E_TRIAL_FIELDS` / `E_TRIAL_TYPE` / `E_TRIAL_VALUE` | trial 字段缺失或多余 / 类型错 / 取值越界 |
| `E_TRIAL_COUNT` | 单场景 trial 数 > 512 |
| `E_CARDINALITY` | `measured` 行 trial 数 < 20 |
| `E_KIND_MISMATCH` | headless 声称测得 GUI 场景，或 native_gui 声称测得 helper 行 |

## 8. 隐私与有界读取

- 只读取显式路径的普通文件，逐组件拒绝符号链接；先按 `st_size` 拒绝超预算文件，再以有界读取并校验读取前后元数据一致。
- 摘要只输出固定枚举、数值统计与十六进制 hash；**不回显** `os` / `arch` / `hardware_id` / `report_id` 或任何自由文本，因此恶意字符串不会反射到输出。

## 9. 使用命令

```bash
python3 scripts/s2-performance-report.py --template
python3 scripts/s2-performance-report.py --input <input.json> --evidence-root <evidence-root>
python3 scripts/s2-performance-report.py --input <input.json> --evidence-root <evidence-root> --baseline <baseline.json>
python3 scripts/tests/test_s2_performance_report.py -v
```

## 10. 未知与后续门禁（本阶段不做）

- `native_gui` 的八个场景需要**确认真实公开会话 + 隔离候选**后由父级/用户在授权下采集；本阶段**不运行**、不伪造 App 的 `p50` / `p95`。
- 真实 QQ（S2-06）与 A02 现场证据仍为 `UNKNOWN`；headless 数据不能替代。
- Windows、双设备、真实目标应用与托管 CI 证据不在本工具范围内，保持 `UNKNOWN`。
- 本协议不改变产品代码、后端、Rust/Swift/React、依赖、锁文件或 schema；仅提供报告与协议。
