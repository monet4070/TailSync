# TailSync 主题指南（内置主题 + 运行时自定义主题）

> 适用版本：产品 2.1.0（含运行时自定义主题支持）。本文档描述 TailSync 的主题系统架构、主题令牌契约（单一事实来源），以及第三方分发自定义主题的完整方式。

## 0. 先读这一段：主题的生效方式

**TailSync 已支持运行时主题。** 主题来源有两类：

1. **5 个内置主题**（`tailsync` / `ocean` / `forest` / `rose` / `high-contrast`）——编译期定义，双端各一份（Windows = CSS 自定义属性块 + TypeScript 常量表；macOS = Swift 枚举 + 定义表），不可被用户文件覆盖。
2. **运行时自定义主题**——用户（或第三方作者）把 JSON 主题文件放进 `{数据目录}/themes/`，应用在设置页展示并应用，与内置主题并列。两端都从共享 core（`shared/rust-core/src/themes.rs`）的同一份校验规则加载：

- Windows：daemon `list_themes` 命令返回校验后的主题清单，前端在 `.app` 元素上以 `style.setProperty()` 注入当前亮暗模式对应的全套 CSS 变量；
- macOS：SwiftUI 把 daemon 清单解码为 `TailSyncThemeDefinition`，经 `TailSyncThemeSelection` 应用到环境值。

**同步语义（重要）**：主题*选择*（`color_theme` 设置值，形如 `custom:studio`）随设置跨设备同步；主题*文件*本身是本地的、不随设置同步。对端缺文件时的行为就是回退：应用显示默认主题，设置页标注"此主题文件不可用"，不会崩溃。分享方式 = 用户经现有文件传输把主题文件发给对端后导入（见 §3.3）。

## 1. 主题系统架构

### 1.1 两套并行实现 + 运行时覆盖层

| | Windows | macOS |
|---|---|---|
| 渲染技术 | React + Tauri WebView | SwiftUI 原生 |
| 内置主题载体 | CSS 自定义属性（`shared/art-direction.css` 的 `.app.light/dark.theme-{name}` 块 + `windows/src/styles/theme.css` 基础层） | `TailSyncColorTheme` 枚举 + `definition` 查表（`macos/swift-ui/Sources/TailSync/Models/Theme.swift`） |
| 自定义主题载体 | `.app` 元素内联 CSS 变量（`useTheme` 注入，`windows/src/utils/themeCss.ts` 纯函数生成） | `TailSyncThemeDefinition`（daemon 清单 JSON 解码）+ `TailSyncThemeSelection` 解析 |
| 挂载方式 | 根元素 class：`app ${light\|dark} theme-${name}`（`Settings.tsx` 的 `appClassName`；自定义主题 class 回退默认） | 环境值注入：`.tailSyncThemed()` 修饰器（`tailSyncSelection` / `tailSyncPalette`） |
| 亮暗切换 | `light` / `dark` / `system`（`prefers-color-scheme` 媒体查询） | SwiftUI `colorScheme` 环境自动跟随系统 + 本地偏好 |

### 1.2 令牌分层（CSS 侧）

```
windows/src/styles/theme.css        基础层：:root,.app.light 与 .app.dark 定义全量令牌默认值
shared/art-direction.css            主题层：每个内置主题三个块覆盖令牌（见 §2.1）
.app 内联 style                     运行时层：自定义主题经 style.setProperty() 注入
```

`art-direction.css` 位于 `shared/`，是设计系统的单一事实来源；修改它只需改一份，但必须在 Windows 前端实测（macOS 的 Tauri 前端是占位 stub，实际 UI 走 SwiftUI）。

### 1.3 主题注册与持久化

| 环节 | Windows | macOS |
|---|---|---|
| 内置主题清单 | `windows/src/hooks/useTheme.ts` 的 `COLOR_THEMES` 常量 | `TailSyncColorTheme` 枚举 case |
| 自定义主题清单 | `useTheme` 经 `list_themes` 命令加载（含错误标记项） | `Loc.customThemes` 经 `ApiClient.listThemes()` 加载 |
| 持久化 | localStorage：`tailsync-color-theme`（配色，值可为 `custom:{id}`）、`tailsync-theme`（亮暗） | UserDefaults（经 `Loc.swift` 的 `colorTheme`） |
| 多窗口同步 | `storage` 事件（设置窗口改主题 → 历史窗口即时生效） | 单进程，`@Published` 自动传播 |
| 未知值兜底 | `readStoredColorTheme` 保持 `custom:` 原值不提前回退；应用时 `resolveColorTheme` 回退 `tailsync` | `Loc.normalizeColorTheme` 保持 `custom:` 原值；`TailSyncThemeSelection` 应用时回退 `.tailsync` |

**兼容性注意**：两端对未知主题名（含对端缺文件的 `custom:` 值）都回退到默认主题，所以旧版本客户端打开新主题的同步设置不会崩溃——但会显示成默认主题的样子。

## 2. 主题令牌契约

### 2.1 CSS 侧：内置主题每主题三个块

以现有主题为模板（`shared/art-direction.css` 中 `theme-tailsync` / `theme-ocean` 等）：

```css
/* 块 1：浅色配色（必需） */
.app.light.theme-mytheme { /* 24 个颜色令牌，见 §2.2 */ }

/* 块 2：深色配色（必需） */
.app.dark.theme-mytheme { /* 同样 24 个 */ }

/* 块 3：跨亮暗共享（可选，字体与结构微调） */
.app.theme-mytheme {
  --art-display: ...;   /* 标题展示字体栈 */
  --art-reading: ...;   /* 正文字体栈 */
  /* 可选：结构性覆盖，参考 .app.theme-high-contrast
     （border-radius、box-shadow: none）*/
}

/* 块 4（可选）：logo 着色，参考 .app.theme-ocean .theme-logo */
.app.theme-mytheme .theme-logo { ... }
```

自定义主题**不写 CSS 块**：令牌由 core 校验后经 CSS 变量注入（§3）。

### 2.2 颜色令牌总表与 Swift 对照

这是主题作者必须维护的核心契约。**同一行的两列必须视觉一致**（同一个颜色值）。

| 语义 | CSS 变量（`theme-*` 块内覆盖 / 自定义主题注入） | Swift `TailSyncThemePalette` 字段 |
|---|---|---|
| 品牌色 | `--brand` | `accent` |
| 品牌色上的文字 | `--brand-text` | `accentContrast` |
| 品牌悬停 | `--brand-hover` | —（仅 CSS，交互态） |
| 品牌淡底 | `--brand-soft` | —（仅 CSS；Swift 用 `accent` + 自定透明度） |
| 窗口底色 | `--bg-window` | `window` |
| 卡片面 | `--bg-card` | `surface` |
| 输入框底色 | `--bg-input` | `softSurface`（配 `softSurfaceOpacity`） |
| 悬停底色 | `--bg-hover` | —（仅 CSS） |
| 按下底色 | `--bg-active` | —（仅 CSS） |
| 浮起面 | `--bg-raised` | `raised` |
| Toast 底 / 文字 | `--bg-toast` / `--text-toast` | `toast` / `toastText`（配 `toastOpacity`） |
| 主 / 次 / 三级文字 | `--text-primary` / `--text-secondary` / `--text-tertiary` | `textPrimary` / `textSecondary` / `textTertiary`（各配 Opacity 字段） |
| 边框 / 强边框 / 分隔线 | `--border` / `--border-strong` / `--divider` | `border` / — / `divider`（border 配 `borderOpacity`） |
| 成功色 | `--green` | `positive` |
| 警告色 | `--orange` | `warning` |
| 强调色（第三语义色） | `--purple` | —（仅 CSS） |
| 各语义色淡底 | `--green-soft` / `--orange-soft` / `--purple-soft` | —（仅 CSS） |

**本表共 24 个颜色令牌**（Windows 侧每个主题块正好 24 个变量；macOS 侧对应 15 个颜色字段 + 7 个透明度字段）。

**透明度写法差异**：CSS 把透明度直接烘焙进颜色值（如 `--bg-input: rgba(26, 25, 22, 0.045)`），Swift 拆成"十六进制颜色 + Opacity 字段"（`softSurface: 0x1A1916, softSurfaceOpacity: 0.045`）。换算时保持数值一致。自定义主题文件中每个颜色令牌是 `#rrggbb` 十六进制 + 可选 `opacity ∈ [0,1]`；带 opacity 的令牌在 Windows 侧经 `rgba(r, g, b, opacity)` 合成，Swift 侧拆成颜色 + 透明度字段（§3.2）。

**仅 Swift 侧的主题维度**（每个主题必须提供）：

- `metrics`：`cardRadius` / `controlRadius` / `rowPadding` / `shadowRadius`（形状与密度）
- `typography`：节标题字号、是否大写、搜索框字号、正文内容字号
- `displayFontName` / `readingFontName`：字体名（与 CSS 块 3 的 `--art-display` / `--art-reading` 对应）
- `symbolName`：设置页 SF Symbol 图标（仅内置主题；自定义主题用通用图标）

CSS 侧没有 metrics/typography 枚举，等价调整通过块 3 的结构覆盖和字体变量实现（目前仅 high-contrast 用了结构覆盖；tailsync 用了展示字体）。**新增内置主题若调整形状/排版，需要自己评估在 CSS 里补充对应规则的位置。**

## 3. 自定义主题文件规范（正式）

### 3.1 目录与生命周期

- **位置**：`{数据目录}/themes/*.json`。目录由应用创建并收紧权限（Unix `0700`，Windows owner-only DACL）。Windows 侧 `{数据目录}` 是平台应用数据目录（可用 `TAILSYNC_DATA_DIR` 覆盖）；macOS 为 `~/Library/Application Support/com.tailsync.TailSync`。
- **扫描**：只读取目录内直接子文件、扩展名为 `.json` 的文件；子目录与其他扩展名一律忽略。超过 32 个主题文件时，超出部分忽略并上报错误。
- **导入**：设置页"导入主题"（文件选择器，过滤 `.json`）→ daemon 校验 → **复制**到 themes 目录（文件名固定为 `{id}.json`，不是引用）。同 ID 已存在、ID 为内置保留名、目录已满或校验失败时返回具体原因。源文件删除不影响已导入主题。
- **删除**：仅允许删除 themes 目录内、ID 非内置保留名的 `{id}.json` 文件。
- **打开文件夹**：设置页按钮在平台文件管理器中打开 themes 目录（路径来自服务端常量，不经前端参数）。
- **限额**：单文件 ≤ 64 KiB；目录内 ≤ 32 个主题；名称值 ≤ 64 字符。

### 3.2 文件格式

```jsonc
{
  "format": 1,                              // 必填，仅接受 1
  "id": "studio",                           // ^[a-z0-9][a-z0-9-]{0,31}$；不可用 5 个内置 ID
  "name": { "en": "Studio", "zh-CN": "工作室" },  // 至少含 "en"；键 ^[a-zA-Z-]{2,10}$；值 ≤64 字符
  "palette": {
    "light": { /* 24 个颜色令牌，见下 */ },
    "dark":  { /* 24 个颜色令牌，必填，亮暗必须成对 */ }
  },
  "metrics": { "cardRadius": 10, "controlRadius": 9, "rowPadding": 13, "shadowRadius": 8 },
  "typography": {
    "sectionTitleSize": 25,
    "uppercasesSectionTitles": false,
    "searchSize": 18,
    "searchUsesDisplayFont": true,
    "historyContentSize": 15
  },
  "fonts": { "display": "Songti SC, PingFang SC", "reading": null },  // null = 系统字体；逗号分隔 = 候选列表
  "structural": { "borderRadius": 10, "shadow": false }  // 可选；V1 仅这两个键
}
```

**palette 的 24 个颜色令牌**（字段名 = §2.2 CSS 变量名去掉 `--` 的驼峰形式，与内置块一一对应）：

`brand`、`brandHover`、`brandSoft`、`brandText`、`bgWindow`、`bgCard`、`bgInput`、`bgHover`、`bgActive`、`bgRaised`、`bgToast`、`textPrimary`、`textSecondary`、`textTertiary`、`textToast`、`border`、`borderStrong`、`divider`、`green`、`greenSoft`、`orange`、`orangeSoft`、`purple`、`purpleSoft`

每个令牌值二选一：

```jsonc
"brand": "#d5684b",                                // 裸十六进制串 ^#[0-9a-fA-F]{6}$
"brandSoft": { "hex": "#d5684b", "opacity": 0.11 } // 或对象：hex + 可选 opacity ∈ [0,1]
```

**校验规则**（`shared/rust-core/src/themes.rs` 是唯一实现；不满足即文件被跳过并在设置页显示错误卡片）：

- `format` 必须为 1；顶层未知字段（如 Swift 风格字段名 `accent`）直接报错，便于作者发现笔误；
- `id` 匹配 `^[a-z0-9][a-z0-9-]{0,31}$`，5 个内置 ID 保留（冲突即错误）；
- 颜色全部为 `^#[0-9a-fA-F]{6}$`，opacity ∈ [0,1]；
- metrics 范围：`cardRadius`/`controlRadius` ∈ [0,24]、`rowPadding` ∈ [4,32]、`shadowRadius` ∈ [0,32]；
- typography 字号 ∈ [9,32]；布尔字段为 `true`/`false`；
- fonts 名仅允许 `[A-Za-z0-9 .,'-]` 且 ≤ 64 字符（按字符计），`null`/缺省 = 系统字体；值可以是**逗号分隔的候选字体列表**（如 `"Songti SC, PingFang SC"`）：渲染端按顺序取第一个本机已安装的字体，全部缺失则回退系统字体（macOS 侧在 `FontCandidates` 中解析并探测；Windows 侧原样注入 CSS `font-family`，由 CSS 原生回退语义处理——两端行为一致，见 §3.6）；
- structural 仅 `borderRadius`（∈ [0,64]，注入 `border-radius` 像素值）与 `shadow: false`（注入 `box-shadow: none`，对应 high-contrast 的结构语义）两个键生效；其余键忽略并上报 warning；
- 文件 ≤ 64 KiB；目录内主题数 ≤ 32。

### 3.3 分享与同步语义

- 主题**选择**（`custom:{id}`）存进设置 `color_theme` 字段，随设置跨设备同步；
- 主题**文件**不参与同步，是本地的；
- 对端缺文件时：应用回退默认主题（两端一致），**存储值不被改写**；Windows 设置页显示本地化警告横幅并标出缺失的 ID（`settings.colorThemeMissing`，en/zh-CN）；对端补上同名 ID 文件后自动恢复；
- 分享方式：用户经现有文件传输把 `.json` 主题文件发给对端，对端在设置页导入。

### 3.4 安全

- 主题文件是纯声明式数据：颜色（hex + opacity）、字号范围、字体名（受限字符集）、布尔值、内嵌 base64 图片字节（PNG/JPEG 白名单，魔数与尺寸在服务端头解析层校验后才可达渲染层）；
- 任何字段都**不会**进入 HTML / innerHTML / 任意 CSS 文本拼接 / 命令行参数：Windows 侧只经 `style.setProperty()` 注入校验后的值，macOS 侧只经 `Color(rgb:)` 等 SwiftUI 值类型应用；"打开主题文件夹"的路径来自服务端常量并以单参数方式启动 `explorer`/`open`（不经 shell）。

### 3.5 背景图（可选）

主题可在窗口层叠加一张背景图 + 纯色 scrim 遮罩（卡片/输入/浮层面保持纯色令牌，可读性优先）。格式向后兼容：无 `background` 字段的旧主题行为完全不变。

```jsonc
"background": {
  "light": {
    "image": { "mimeType": "image/png", "dataB64": "<base64>" },
    "scrim": { "hex": "#0f1526", "opacity": 0.82 }
  },
  "dark": { /* 可独立存在，允许一侧有图一侧无图 */ }
}
```

**字段与规则**（`shared/rust-core/src/themes.rs` 是唯一实现）：

- `image.mimeType` 仅接受 `image/png` 与 `image/jpeg`（**SVG/GIF/WebP 永不接受**——脚本与动图解码面）；`image.dataB64` 为严格标准 base64。
- `scrim` 与 `image` 必须成对出现：有图无 scrim 或 scrim 无图都报错；scrim 为 `#rrggbb` hex + opacity ∈ [0.5, 0.95]（低于 0.5 文本对比无法保证；缺失即拒绝）。
- 限额：单图解码后 ≤ 3 MiB；宽高 ∈ [1, 6000] 且宽×高 ≤ 2400 万像素（"小文件大尺寸"炸弹在头解析层拒绝，不解码像素）；含图主题文件 ≤ 4 MiB、无图主题仍 ≤ 64 KiB。
- 只允许**内嵌 base64**：主题 JSON 中不得出现任何 URL/路径形式的图片引用。
- 渲染：图片 `cover` 居中铺满窗口层，scrim 以同色渐变层叠加其上；亮暗模式各自独立。
- 清单瘦身：`list_themes` 只返回元数据（`hasImage`/`scrim`/`mimeType`），图片字节经 `get_theme_background(id, mode)` 按需获取——一次 IPC 不会拉几十 MB。

**制作建议**：图片 ≤ 2 MiB；深色 scrim 配深色主题（浅色主题建议浅色底图 + 深 scrim 需谨慎核对对比度）；参考示例 `docs/examples/spider-man-city.json`。

### 3.6 字体候选与 metrics/typography 的 Windows 映射（R006/R007）

**字体候选列表**（R006）：`fonts.display` / `fonts.reading` 可写逗号分隔的多个候选，两端按同一语义取第一个可用字体：

- macOS：`FontCandidates.parse` 按 `,` 拆分并逐项去首尾空白、丢弃空项；`FontCandidates.firstAvailable` 用 `NSFont(name:size:)` 探测，取第一个本机已安装的候选；全部缺失 → 系统字体回退（与 `null` 相同）。`"Avenir Next, Songti SC ,"` → 候选 `["Avenir Next", "Songti SC"]`。
- Windows：原样注入 `--art-display` / `--art-reading`（CSS `font-family` 原生支持逗号回退），行为一致，无需额外解析。
- 校验不变：整串仍限 `[A-Za-z0-9 .,'-]`、≤ 64 字符（按 Unicode 字符计）。

**metrics/typography 映射**（R007，Windows 侧经 `style.setProperty` 注入、切回内置主题时全部清除；字段 → CSS 变量一一对应，值为原始数值）：

| 主题字段 | CSS 变量 | 值 |
|---|---|---|
| `metrics.controlRadius` | `--radius-sm` | `${controlRadius}px` |
| `metrics.cardRadius` | `--radius-md`、`--window-radius` | `${cardRadius}px` |
| `metrics.rowPadding` | `--history-row-padding-y`、`--setting-row-padding-y` | `${rowPadding}px` |
| `metrics.shadowRadius` | `--shadow-md` | `0 4px ${shadowRadius}px rgba(0,0,0,0.08)`；为 0 时 `none` |
| `typography.sectionTitleSize` | `--font-size-section` | `${sectionTitleSize}px` |
| `typography.historyContentSize` | `--font-size-content` | `${historyContentSize}px` |
| `typography.searchSize` | `--search-font-size` | `${searchSize}px` |
| `typography.searchUsesDisplayFont` | `--search-font-family` | `var(--font-display)` / `var(--font-ui)` |
| `typography.uppercasesSectionTitles` | `--section-title-transform` | `uppercase` / `none` |

消费点（Windows）：搜索框 `.search-bar input`、历史分组标题 `.date-header`、设置分组标题 `.setting-group-header h3` 均已改为读取上述变量；主题未注入时使用基础层默认值，内置主题行为完全不变。macOS 侧这些字段直接驱动 SwiftUI（`RoundedRectangle` 圆角、`.padding(.vertical, rowPadding)`、`.textCase`、字号等），无映射表。

## 4. 设计规范

1. **浅色与深色必须成对定义**，缺一个该模式会回落到基础层令牌，视觉断裂。
2. **对比度下限**：正文文字（`text-primary`/`text-secondary`）对窗口底色 ≥ 4.5:1（WCAG AA）；`brand` 对 `brand-text`、大号文字与 UI 边框 ≥ 3:1。以默认 tailsync 主题为可用性基准线，不要明显低于它。
3. **语义色不可互换**：`green` 表成功/在线、`orange` 表警告、`purple` 表第三强调。high-contrast 主题刻意把它们去饱和成灰阶（功能靠形状而非颜色承载）——除非你在做无障碍主题，否则保持三色可区分。
4. **Toast 是反色面**：浅色主题用深 Toast，深色主题用浅 Toast（所有现有主题如此）。
5. **字体栈必须带跨平台回退**：Windows webview 与 macOS 并存，栈内应同时覆盖两平台字体（如 `"Bahnschrift", "Avenir Next", ...`、`"Songti SC", "SimSun", ...` 的写法）。中文场景确认中文字体在栈内。自定义主题的 `fonts.display`/`fonts.reading` 缺字体时两端自然回退系统字体。
6. **令牌纪律**：只覆盖 §2.2 表内的令牌。结构性调整（圆角、阴影）仅在你明确设计意图时加入块 3（内置）或 `structural`（自定义），并先看 high-contrast 的先例。
7. **主题 ID 不可复用既有 ID**，且一旦发布即成公共契约（同步设置会携带它跨设备），重命名等于废弃一个主题。

## 5. 现有主题一览（命名与气质参考）

| ID | 展示名 | 气质 |
|---|---|---|
| `tailsync` | Canvas | 默认主题；暖纸底、衬线展示字体、大标题搜索 |
| `ocean` | Flux | 青色系、几何无衬线、紧凑圆角 |
| `forest` | Ledger | 绿色系、书籍式衬线、小圆角、大写节标题 |
| `rose` | Aura | 玫红系、圆体（rounded）、最大圆角 |
| `high-contrast` | Mono | 无障碍主题；灰阶语义色、零圆角、等宽展示字体 |

## 6. 新增一个内置主题（编译期流程，仅供维护者）

以内置主题 ID `mytheme` 为例（ID 用小写连字符，与 `COLOR_THEMES`、class 后缀、i18n key、Swift rawValue 四处一致）：

1. **`shared/art-direction.css`**：按 §2.1 添加三个块（浅色 24 令牌、深色 24 令牌、共享字体块）。以最接近你想要气质的现有主题为底稿复制修改。
2. **`windows/src/hooks/useTheme.ts`**：`COLOR_THEMES` 数组追加 `"mytheme"`。
3. **`windows/src/i18n/en.json` 与 `zh-CN.json`**：添加 `settings.colorTheme.mytheme`（展示名，如 `"Studio"` / `"工作室"`）。
4. **`windows/src/styles/settings.css`**：为主题卡片预览添加字体样式块 `.palette-card-preview.mytheme .palette-font-sample { font-family: ... }`；卡片本身由 `COLOR_THEMES` 自动生成，无需改 `Settings.tsx`。
5. **`macos/swift-ui/Sources/TailSync/Models/Theme.swift`**：枚举加 case、`symbolName` 加图标、`definition` 表加条目（palette×2/metrics/typography/fonts，值按 §2.2 对照表与 CSS 块逐一对齐）。
6. **`macos/swift-ui/Sources/TailSync/Services/Loc.swift`**：en / zh 两张表加 `settings.colorTheme.mytheme`，展示名与 Windows i18n 相同。
7. 验证：`npm test` + `npm run build`（windows/）；`swift build`（macos/swift-ui）；双端四个象限截图 + 跟随系统 + 双窗口联动 + 对照表逐行核对 + 对比度抽检。
