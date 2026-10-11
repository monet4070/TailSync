# QQ 文稿历史缩略图修复 — 2026-10-10

用户截图对应记录 3130（本地 00:16:07，439.2 KB）。只读检查确认历史中已保存两张 PNG；实际归档和数据库隔离副本的共享分析/checked image 接口均返回首图 690×403 RGBA。旧 App 真实列表中该条显示文稿图标、空白摘要；移出筛选结果后重新载入，缩略图能出现，说明已有图片无需重新复制或补下载。

根因是临时错误的传输与消费不一致：macOS socket 用 StableErrorEnvelope 包装错误，但共享 legacy classifier 没有识别 content analysis busy，变成 internal_error；Swift request 又将标准错误转成显示文本，而 getImageData 只按原始英文 busy 字符串重试。多条缩略图争用单个分析许可时，初次失败后保留图标，当前缓存 key 不变不会再次加载。真实私有 socket 回归先失败：标准 temporarily_busy 返回后客户端直接返回 nil、没有发出第二次请求。

修复共享错误分类，将明确的分析忙碌、分析/历史图片快照失效映射为固定 temporarily_busy，不匹配带私有路径的任意后缀。Swift 保留校验后的机器错误码，缩略图按代码执行原有最多 16 次、可取消的重试。Windows/Mac Tauri 错误边界同步明确的临时失效策略。图像身份校验、严格首图顺序和永久错误处理保持有效。

QQ 保存摘要以换行开头，单行列表因此显示空白。两端列表展示 composite 摘要时去掉首尾空白，原摘要、归档、原件和剪贴板内容没有修改。

验证：Mac socket、取消、缩略图缓存、行交互及键盘回归 61/61；共享错误契约 7/7、QQ 和 history display 接口 6/6；Windows 页面与缩略图 hook 79/79，TypeScript/Vite 构建通过，11 个 canonical include 检查通过。两项实际内容重放也通过，临时探针已移出 Cargo 扫描并归档为 .rs.txt。Windows 原生未验收。

新包签名、架构、部署版本、helper 自检和 composite resolver 均 PASS。正常退出旧包，用已有 --open-settings 启动选项启动新包，再通过应用菜单打开历史。真实新包 History 窗口确认记录 3130 同时显示首图缩略图和 Screenshot_20261007… 文字摘要。切换前后全部数据库表逐列一致，5 条历史、剪贴板 snapshot/archive、配置、QQ bookmark、身份和配对文件完整保留。

诊断期间，尝试 Security.framework 的非交互 legacy keychain 查询仍触发 swift-frontend 密码弹窗；该进程已终止，未输入密码或批准访问。后续通过应用既有的 legacy security 读取路径在内存中读取已有密钥，未输出、保存或修改密钥。所有读取的记录副本与解密夹具仅保留在本地 0700 证据目录，用于隔离重放，没有修改生产记录。

包：/Users/monet/Downloads/TailSync构建/2026-10-10-qq-history-thumbnail-01/TailSync.app

源码 SHA-256：11286f36e5b88530a51d511efb8b3f7cdd817f222080c7081370d74cac039069

包 SHA-256：759c00010780e951585530c2777ea04f879c4bc3e033046269be91a0d4c05add

证据：artifacts/rich-text-repair/2026-10-10-qq-history-thumbnail/。
