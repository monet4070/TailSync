import { Check, Code2, File, ImageIcon, ScanLine } from "lucide-react";
type ClipboardKind = "text" | "image" | "file";

export function ClipboardPreview({ active }: { active: ClipboardKind }) {
  if (active === "image") {
    return (
      <div className="preview-art preview-image" aria-label="图片同步预览">
        <div className="image-scan" aria-hidden="true" />
        <div className="image-plane image-plane-a" />
        <div className="image-plane image-plane-b" />
        <div className="image-pixels" aria-hidden="true">
          {Array.from({ length: 8 }, (_, index) => <span key={`pixel-${index}`} />)}
        </div>
        <div className="image-status">
          <ScanLine size={14} />
          PIXEL MAP / LIVE
        </div>
        <div className="image-caption">
          <ImageIcon size={15} />
          screenshot-0728.png
        </div>
      </div>
    );
  }

  if (active === "file") {
    return (
      <div className="preview-art preview-file" aria-label="文件传输预览">
        <div className="file-icon-wrap">
          <File size={42} strokeWidth={1.25} />
          <span>84</span>
        </div>
        <div className="file-preview-copy">
          <strong>prototype-v2.fig</strong>
          <span>84.2 MB / 84.2 MB · 84 CHUNKS</span>
        </div>
        <div className="file-progress-track">
          <span />
        </div>
        <div className="file-chunk-rail" aria-hidden="true">
          {Array.from({ length: 7 }, (_, index) => <span key={`chunk-${index}`} />)}
        </div>
        <div className="file-metrics">
          <span>OFFSET / 88,289,075</span>
          <span>1 MiB BLOCKS</span>
        </div>
        <div className="file-check">
          <Check size={13} />
          BLAKE3 VERIFIED
        </div>
      </div>
    );
  }

  return (
    <div className="preview-art preview-text" aria-label="文本同步预览">
      <div className="text-scan" aria-hidden="true" />
      <div className="text-preview-head">
        <span><Code2 size={16} /> payload.ts</span>
        <small>LIVE WRITE</small>
      </div>
      <pre className="text-code">
        <span className="code-line"><i>01</i><code><em>const</em> clipboard = <b>await</b> sync&#40;&#123;</code></span>
        <span className="code-line"><i>02</i><code>&nbsp;&nbsp;from: <strong>&quot;macOS&quot;</strong>,</code></span>
        <span className="code-line"><i>03</i><code>&nbsp;&nbsp;to: <strong>&quot;Windows&quot;</strong>,</code></span>
        <span className="code-line"><i>04</i><code>&nbsp;&nbsp;encrypted: <em>true</em></code></span>
        <span className="code-line"><i>05</i><code>&#125;&#41;;<span className="code-caret" /></code></span>
      </pre>
      <div className="text-ack">
        <Check size={13} />
        ACK / 11:42:08
      </div>
    </div>
  );
}
