# 远端截图历史摘要修复 — 2026-10-13

用户报告远端截图传到本机后显示富文本内容，并授权修复。

工作树：`/Users/monet/.codex/worktrees/remote-screenshot-summary-2026-10-13/TailSync-rc-phase35`，基于 `7759a232fc7657df01fd4e482bcb9f7f26f1c930`。此次产品修改仅在 `shared/rust-core/src/db/composite.rs`；其他工作树的未提交工作均保留，未提交、推送或发布。

## 真实记录与根因

当前用户历史 id **3212**，来源 Windows，时间 2026-10-13 17:18:21（北京时间）。修复前数据库分类为 image，显示分类版本 1003、置信度 95。旧 App 的原生历史窗口同一行显示分类“图片”，摘要却是 **Rich clipboard content**。

该条目保存一个 PNG representation，没有 NativeArchive。本地图片通过原生归档生成图片尺寸摘要；接收远端的 PortableBundle 不走此路径，也没有文字/HTML 摘要，最终使用富文本通用标题。真实图片为 **1653×448**，已存储的图片和像素预览完整。此问题属于历史摘要错误，不是截图被转换成文字或富文本。

## 修改及已有记录

在完整、仅含一个独立 PNG representation 的 manifest 上，通过已有尺寸元数据生成 `image/png · 宽×高 · 1 frame`，不为列表解码图片。新接收记录在加密 manifest 和历史身份生成前保存正确摘要。

已有记录读取时，仅在原摘要为 Rich clipboard content 且符合相同严格形状时派生正确摘要；不重写加密 manifest、图片、capture/content/dedupe identity、archive revision/digest 或网络回执。包含正文、HTML、RTF、原生归档、多图或未完成内容继续使用各自原摘要。

## 验证

- RED：修复前新增的远端接收/重开/事件重试测试及旧摘要只读投影测试均失败，实际标题 Rich clipboard content，预期图片尺寸摘要。
- GREEN：2 项回归通过；复合存储组 **78/78** 通过，覆盖图片预览、图文原件、收藏、身份、归档、重试及生命周期。
- Runtime 历史展示：默认并行运行 **3/6** 通过，另外 3 项分别遇到数据库锁、全局分析 Busy 和受竞争影响的结果；原始失败日志保留。串行独立运行 **6/6** 通过。此处不将串行通过记成默认并行全绿，也不在此修复中扩大为并发测试整治。
- Core clippy `--all-targets --features test-support -- -D warnings`、fmt、diff whitespace 和跨平台契约通过。未带 test-support 的 all-targets 运行因既有 rollback_probe 示例依赖测试支持入口失败，保留日志；未修改该示例。
- 使用实时 SQLite 一致性备份与同一原图运行生产 Core 的 probe：id 3212 显示 `image/png · 1653×448 · 1 frame`，image 预览像素与原 PNG 一致，加密 manifest、历史身份不变。probe 仅打开私有 snapshot 目录，不连接或修改运行中 API/数据库。

## 测试包与应用

测试包：`/Users/monet/Downloads/TailSync构建/2026-10-13-remote-screenshot-summary-01/TailSync.app`。

包以用户当前使用的 Finder 修复包复制生成，仅替换重新编译的 daemon 后签名。GUI 与 clipboard-helper 去签名副本 SHA-256 保持一致，保留 Finder 分流修复。旧包全文件摘要保持不变。Rust daemon 的源码与 10 月 10 日已封存的源清单逐文件对照，只有本次 composite.rs 修改不同，避免无关未提交工作混入。

release daemon 构建、deep/strict 签名验证、三个二进制 arm64/macOS 13.0 部署检查及包内 helper protocol 4 resolver 检查通过。

通过旧 App 菜单正常退出后启动新版，GUI/daemon 均运行于新包路径。切换前后同条截图的 history、parts、identity、event 行，设置、配对身份、QQ bookmark 和原生 clipboard archive 摘要均保持一致，详见 live-preservation.json。

新包启动后暂未打开窗口，界面自动化无法连接只有菜单栏的应用。已请求用户打开历史窗口；修复后的同条记录原生 GUI 核验待完成。真实双端重新发送与 Windows 原生构建未执行，不以共享源码或本机包检查代替这些证据。

证据目录：本工作树 `artifacts/remote-screenshot-summary/`。私有目录含加密数据库备份与源图归档，不提交或分享。
