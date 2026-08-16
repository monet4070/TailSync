import { useEffect, useRef, useState } from "react";
import { ZoomIn, ZoomOut } from "lucide-react";
import { renderAsync } from "docx-preview";
import { useModifierWheelZoom, zoomFromWheel } from "../previewPreferences";

export function DocxPreview({
  data,
  t,
  onCorrupt,
}: {
  data: Uint8Array;
  t: (key: string) => string;
  onCorrupt: () => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [loading, setLoading] = useState(true);
  const [zoom, setZoom] = useState(1);
  const stageRef = useModifierWheelZoom<HTMLDivElement>((deltaY) => {
    setZoom((value) => zoomFromWheel(value, deltaY, 0.5, 2.5));
  });

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return undefined;
    let active = true;
    container.replaceChildren();
    setLoading(true);
    setZoom(1);
    void renderAsync(
      new Blob([Uint8Array.from(data)], {
        type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
      }),
      container,
      container,
      {
        className: "tailsync-docx",
        inWrapper: true,
        breakPages: true,
        ignoreWidth: false,
        ignoreHeight: false,
        renderHeaders: true,
        renderFooters: true,
        renderFootnotes: true,
        renderEndnotes: true,
        useBase64URL: true,
      },
    )
      .then(() => {
        if (active) setLoading(false);
      })
      .catch((error: unknown) => {
        console.error("DOCX preview failed:", error);
        if (active) {
          setLoading(false);
          onCorrupt();
        }
      });

    return () => {
      active = false;
      container.replaceChildren();
    };
  }, [data, onCorrupt]);

  return (
    <section className="preview-docx" data-testid="preview-docx">
      <div className="preview-content-toolbar">
        <button
          type="button"
          className="preview-icon-button"
          onClick={() => setZoom((value) => Math.max(0.5, Number((value - 0.1).toFixed(2))))}
          title={t("history.preview.zoomOut")}
          aria-label={t("history.preview.zoomOut")}
        >
          <ZoomOut size={15} aria-hidden="true" />
        </button>
        <span className="preview-zoom-value">{Math.round(zoom * 100)}%</span>
        <button
          type="button"
          className="preview-icon-button"
          onClick={() => setZoom((value) => Math.min(2.5, Number((value + 0.1).toFixed(2))))}
          title={t("history.preview.zoomIn")}
          aria-label={t("history.preview.zoomIn")}
        >
          <ZoomIn size={15} aria-hidden="true" />
        </button>
      </div>
      {loading && <div className="preview-render-loading" />}
      <div
        ref={stageRef}
        className="preview-docx-stage"
      >
        <div ref={containerRef} className="preview-docx-document" style={{ zoom }} />
      </div>
    </section>
  );
}
