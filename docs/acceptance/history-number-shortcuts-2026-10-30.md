# 历史数字取用与左右翻页：2026-10-30 实施记录

状态：**双端源码与自动化验证完成，真实桌面验收待完成**。执行依据为用户批准的[方案](../features/history-number-shortcuts.md)，实现位于当前工作树，未创建发布版本。

## 已实现的交互

- 历史首次打开及再次呼出时，当前页首条选中，键盘焦点位于列表。加载中与空结果也保留可接收 `f` 的列表焦点宿主；查询完成不会重新夺取已进入搜索的焦点。
- 只有记录行或列表宿主真正获得焦点时，无修饰键 `1–6` 才恢复对应记录，`← / →` 才翻页，`f` 才转入搜索。搜索里的数字、字母和左右光标继续原生编辑，`↓` 回到列表首条。
- 每页保留 30 条；前六个实际渲染的记录行在整条记录选中框的左上角显示编号，不覆盖图片或图标。日期、批次标题和折叠文件不占编号，展开批次后重新对应。编号固定于本页顺序，不按滚动位置重新编号。
- 数字恢复共用普通 Enter 的恢复锁、稳定 ID / identity 校验与完成路径：完整成功返回原应用，错误和部分恢复保留窗口。单个文件编号只恢复该文件。
- 左右与鼠标分页共用入口；成功选新页首条并回到顶部。保留上下跨页时向后选首条、向前选末条的现有区别。
- 翻页开始时立即阻止新的翻页和数字恢复。失败撤销待选页边，保持原页、查询与选择；Windows 回退已提交页且不自动重读，macOS 沿用成功后才提交页码的流程。目标页因删除而变空时向前查找有效页。
- `f` / 现有搜索快捷键可以取消未完成翻页，旧请求不得覆盖后续界面。中文输入法门禁、长按抑制、修饰键排除和真实控件焦点边界继续生效。
- 收藏窗口不启用这次新增的数字、裸 `f` 和左右命令，保留其搜索默认焦点。徽标、提示与用户说明均有中英文支持。

macOS 延用列表的真实 AppKit responder 和搜索 field editor；未增加全局按键监听。Windows 的 document 监听先校验实际焦点是否为行/列表宿主，再识别新命令。数据库、网络与后端 IPC 无需改动。

## 自动检查

所有命令均从仓库根目录执行。Swift 测试使用已有隔离环境，历史和恢复用例使用注入数据，没有操作真实用户剪贴板。

| 检查 | 结果 |
|---|---|
| Swift 历史键盘与批次定向测试 | 51 项，0 失败 |
| Windows 历史键盘/页面/行定向测试 | 98 项通过 |
| 完整 Swift UI 测试 | 425 项中 5 项跳过，其余通过，0 失败 |
| 完整 Windows 前端测试 | 62 个文件，516 项通过 |
| Swift release 构建 | 通过 |
| TypeScript / Vite production 构建 | 通过 |
| Windows 前端 lint | 退出码 0；保留既有 React effect/ref 等警告 |
| 跨平台契约检查 | 通过 |
| `git diff --check` | 通过 |

Swift 的 5 个跳过项目为：原生历史格式矩阵 1 项、需要活跃 GUI 测试宿主的预览键盘所有权 1 项、需显式启用的真实 WKWebView/SVG 路径 3 项。它们均不计为本次原生验收通过。Release 构建保留未改动的预览窗口代码中 `weak var initialResponder` 的既有编译警告。

检查入口：

```sh
node scripts/run-isolated-tests.mjs -- swift test --package-path macos/swift-ui --filter 'HistoryKeyboard|HistoryBatchPresentation'
npm --prefix windows test -- src/utils/historyKeyboard.test.ts src/pages/History.test.tsx src/pages/history/HistoryItem.test.tsx
node scripts/run-isolated-tests.mjs -- swift test --package-path macos/swift-ui
npm --prefix windows test
swift build -c release --package-path macos/swift-ui
npm --prefix windows run build
npm --prefix windows run lint
node windows/scripts/check_cross_platform_sync.mjs --win-root windows --mac-root macos --core-root shared/rust-core
git diff --check
```

## 本次回归覆盖

macOS 新增/调整测试位于 `HistoryKeyboardNavigationTests.swift` 和 `HistoryKeyboardIntegrationTests.swift`，Windows 位于 `historyKeyboard.test.ts` 和 `History.test.tsx`。重点覆盖：

- 真正的列表 first responder、呼出重选首条、默认第一次 ↓ 前进到第二条；搜索里的第一次 ↓ 仍选第一条。
- 列表 `f` 进入搜索；原生搜索输入 `f16` 不被数字/搜索快捷键截获。
- 第六个稳定 ID 的单次恢复、同时数字/Enter 的恢复互斥、恢复失败后的选择保留。
- 折叠文件不消耗编号，Windows 展开后编号随实际显示更新，次页从 1 开始。
- 翻页等待立即阻止旧页数字恢复和再次翻页；失败保留原页并可继续数字恢复。
- `f` 取消待完成页面、迟到结果不覆盖搜索；首次慢加载后不夺走搜索焦点。
- 收藏原行为、输入法事件、修饰键、重复键和工具栏控件边界。

现有全量用例同时回归预览跟随、上下跨页、复合恢复反馈和受保护删除。测试修改也相应替换了原“历史默认搜索焦点”的旧断言，没有把新默认列表焦点当成原流程的测试失败。

## 待完成的原生验收

| 环境/场景 | 状态 |
|---|---|
| macOS 已打包应用中的全局呼出、键盘取用、返回原应用并实际粘贴 | 待验 |
| macOS 真实中文输入法候选选择、外接键盘和字号/高对比视觉走查 | 待验 |
| Windows WebView2 中的真实前台 HWND 返回、输入法及系统剪贴板 | 待验；本机为 macOS |
| Windows 小键盘 Num Lock 状态、主题与缩放视觉走查 | 待验 |

自动化使用真实隐藏 AppKit 控件/注入依赖，以及 jsdom/Tauri 桩；不能以此宣称操作系统前台切换、真实输入法或实际粘贴已通过。上述源码验证阶段未替换或重启已安装应用，也未推送、打包发布或提交当前工作树中的其他改动。

## 后续安装：用户授权替换应用程序中的 TailSync

已按用户后续要求替换并启动 `/Applications/TailSync.app`。重新确认当前 Swift release 构建后，将新 UI 装入完整应用包，沿用原安装包中已启用复合剪贴板的守护进程与 helper。完成 ad-hoc runtime 签名，验证全部 Mach-O 为 arm64 / macOS 13.0，生产 helper protocol 4 自检健康；安装后的全部文件哈希与已验证候选一致。

已确认 UI 和 `tailsyncd` 均从 `/Applications/TailSync.app/Contents/MacOS/` 运行，后台监听 TCP 19890。安装成功不替代上表中的真实输入法、跨应用粘贴和视觉验收。

旧包及安装核验记录保存在本机：

- [旧应用备份](../../artifacts/history-shortcuts-install-2026-10-30-_okk3rin/previous/TailSync.app)
- [安装记录与二进制摘要](../../artifacts/history-shortcuts-install-2026-10-30-_okk3rin/installation.json)

## 根据截图反馈调整编号样式

按用户要求移除独立序号列，编号改为缩略图左上角的 14 pt 实心小角标，文本条目叠在类型图标左上角；边框和背景沿用当前主题。六条之后不保留编号空列，正文恢复原有对齐。角标放在图像边界内，避免宽图的角标被历史行裁剪。

本轮 Swift 键盘、行交互和布局相关 69 项测试通过；Windows 历史页面和行相关 95 项测试及 production 构建通过。使用真实 `HistoryRow`、合成宽/长图片与文本图标生成浅色、深色截图并检查，临时渲染入口已移除。

已完成本轮 Swift release 构建，重新签名和核验后替换 `/Applications/TailSync.app`，并确认 UI 与守护进程均从该位置启动。最新[安装记录](../../artifacts/history-shortcut-corner-2026-10-30-dwe2klh3/installation.json)、[浅色预览](../../artifacts/history-shortcut-corner-2026-10-30-dwe2klh3/preview-light.png)与[深色预览](../../artifacts/history-shortcut-corner-2026-10-30-dwe2klh3/preview-dark.png)保存在本机；本节安装记录取代上一节作为当前安装包摘要。

## 后续修正：编号不得遮挡图标

按用户进一步反馈，将编号从图片/图标内部移出，改为在其上方正常排列，留出 4 pt 间距；SwiftUI 使用垂直布局，Windows 使用纵向 flex，不再通过 overlay 或绝对定位放置编号。无需增加横向序号列，图标与缩略图均完整可见。已复核真实 HistoryRow 的浅色、深色合成截图；Swift 布局/行交互 24 项、Windows 行组件 18 项及 Windows production 构建通过，Swift release 构建通过。

已替换并重启 `/Applications/TailSync.app`，校验签名、helper 健康和安装文件摘要，并确认新 UI 与守护进程运行。本次[安装记录](../../artifacts/history-shortcut-clearance-2026-10-30-spippy9m/installation.json)、[浅色预览](../../artifacts/history-shortcut-clearance-2026-10-30-spippy9m/preview-light.png)和[深色预览](../../artifacts/history-shortcut-clearance-2026-10-30-spippy9m/preview-dark.png)取代上节作为当前安装包与样式记录。

## 用户最终指定：整条记录选中框左上角

编号现以记录边框为定位基准，14 pt 徽标中心固定于框的左上角；未选中行保持相同的行角位置。外围预留 8 pt，避免首行或左侧裁剪；图片和图标另留顶部间距，编号不再依附其布局。已复核实际 HistoryRow 渲染，Swift 布局/行交互 24 项及临时渲染 1 项通过，Windows 行组件 18 项及 production 构建通过，Swift release 构建通过。

已重新签名、替换并启动 `/Applications/TailSync.app`，确认新 UI 与守护进程运行。本次[安装记录](../../artifacts/history-shortcut-rowcorner-2026-10-30-q42iluxq/installation.json)、[浅色预览](../../artifacts/history-shortcut-rowcorner-2026-10-30-q42iluxq/preview-light.png)与[深色预览](../../artifacts/history-shortcut-rowcorner-2026-10-30-q42iluxq/preview-dark.png)为当前安装包及样式记录，取代前述位置方案。
