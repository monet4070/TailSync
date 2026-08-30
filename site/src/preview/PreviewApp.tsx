import { useEffect } from "react";
import {
  ArrowLeft,
  ArrowUpRight,
  Check,
  Code2,
  FileType,
  FileType2,
  FileText,
  Hash,
  Image as ImageIcon,
  KeyRound,
  Layers,
  MousePointerClick,
  ScanSearch,
  ShieldCheck,
} from "lucide-react";
import { RichPreview } from "../components/RichPreview";
import { GITHUB_URL, RELEASE_URL } from "../product";

const PREVIEW_DOC_URL = `${GITHUB_URL}/blob/main/docs/features/history-preview.md`;

// Every claim on this page traces to docs/features/history-preview.md and the
// shipped renderers (windows/src/preview, macos/swift-ui HistoryPreview*).
const formatDetails = [
  {
    key: "image",
    zh: "图片",
    en: "IMAGE",
    icon: ImageIcon,
    lead: "直接预览剪贴板位图和常见图片文件。",
    points: [
      "PNG、JPEG、GIF、WebP 文件；剪贴板位图按原始像素绘制",
      "首帧居中适配，最高 8× 缩放，视图旋转，透明棋盘格",
      "显示像素尺寸；macOS 原生解码另支持 HEIC / TIFF / BMP",
    ],
  },
  {
    key: "text",
    zh: "文本",
    en: "TEXT",
    icon: FileText,
    lead: "可选中、可搜索的纯文本。",
    points: [
      "UTF-8 解码，长行换行开关",
      "字号 12–32（默认 18，记忆上次设置），一键全部复制",
      "查找并计数，底部显示行数与字符数",
    ],
  },
  {
    key: "code",
    zh: "代码",
    en: "CODE",
    icon: Code2,
    lead: "识别为源码后显示行号和语法高亮。",
    points: [
      "内置语法高亮：Windows 覆盖 20 种语言语法，macOS 原生着色",
      "左侧行号栏，纯文本 / 源码手动切换",
      "按内容与扩展名判定，避免把普通文本误当代码",
    ],
  },
  {
    key: "markdown",
    zh: "Markdown",
    en: "MARKDOWN",
    icon: Hash,
    lead: "按排版后的内容显示，不必阅读 Markdown 源码。",
    points: [
      "标题、段落、嵌套列表 / 任务项、引用、围栏与缩进代码、分割线、竖线表格",
      "净化渲染：不自动加载远程图片、媒体、框架、脚本或样式",
      "链接仅在你点击后才交给系统浏览器",
    ],
  },
  {
    key: "pdf",
    zh: "PDF",
    en: "DOCUMENT",
    icon: FileType2,
    lead: "可以翻页、搜索和缩放，不只显示首页缩略图。",
    points: [
      "翻页、可选中文本、按需缩略图导航",
      "异步全文搜索，修饰键 + 滚轮缩放",
      "Windows 用 canvas + 文本层，macOS 用原生 PDFKit",
    ],
  },
  {
    key: "docx",
    zh: "docx",
    en: "WORD",
    icon: FileType,
    lead: "Windows 在应用内渲染，macOS 使用系统 Quick Look。",
    points: [
      "Windows 在应用内渲染：页眉、页脚、脚注、分页",
      "macOS 走系统原生 Quick Look 预览路径",
      "另：macOS 还能原生预览 PPT / PPTX 演示文稿",
    ],
  },
] as const;

const interactions = [
  { keys: ["单击"], label: "选中一条历史" },
  { keys: ["空格"], label: "打开 / 关闭选中项的预览" },
  { keys: ["双击"], label: "恢复该条到剪贴板" },
  { keys: ["右键"], label: "删除该条" },
  { keys: ["Alt", "←/→"], label: "在同一文件批次内翻看" },
  { keys: ["Ctrl/⌘", "滚轮"], label: "缩放图片 / PDF 或调整字号" },
];

const guarantees = [
  {
    icon: ShieldCheck,
    title: "负载上限 64 MiB",
    body: "预览负载在解密前后各校验一次，超限时单独提示，你仍可将其恢复到剪贴板。",
  },
  {
    icon: Check,
    title: "Markdown 与 SVG 净化",
    body: "Markdown 不会自动拉取远程资源；SVG 仅作为源码文本呈现，其标记从不被执行。",
  },
  {
    icon: KeyRound,
    title: "解密数据不落盘",
    body: "Windows 将解密字节留在内存，替换与关闭时回收 Blob URL；macOS 仅在原生 Office 预览需要时写临时文件，目录 0700、文件 0600，替换 / 关闭 / 启动时清理。",
  },
  {
    icon: Layers,
    title: "按批取用，逐条解密",
    body: "后端返回一个文件批次的有序元数据，但只解密当前项。单条加载失败不影响翻看批次内其余文件。",
  },
];

export function PreviewApp() {
  useEffect(() => {
    const elements = [...document.querySelectorAll<HTMLElement>("[data-reveal]")];
    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.setAttribute("data-visible", "true");
            observer.unobserve(entry.target);
          }
        });
      },
      { threshold: 0.08 },
    );
    elements.forEach((element) => observer.observe(element));
    return () => observer.disconnect();
  }, []);

  return (
    <div className="preview-page">
      <header className="preview-topbar">
        <a className="preview-home" href="/">
          <ArrowLeft size={15} />
          <span>TAILSYNC</span>
        </a>
        <span className="preview-topbar-tag">
          <ScanSearch size={13} />
          RICH PREVIEW / HISTORY
        </span>
        <a className="preview-source" href={PREVIEW_DOC_URL} target="_blank" rel="noreferrer">
          功能说明
          <ArrowUpRight size={13} />
        </a>
      </header>

      <main>
        <section className="preview-hero">
          <div className="preview-hero-kicker">
            <span className="preview-live-dot" />
            HISTORY PREVIEW · 06 FORMATS · LOCAL ONLY
          </div>
          <h1 className="preview-hero-title">
            不用先恢复到剪贴板，
            <br />
            <span>直接查看历史内容。</span>
          </h1>
          <p className="preview-hero-lead">
            在历史或收藏中选中一条记录，按空格即可打开独立预览窗口，原列表仍可继续使用。
            支持图片、文本、代码、Markdown、PDF 和 docx；内容在本地处理，单次预览负载上限为 64 MiB。
          </p>
        </section>

        <section className="preview-sheet-band" aria-label="六种格式一览">
          <RichPreview />
        </section>

        <section className="preview-details">
          {formatDetails.map((f, index) => {
            const Icon = f.icon;
            return (
              <article className="preview-detail" data-reveal key={f.key}>
                <div className="preview-detail-mark">
                  <span className="preview-detail-no">{String(index + 1).padStart(2, "0")}</span>
                  <Icon size={26} strokeWidth={1.4} />
                </div>
                <div className="preview-detail-body">
                  <h2>
                    {f.zh}
                    <small>{f.en}</small>
                  </h2>
                  <p className="preview-detail-lead">{f.lead}</p>
                  <ul>
                    {f.points.map((point) => (
                      <li key={point}>
                        <Check size={15} />
                        {point}
                      </li>
                    ))}
                  </ul>
                </div>
              </article>
            );
          })}
        </section>

        <section className="preview-interaction" data-reveal>
          <div className="preview-block-head">
            <span className="preview-eyebrow">
              <MousePointerClick size={15} />
              INTERACTION
            </span>
            <h2>用键盘打开和切换预览</h2>
            <p>
              预览窗口不会挡住历史列表，也不会一直置顶。关闭或最小化历史窗口时，预览窗口会一起响应；应用还会记住不同类型预览窗口的位置。标题栏会显示当前文件名和批次位置，例如
              <code>2 / 6</code>。
            </p>
          </div>
          <ul className="preview-keymap">
            {interactions.map((item) => (
              <li key={item.label}>
                <span className="preview-keymap-keys">
                  {item.keys.map((k, i) => (
                    <span key={k}>
                      {i > 0 && <i>+</i>}
                      <kbd>{k}</kbd>
                    </span>
                  ))}
                </span>
                <span className="preview-keymap-label">{item.label}</span>
              </li>
            ))}
          </ul>
        </section>

        <section className="preview-guarantees" data-reveal>
          <div className="preview-block-head">
            <span className="preview-eyebrow">
              <ShieldCheck size={15} />
              TRUST &amp; LIMITS
            </span>
            <h2>预览失败时会说明具体原因</h2>
            <p>
              超过大小限制、格式不支持、文件损坏、解密失败和临时传输故障会分别提示。
              无法预览的类型（例如 XLSX）仍会显示元数据，并保留恢复到剪贴板的入口。
            </p>
          </div>
          <div className="preview-guarantee-grid">
            {guarantees.map((g) => {
              const Icon = g.icon;
              return (
                <article className="preview-guarantee" key={g.title}>
                  <Icon size={20} />
                  <strong>{g.title}</strong>
                  <p>{g.body}</p>
                </article>
              );
            })}
          </div>
        </section>

        <section className="preview-cta" data-reveal>
          <h2>下载 TailSync，试试历史预览</h2>
          <div className="preview-cta-actions">
            <a className="button button-primary" href={RELEASE_URL} target="_blank" rel="noreferrer">
              获取 TailSync
              <ArrowUpRight size={15} />
            </a>
            <a className="button button-quiet" href="/#preview">
              回到首页
            </a>
          </div>
        </section>
      </main>

      <footer className="preview-footer">
        <a className="preview-home" href="/">
          <ArrowLeft size={14} />
          <span>TAILSYNC</span>
        </a>
        <p>历史记录预览 · 本地处理 · macOS 与 Windows</p>
        <a href={GITHUB_URL} target="_blank" rel="noreferrer">
          GitHub
          <ArrowUpRight size={13} />
        </a>
      </footer>
    </div>
  );
}
