# TailSync 主题指南（Theme V2：包格式、内置主题与本地选择）

> 适用版本：产品 2.2.2，Theme V2（解析、校验与存储实现位于 `shared/tailsync-themes/`，两端只负责渲染适配）。
> 本文档描述 V2 主题包格式、内置主题 ID、本地选择语义、旧版迁移规则与全部实际限额。

## 0. 先读这一段：主题系统如何工作

**Theme V2 取代了旧的运行时自定义主题系统。** 三类事实需要区分：

1. **内置主题**——编译期内建于共享 core 的 5 个主题，有固定的 V2 ID（见 §1），任何设备上
   行为一致，不可被用户文件覆盖。
2. **自定义主题包**——一个 `.tailsync-theme` 文件（zip 归档，内含 `theme.json` 清单和可选
   `assets/` 图片），用户经设置页导入后安装到 `{数据目录}/themes-v2/`（见 §3）。
3. **本地选择**——每台设备自己的主题偏好，保存在 `{数据目录}/themes-v2/local-settings.json`
   （`activeThemeId` / `appearance` / `highContrast`，见 §5）。选择**不再**写入
   `config-v2.json`，因此**不再跨设备同步**；同步的只有设置里与主题无关的其余字段。

两端渲染方式：Windows 前端（`useTheme`）调用 daemon 的 `resolve_theme` 拿到解析后的令牌，
用 `style.setProperty()` 注入 CSS 变量；macOS SwiftUI 调用 `ApiClient.resolveThemeV2` 拿到
`TailSyncThemeDefinition` 后以 SwiftUI 值类型应用。解析、校验、存储全在共享 core，两端只有
渲染边界不同。

## 1. 内置主题与 ID

### 1.1 五个内置主题（V2 ID）

| V2 ID | 展示名 | 气质 |
|---|---|---|
| `builtin:canvas@1` | Canvas | 默认主题；暖纸底、衬线展示字体、大标题搜索 |
| `builtin:flux@1` | Flux | 青色系、几何无衬线、紧凑圆角 |
| `builtin:ledger@1` | Ledger | 绿色系、书籍式衬线、小圆角、大写节标题 |
| `builtin:aura@1` | Aura | 玫红系、圆体、最大圆角 |
| `builtin:mono@1` | Mono | 无障碍主题；灰阶语义色、零圆角、等宽展示字体 |

内置 ID 是保留字：自定义主题不得以 `builtin:` 开头，`valid_id` 只接受这两个形状——

- `builtin:<id>` 且必须是上面 5 个之一；
- `custom:<author>.<name>`，author 与 name 各 1–32 字符，仅小写 ASCII 字母、数字、连字符。

### 1.2 旧版（V1）内置主题映射

升级前 `config-v2.json` 的 `color_theme` 值在启动迁移时按下表映射（见 §6）：

| 旧 `color_theme` | V2 内置主题 |
|---|---|
| `tailsync` | `builtin:canvas@1` |
| `ocean` | `builtin:flux@1` |
| `forest` | `builtin:ledger@1` |
| `rose` | `builtin:aura@1` |
| `high-contrast` | `builtin:mono@1` |

## 2. 解析模型

`resolve_theme(id, mode, platform, high_contrast)` 按固定顺序合并令牌并返回 `ResolvedTheme`
（`tokens` + `provenance` + `assets` + `assetSlots`）：

1. **基线**：`builtin:canvas@1` 的完整令牌树（所有主题都必须提供 `colors/background/canvas`
   与 `colors/text/primary`，其余令牌在验证时按需补齐）；
2. **自定义包覆盖**：包的 `light` / `dark` 令牌树逐键覆盖基线（包可只写要改的键）；
3. **平台覆盖**：`platform.windows` / `platform.macos` 再覆盖；
4. **高对比度策略**：`high_contrast` 为 true 时，先应用包的可选 `highContrast` 令牌对，再由
   core 的对比度策略把不达标的文字/边框提升为黑或白，并把透明度颜色合并不透明（`systemHighContrast`
   来源标记在 `provenance` 中）。

令牌组（`validate_tokens` 白名单）：`colors`（background/text/accent/border/status）、
`typography`（ui/display/reading/search/section/history）、`density`（control/row）、
`shape`（controlRadius/surfaceRadius/windowRadius）、`effects`（opacity/shadow/motion）、
`components`（search/history/section/panel/button/input/toast，每组件
default/hover/active/selected/disabled/focus 六态）。颜色令牌支持 `#rrggbb`、`rgba()`、
`ref:` 引用、`alpha()`、`mix`、`contrastColor`、`systemAccent` 表达式；任何含 `url(`、`<`、
`;`、`@import` 的字符串一律拒绝（§7）。

## 3. 包格式（`.tailsync-theme`）

一个主题包是 **zip 归档**，扩展名 `.tailsync-theme`，只允许两类条目：

| 条目 | 必需 | 说明 |
|---|---|---|
| `theme.json` | ✅ | 清单，见 §4 |
| `assets/*` | ❌ | 图片资产，仅 PNG/JPEG（按文件头魔数判定），见 §3.1 |

其余任何条目（目录、`theme.json` 以外的根文件、非 `assets/` 前缀文件）都会使包被拒绝；
绝对路径与 `..` 遍历路径同样拒绝。

安装后包存放在 `{数据目录}/themes-v2/{id 目录}/`：

```
themes-v2/
├── local-settings.json          # 本地选择（§5）
├── .themes-v2.lock              # 跨进程互斥锁（不计入任何限额）
└── {author}_{name}/             # 例如 custom:studio.night → studio_night/
    ├── current.tailsync-theme   # 当前生效的包（校验后安装）
    └── rollback.tailsync-theme  # 上一次的包（更新回滚用）
```

更新是"先装 rollback、成功后再提升为 current"的两步交换，中途断电由启动时恢复逻辑收尾；
`delete_theme` 只按安装目录删除，损坏包仍可通过 `storageHandle` 删除。

### 3.1 资产

- 仅 `image/png` 与 `image/jpeg`（魔数判定：PNG 签名 / `FF D8 FF` 起始）；SVG/GIF/WebP 永不接受。
- 每个资产 ≤ **10 MiB**；单边尺寸 ≤ **8192 px**；**所有资产累计解码像素 ≤ 8000 万**（80,000,000）。
- 语义槽位 `assetSlots` 只允许 `logo`、`emptyState`、`previewPlaceholder` 三个键，值必须是
  包内真实存在的 `assets/...` 条目；渲染端取不到或解码失败时回退内置图形，不报错。

## 4. `theme.json` 清单规范

字段名一律 **camelCase**，未知字段直接拒绝（`deny_unknown_fields`）：

| 字段 | 必需 | 规则 |
|---|---|---|
| `formatVersion` | ✅ | 必须为 `2` |
| `id` | ✅ | `custom:<author>.<name>`（§1.1 规则） |
| `version` | ✅ | SemVer（含预发布标识符） |
| `minCoreVersion` | ✅ | SemVer，且 ≤ 当前 Core 版本（2.2.2），否则包被拒绝 |
| `name` | ✅ | 语言名映射，至少含 `"en"` 键 |
| `extends` | ✅ | 仅允许 `"builtin:canvas@1"`（只继承 Canvas） |
| `requiredCapabilities` | ❌ | 白名单：`theme-v2`、`high-contrast`、`platform-overrides`；声明不支持的
  能力 = 拒绝（fail closed） |
| `digest` | ❌ | 若提供，必须等于包字节的 Blake3 摘要（防篡改） |
| `signature` | ❌ | 保留字段，当前不校验 |
| `light` / `dark` | ✅ | 令牌树，见 §2；两者都必须包含 `colors/background/canvas` 与
  `colors/text/primary` |
| `highContrast` | ❌ | `{ light, dark }` 令牌对，高对比度模式专用 |
| `foundation` / `components` | ❌ | 令牌对象（缺省为空对象） |
| `platform` | ❌ | `{ windows, macos }` 平台覆盖令牌（缺省为空对象） |
| `assetSlots` | ❌ | 语义资产槽位映射（§3.1） |

## 5. 本地选择语义（`local-settings.json`）

```jsonc
{ "activeThemeId": "builtin:canvas@1", "appearance": "system", "highContrast": false }
```

- `activeThemeId`：内置 ID 或已安装的自定义 ID；`appearance`：`system` / `light` / `dark`；
  `highContrast`：布尔。
- **文件缺失时**应用默认值 `builtin:canvas@1` / `system` / `false`；文件损坏时同样回退默认值。
- **选择是本地偏好，不跨设备同步**（V2 起不再占用 `config-v2.json` 字段）。设备内多窗口经
  `theme_changed` 事件即时联动。
- 写入是**原子写**（临时文件 + rename）并持有 `.themes-v2.lock`；写入自定义 ID 前会先解析
  校验该包在两端、两模式下都可用，**绝不持久化悬空选择**。
- 已选择的包被删除时：选择自动回退 `builtin:canvas@1`（原子提交），`appearance` 与
  `highContrast` 保持不变。
- 应用期兜底：选择引用的包缺文件或损坏时，应用显示默认主题（Canvas），存储值不被改写。

### 5.1 daemon 命令面

Windows（Tauri invoke）与 macOS（本地 API，SwiftUI `ApiClient`）同名：
`list_themes_v2`、`validate_theme`、`install_theme`、`update_theme`、`rollback_theme`、
`delete_theme_v2`、`get_local_theme_settings`、`set_local_theme_settings`、`resolve_theme`、
`get_theme_asset`、`get_theme_asset_slot`。

## 6. 升级迁移（旧版本用户）

升级前版本把 `theme`（`system`/`light`/`dark`）与 `color_theme`（`tailsync`/`ocean`/
`forest`/`rose`/`high-contrast` 或 `custom:<id>`）写在 `config-v2.json` 里。新 daemon 启动时
（`crypto::Settings::load`）执行一次性迁移：

1. **识别并移除** `theme` / `color_theme` 两个键，然后**严格解析其余字段**——其他任何未知
   字段仍然导致启动失败（`deny_unknown_fields` 不变）。
2. 若 `themes-v2/local-settings.json` **不存在**：把 `theme` 写入 `appearance`、按 §1.2 表把
   `color_theme` 映射为内置 ID 写入 `activeThemeId`（`highContrast = false`）。旧
   `custom:<id>` **不自动转换**：回退 `builtin:canvas@1` 并记录明确警告；旧主题文件原样保留在
   `{数据目录}/themes/`，作者可自行按 V2 格式重新打包后导入。
3. 若 `local-settings.json` **已存在**（无论值是什么）：**绝不被旧配置覆盖**——已有 V2 选择
   是权威。文件存在但不可读时，旧字段保留在 config 中作为唯一副本，下次启动重试迁移。
4. **只有 V2 状态落盘成功后**，才原子清理 `config-v2.json` 中的旧字段（临时文件 + rename）。
   迁移是幂等的：清理完成后不再有任何迁移动作。
5. 失败路径：JSON 损坏、权限错误、未知字段、V2 写入失败——全部报错并**保留原文件**，daemon
   以非零退出码退出（见 §8）。用户身份、配对、存储路径与历史数据在任何情况下都不受影响。

## 7. 安全

- 主题包是纯声明式数据：颜色字面量/表达式、受限数值（范围随令牌而定，见 `shared/tailsync-themes/`
  的 `bounded_number`）、字符串值（字体名等仅拒绝 `url(`、`<`、`;`、`@import` 危险子串）、
  布尔值、PNG/JPEG 字节。
- 任何字段都不会进入 HTML / innerHTML / 命令行：Windows 只经 `style.setProperty()` 注入，
  macOS 只经 `Color(rgb:)` 等 SwiftUI 值类型应用；资产经 daemon 魔数 + 尺寸校验后才到达渲染层。
- zip 路径遍历（`..`、绝对路径）在解包前拒绝；解压总字节与像素总量有硬上限（§3/§4）。
- 安装目录权限收紧（Unix `0700`，Windows owner-only DACL）；写入经独占锁串行化。

## 8. 故障排查与启动诊断

- 启动失败（含旧配置解析失败、迁移写入失败）时 daemon 会：
  1. 向 stderr 与日志输出 `TailSync failed to start: <原因>`（`npm run tauri:dev` 下直接可见）；
  2. 把同一行**追加**到 `{数据目录}/startup-error.log`（带时间戳；发布版无控制台，这是可靠
     的日志位置）；
  3. 以**非零退出码**退出（Windows 与 macOS daemon 一致），供启动器/安装器/脚本检测。
- 迁移相关警告（旧 custom 回退、未知旧值、local-settings 不可读）以 `warn` 级别输出，不会
  阻止启动。
- 数据目录可通过 `TAILSYNC_DATA_DIR` 环境变量覆盖（测试与排障用）；macOS 默认为
  `~/Library/Application Support/com.tailsync.TailSync`，Windows 为平台应用数据目录。

## 9. 制作与分发建议

- 从最小包开始：只写你要改的令牌，其余继承 Canvas（`extends: "builtin:canvas@1"`）。
- 两个模式都要提供 `colors/background/canvas` 与 `colors/text/primary`；对比度由 core 校验
  但只警告不拒绝（除高对比度策略外），请以 Canvas 为可用性基准线。
- 包与资产合计建议 ≤ 2 MiB（限额见 §3/§4）；资产用 PNG（无损、尺寸小）。
- `minCoreVersion` 应写你测试过的最低版本；声明不支持的 `requiredCapabilities` 会被拒绝。
- 分享方式：把 `.tailsync-theme` 文件发给对端，对端在设置页导入。选择不随设置同步，对端需
  自行选择该主题；缺包时自动回退默认主题，不会崩溃。

### 9.1 官方增强主题集

`themes/` 收录五套内置主题的官方增强版（纸上工坊 / 流电矩阵 / 绿档账房 / 绮光绽放 /
白纸黑律），是完整的 V2 清单范例：`foundation` 承载跨模式的结构令牌（字体、形状、动效、
密度），顶层 `components` 用 `ref:` 引用颜色做跨模式组件覆盖，`light` / `dark` 只放调色板。
打包产物（`.tailsync-theme`）在 `themes/packages/`，可用
`cargo run --example theme_package_tool -- <theme.json> [输出路径]` 复核校验并重新打包；
在线效果预览见官网 `themes.html`（主题工坊页）。
