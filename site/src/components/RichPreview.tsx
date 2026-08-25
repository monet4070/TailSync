import { FileText, Code2, Hash, Image as ImageIcon, FileType2, FileType } from "lucide-react";

// A "contact sheet" of the six richly-rendered preview formats. Every claim here
// traces to docs/features/history-preview.md — the shipped preview-window spec.
// Deliberately NOT a device→hub→device diagram and NOT the flow-workbench tabs:
// a static grid of authentic-looking micro-renders (see memory site-diagram-instruments).

const formats = [
  { key: "image", zh: "图片", en: "IMAGE", icon: ImageIcon, note: "PNG · JPEG · GIF · WebP" },
  { key: "text", zh: "文本", en: "TEXT", icon: FileText, note: "可选中 · 可搜索 · 换行" },
  { key: "code", zh: "代码", en: "CODE", icon: Code2, note: "行号 · 内置语法高亮" },
  { key: "markdown", zh: "Markdown", en: "MARKDOWN", icon: Hash, note: "标题 · 列表 · 表格" },
  { key: "pdf", zh: "PDF", en: "DOCUMENT", icon: FileType2, note: "翻页 · 搜索 · 缩略图" },
  { key: "docx", zh: "docx", en: "WORD", icon: FileType, note: "Win 本地渲染 · mac 原生" },
] as const;

export function RichPreview() {
  return (
    <div className="preview-sheet" data-reveal aria-label="六种格式的富预览示例">
      {formats.map((f) => {
        const Icon = f.icon;
        return (
          <article className={`preview-card preview-card-${f.key}`} key={f.key}>
            <header className="preview-card-head">
              <span className="preview-card-name">
                <Icon size={15} />
                {f.zh}
              </span>
              <small>{f.en}</small>
            </header>

            <div className="preview-card-art" aria-hidden="true">
              {f.key === "image" && (
                <div className="pv-image">
                  <span className="pv-image-frame" />
                  <b className="pv-image-dim">2400 × 1600</b>
                  <b className="pv-image-zoom">120%</b>
                </div>
              )}

              {f.key === "text" && (
                <div className="pv-text">
                  <i /><i /><i className="pv-text-hit" /><i /><i className="pv-text-short" />
                </div>
              )}

              {f.key === "code" && (
                <pre className="pv-code">
                  <span><i>01</i><code><em>const</em> ok = <em>await</em> sync&#40;&#41;;</code></span>
                  <span><i>02</i><code><em>if</em> &#40;ok&#41; render&#40;&#41;;</code></span>
                  <span><i>03</i><code><em>return</em> ok;</code></span>
                </pre>
              )}

              {f.key === "markdown" && (
                <div className="pv-md">
                  <span className="pv-md-h" />
                  <span className="pv-md-li"><i />文本渲染</span>
                  <span className="pv-md-li"><i />表格与任务</span>
                  <div className="pv-md-table"><span /><span /><span /><span /></div>
                </div>
              )}

              {f.key === "pdf" && (
                <div className="pv-pdf">
                  <span className="pv-pdf-page pv-pdf-p3" />
                  <span className="pv-pdf-page pv-pdf-p2" />
                  <span className="pv-pdf-page pv-pdf-p1">
                    <i /><i /><i className="pv-pdf-short" />
                  </span>
                  <b className="pv-pdf-count">3 / 12</b>
                </div>
              )}

              {f.key === "docx" && (
                <div className="pv-docx">
                  <span className="pv-docx-title" />
                  <i /><i /><i className="pv-text-short" />
                  <b className="pv-docx-tag">.docx</b>
                </div>
              )}
            </div>

            <p className="preview-card-note">{f.note}</p>
          </article>
        );
      })}
    </div>
  );
}
