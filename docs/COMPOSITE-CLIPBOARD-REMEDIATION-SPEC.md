# 《TailSync 图文混合剪贴板（Multi-Flavor）彻底根治与全链路改造方案》

> **方案目标**：终结“文字 + 图片混合内容必定丢失图片”的缺陷，将剪贴板同步能力从单一样态（Single-Flavor）升级为**操作系统级多格式复合容器（Multi-Flavor Composite Snapshot）**。  
> **适用版本**：下一代架构版本（`v2.5.0` 或重大升级）  
> **设计准则**：向后兼容 Wire v5、无感还原系统多格式剪贴板、数据库平滑演进、零脏写入与精准防回音。

---

## 一、架构重构核心：从“单样态排他”到“多格式快照容器”

### 1. 根本理念变革
操作系统剪贴板本质上是一个**键值映射容器（Map of MIME/Format -> Bytes）**。一次复制动作可以且应当同时包含：
- 纯文本：`public.utf8-plain-text` / `CF_UNICODETEXT`
- 图像数据：`public.png` / `CF_DIBV5`
- 富文本标记（可选）：`public.html` / `HTML Format`

新架构将把一次复制捕获为 `CompositeClipboardItem`，接收端在一次原子写入中**同时向系统剪贴板注册并填入所有格式**。这样用户粘贴到纯文本编辑器（如终端/记事本）能出字，粘贴到图片编辑框能出图，粘贴到富文本编辑器能同时保留图文。

### 2. 全链路数据流图谱

```
[平台抓取层: macos/windows]
  │ (一次性提取 Text + Image + HTML)
  ▼
[剪贴板监听: clipboard.rs]
  │ (废弃 If-Else 互斥 continue，构造 CompositeItem)
  ▼
[网络协议: tailsync-protocol]
  │ (引入 Command::CompositePayload / Capability 协商)
  ▼
[状态机与数据库: rust-core]
  │ (复合持久化: 主记录存摘要/文本，附件存解密图片)
  ▼
[接收端物理写入]
  │ (macOS: declareTypes / Windows: 同一会话多 Format SetClipboardData)
  ▼
[UI 展示层: SwiftUI / React]
  │ (图文复合卡片渲染)
```

---

## 二、详细落地执行方案（精确到文件与代码）

### 模块 1：线协议层扩展（Wire v5 细粒度扩展）
* **涉及文件**：`shared/tailsync-protocol/src/lib.rs`

#### 1.1 扩展命令字枚举与能力声明
新增复合有效载荷命令字，并在能力集中加入 `composite_clipboard`：

```rust
// shared/tailsync-protocol/src/lib.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum Command {
    // ... 既有命令 ...
    TextPayload = 1,
    ImagePayload = 2,
    FileChunk = 3,
    // [新增] 复合剪贴板包（同时封装文本、图片与富文本）
    CompositePayload = 10,
    // [新增] 复合包大图分片（复用既有 ImageChunk 流水线）
    CompositeImageChunk = 11,
}

// 复合有效载荷的元数据结构（以紧凑二进制/MessagePack/CBOR 序列化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositeManifest {
    pub text: Option<String>,
    pub html: Option<String>,
    pub image_meta: Option<CompositeImageMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositeImageMeta {
    pub width: u32,
    pub height: u32,
    pub format: String, // "png" 或 "zstd_rgba"
    pub total_bytes: usize,
    pub data_hash: String,
}
```

#### 1.2 细粒度能力协商绑定
* **涉及文件**：`shared/tailsync-protocol/src/capabilities.rs`

在会话握手中，只有双方协商开启 `composite_clipboard=true` 时才发送新格式；若对端未开启，发送端降级策略为：**如果既有文字又有大图片，优先发送更具信息密度的图片**，彻底终结“误将图片降为一段链接文字”的 Bug。

---

### 模块 2：平台底层多格式剪贴板读取与写入

#### 2.1 macOS 原生读取与写入改造
* **涉及文件**：`macos/src-tauri/src/clipboard_file.rs`

**修改前**：函数之间割裂，`read_clipboard_text()` 读完后系统剪贴板就结束了。  
**修改后**：新增原子快照提取函数 `read_clipboard_snapshot()`：

```rust
// macos/src-tauri/src/clipboard_file.rs

#[derive(Default, Debug, Clone)]
pub struct ClipboardSnapshot {
    pub files: Option<Vec<PathBuf>>,
    pub text: Option<String>,
    pub image: Option<ClipboardImageData>,
    pub html: Option<String>,
}

pub fn read_clipboard_snapshot() -> ClipboardSnapshot {
    use objc2_app_kit::NSPasteboard;
    let pb = NSPasteboard::generalPasteboard();
    
    // 1. 先检测并提取文件
    let files = read_clipboard_files();
    if files.is_some() {
        return ClipboardSnapshot { files, ..Default::default() };
    }

    // 2. 在同一个 changeCount 生命周期内提取文本、图片和 HTML
    let text = read_clipboard_text().ok().filter(|t| !t.is_empty());
    let image = read_clipboard_image().ok();
    let html = read_clipboard_html().ok().filter(|h| !h.is_empty());

    ClipboardSnapshot {
        files: None,
        text,
        image,
        html,
    }
}
```

**多格式写入函数**（接收端写入）：
* **涉及文件**：`macos/src-tauri/src/clipboard.rs`

```rust
// macos/src-tauri/src/clipboard.rs

pub fn write_clipboard_composite(
    text: Option<&str>,
    image: Option<&ClipboardImageData>,
    html: Option<&str>,
) -> Result<(), String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString, NSPasteboardTypeHTML, NSPasteboardTypePNG};
    use objc2_foundation::{NSData, NSString};

    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();

    let mut types = Vec::new();
    if text.is_some() { unsafe { types.push(NSPasteboardTypeString); } }
    if html.is_some() { unsafe { types.push(NSPasteboardTypeHTML); } }
    if image.is_some() { unsafe { types.push(NSPasteboardTypePNG); } }

    unsafe {
        pb.declareTypes_owner(&types, None);
        if let Some(t) = text {
            let ns_str = NSString::from_str(t);
            pb.setString_forType(&ns_str, NSPasteboardTypeString);
        }
        if let Some(h) = html {
            let ns_html = NSString::from_str(h);
            pb.setString_forType(&ns_html, NSPasteboardTypeHTML);
        }
        if let Some(img) = image {
            let png_bytes = encode_rgba_to_png(img.width, img.height, &img.rgba)?;
            let ns_data = NSData::from_vec(png_bytes);
            pb.setData_forType(&ns_data, NSPasteboardTypePNG);
        }
    }
    Ok(())
}
```

---

#### 2.2 Windows 原生 OLE 多格式读取与写入改造
* **涉及文件**：`windows/src-tauri/src/clipboard_file.rs`

**修改点**：在 `OpenClipboard` 的单次锁定期间，依次拉取所有可用格式，而不是分别调用并排他跳过：

```rust
// windows/src-tauri/src/clipboard_file.rs

#[cfg(target_os = "windows")]
pub fn read_clipboard_snapshot_windows() -> Result<ClipboardSnapshot, ClipboardFileReadError> {
    // 循环退避 OpenClipboard (复用已实现的 retryable 逻辑)
    with_open_clipboard(|| {
        let mut snapshot = ClipboardSnapshot::default();
        
        // 1. CF_HDROP (文件)
        if IsClipboardFormatAvailable(15) != 0 {
            snapshot.files = extract_hdrop_files();
            return Ok(snapshot);
        }
        
        // 2. CF_UNICODETEXT (文本)
        if IsClipboardFormatAvailable(13) != 0 {
            snapshot.text = extract_unicode_text();
        }
        
        // 3. CF_DIB / CF_DIBV5 (图片)
        if IsClipboardFormatAvailable(17) != 0 || IsClipboardFormatAvailable(8) != 0 {
            snapshot.image = extract_dib_image();
        }
        
        // 4. HTML Format
        let cf_html = RegisterClipboardFormatW(w!("HTML Format"));
        if cf_html != 0 && IsClipboardFormatAvailable(cf_html) != 0 {
            snapshot.html = extract_registered_html(cf_html);
        }
        
        Ok(snapshot)
    })
}
```

**Windows 多格式原子写入**：
在同一个 `OpenClipboard` 句柄下，**连续多次调用 `SetClipboardData`** 挂载 `CF_UNICODETEXT`、`CF_DIBV5` 及 `HTML Format`。

---

### 模块 3：监视器决策消除互斥（解决 `continue; // skip` 缺陷）
* **涉及文件**：
  - macOS: `macos/src-tauri/src/clipboard.rs`
  - Windows: `windows/src-tauri/src/clipboard.rs`

#### 3.1 彻底删除旧有阶梯互斥逻辑
将原先的：
```rust
// ── 2. Try text ────────────────────────────────────────────
match read_clipboard_text(...) {
    Ok(t) => {
        // ...
        continue; // text present → skip image check  <--- 【直接删除该 continue 逻辑】
    }
}
// ── 3. Try image ──────────────────────────────────────────
```

#### 3.2 替换为综合状态判断引擎
```rust
// clipboard.rs 核心轮询改造

let snapshot = clipboard_file::read_clipboard_snapshot();

// A. 纯文件情况：保持独立流水线不变
if let Some(files) = snapshot.files {
    dispatch_files(files).await;
    continue;
}

// B. 复合或图文混合情况：
match (snapshot.text, snapshot.image) {
    (Some(text), Some(image)) => {
        info!("Detected composite clipboard: text ({} bytes) + image ({}x{})", 
              text.len(), image.width, image.height);
        
        // 1. 触发防回音与影子检查（双哈希组合判断）
        let composite_hash = blake3::hash(format!("{}_{}", text, image.data_hash).as_bytes());
        if composite_shadow_check(&sync_engine, &composite_hash).await {
            continue;
        }

        // 2. 广播 CompositePayload
        broadcast_composite(text, image, snapshot.html, pool, settings).await;
        
        // 3. 入库持久化
        spawn_save_composite(database.clone(), text, image, snapshot.html);
    }
    (Some(text), None) => {
        // 纯文本流程（保留原有优化路径）
        dispatch_pure_text(text).await;
    }
    (None, Some(image)) => {
        // 纯图片流程（保留原有优化路径）
        dispatch_pure_image(image).await;
    }
    (None, None) => {}
}
```

---

### 模块 4：存储与数据库设计（Schema v12 平滑升级）
* **涉及文件**：
  - `shared/rust-core/src/db.rs`
  - `shared/rust-core/src/db/queries.rs`
  - `shared/rust-core/src/db/storage.rs`

#### 4.1 方案选型：保持单行还是引入关联表？
为了保持极端高性能和轻量级：
- 在 `history` 表新增类型 `'composite'`：
  ```sql
  ALTER TABLE history ADD COLUMN secondary_hash TEXT;
  ALTER TABLE history ADD COLUMN secondary_meta TEXT; -- 保存格式参数与尺寸
  ```
- **存储分配**：
  - `content_encrypted`：存加密后的**文本正文**与富文本 HTML 块；
  - `data_hash`：依然作为主搜索索引；
  - 关联图片二进制：复用既有的私有图片存储目录 `private_fs`，文件名使用 `secondary_hash.bin` 加密存储。
- **优点**：查询时可以直接进行全文检索匹配文本，渲染列表时同时拉出图片缩略图，互不争抢 I/O。

---

### 模块 5：回音抑制机制升级（Dual-Flavor Write Receipt）
* **涉及文件**：
  - `macos/src-tauri/src/clipboard_change.rs`
  - `windows/src-tauri/src/clipboard_change.rs`

#### 5.1 痛点
当应用自己把复合内容（Text + Image）写入操作系统剪贴板时，系统的 `changeCount` 发生变化。如果回执只记录单一文本哈希，监视器可能会漏判图片，误把刚刚写入的图片当作新事件再次广播出去。

#### 5.2 解决方案
写回执扩展为组合签名：
```rust
struct CompositeWriteReceipt {
    change_count: isize,
    combined_hash: String, // hash(text_hash + image_hash)
}
```
写入完成后记录 `record_composite_write_receipt(combined_hash)`；监听器监听到变化时，若计算出的双哈希匹配，则判定为自身写入，**一次性静默消耗回执**，彻底防死锁和防回音。

---

### 模块 6：客户端 UI 渲染器适配
* **涉及文件**：
  - macOS: `macos/swift-ui/Sources/TailSync/Views/HistoryRowView.swift`
  - Windows: `windows/src/components/HistoryItem.tsx`

#### 6.1 交互与视觉
- **历史记录列表**：若 `entry.type == "composite"`，采用分体式卡片：
  - 左侧/上方：图片缩略图，标有 `[图文]` 标签；
  - 右侧/下方：文字预览截断显示。
- **快捷复制行为**：
  - 用户按下 Enter 或点击该卡片时，调用 `write_clipboard_composite`，将图文一次性全量塞回本地剪贴板。

---

## 三、工程实施里程碑与退出门槛（Exit Gate）

| 阶段 | 周期 | 核心交付物 | Exit Gate 验收标准 |
|---|:---:|---|---|
| **Step 1：平台提取与读取解耦** | 1~2 天 | `read_clipboard_snapshot` 在双端实现 | 单元测试能稳定读取由第三方富文本应用（Word/浏览器）生成的复合剪贴板，文本和图片字段均不为 None。 |
| **Step 2：协议拓展与协商降级** | 1 天 | `Command::CompositePayload` 与 Capability | 当关闭新特性或对端为老版本时，自动降级为优先传输图片，绝不再丢失图像数据。 |
| **Step 3：监听器 continue 移除与流转** | 2 天 | `clipboard.rs` 调度逻辑重构 | 彻底移除 `text present -> skip image check`；双端实现复合广播与防回音抑制。 |
| **Step 4：数据库与存储演进** | 1~2 天 | SQLite 兼容写入与图片 sidecar 落盘 | 历史记录能够存入并查询 `'composite'` 类型；收藏、配额自动 Trim 测试全绿。 |
| **Step 5：UI 呈现与双机实体验收** | 2 天 | SwiftUI / React 图文卡片渲染 | 真实在 Chrome 中同时框选文字与网页图片并复制：**在另一台电脑直接粘贴到微信输入框（出图文），粘贴到记事本（出文字），两端历史均展示图文卡片**。 |
