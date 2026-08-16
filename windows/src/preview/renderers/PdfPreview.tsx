import { useEffect, useRef, useState } from "react";
import { ChevronLeft, ChevronRight, ZoomIn, ZoomOut } from "lucide-react";
import {
  GlobalWorkerOptions,
  TextLayer,
  getDocument,
  type PDFDocumentProxy,
  type RenderTask,
} from "pdfjs-dist";
import pdfWorkerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { useModifierWheelZoom, zoomFromWheel } from "../previewPreferences";

GlobalWorkerOptions.workerSrc = pdfWorkerUrl;

export function PdfPreview({
  data,
  t,
  onCorrupt,
}: {
  data: Uint8Array;
  t: (key: string) => string;
  onCorrupt: () => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const pageRef = useRef<HTMLDivElement>(null);
  const textLayerRef = useRef<HTMLDivElement>(null);
  const [document, setDocument] = useState<PDFDocumentProxy | null>(null);
  const [pageNumber, setPageNumber] = useState(1);
  const [scale, setScale] = useState(1.1);
  const [rendering, setRendering] = useState(true);
  const stageRef = useModifierWheelZoom<HTMLDivElement>((deltaY) => {
    setScale((value) => zoomFromWheel(value, deltaY, 0.4, 3));
  });

  useEffect(() => {
    let active = true;
    const task = getDocument({
      data: Uint8Array.from(data),
      isEvalSupported: false,
      useWasm: false,
    });
    setDocument(null);
    setPageNumber(1);
    setRendering(true);
    void task.promise
      .then((pdf) => {
        if (!active) {
          void pdf.destroy();
          return;
        }
        setDocument(pdf);
      })
      .catch((error: unknown) => {
        console.error("PDF preview failed:", error);
        if (active) onCorrupt();
      });
    return () => {
      active = false;
      void task.destroy();
    };
  }, [data, onCorrupt]);

  useEffect(() => {
    if (!document || !canvasRef.current || !pageRef.current || !textLayerRef.current) return undefined;
    let active = true;
    let renderTask: RenderTask | null = null;
    let textLayer: TextLayer | null = null;
    setRendering(true);
    textLayerRef.current.replaceChildren();
    void document
      .getPage(pageNumber)
      .then(async (page) => {
        if (!active || !canvasRef.current || !pageRef.current || !textLayerRef.current) return;
        const viewport = page.getViewport({ scale });
        const pixelRatio = Math.min(window.devicePixelRatio || 1, 2);
        const canvas = canvasRef.current;
        const context = canvas.getContext("2d", { alpha: false });
        if (!context) throw new Error("PDF canvas is unavailable");
        canvas.width = Math.max(1, Math.floor(viewport.width * pixelRatio));
        canvas.height = Math.max(1, Math.floor(viewport.height * pixelRatio));
        canvas.style.width = `${Math.floor(viewport.width)}px`;
        canvas.style.height = `${Math.floor(viewport.height)}px`;
        pageRef.current.style.width = `${Math.floor(viewport.width)}px`;
        pageRef.current.style.height = `${Math.floor(viewport.height)}px`;
        renderTask = page.render({
          canvas,
          canvasContext: context,
          viewport,
          transform: pixelRatio === 1 ? undefined : [pixelRatio, 0, 0, pixelRatio, 0, 0],
        });
        const textContent = await page.getTextContent({ includeMarkedContent: true });
        if (!active || !textLayerRef.current) return;
        textLayerRef.current.style.setProperty("--total-scale-factor", String(viewport.scale));
        textLayer = new TextLayer({
          textContentSource: textContent,
          container: textLayerRef.current,
          viewport,
        });
        await Promise.all([renderTask.promise, textLayer.render()]);
      })
      .then(() => {
        if (active) setRendering(false);
      })
      .catch((error: unknown) => {
        if (!active || (error instanceof Error && error.name === "RenderingCancelledException")) return;
        console.error("PDF page render failed:", error);
        onCorrupt();
      });
    return () => {
      active = false;
      renderTask?.cancel();
      textLayer?.cancel();
    };
  }, [document, onCorrupt, pageNumber, scale]);

  return (
    <section className="preview-pdf" data-testid="preview-pdf">
      <div className="preview-content-toolbar">
        <button
          type="button"
          className="preview-icon-button"
          disabled={pageNumber <= 1}
          onClick={() => setPageNumber((value) => Math.max(1, value - 1))}
          title={t("history.prev")}
        >
          <ChevronLeft size={15} aria-hidden="true" />
        </button>
        <span className="preview-page-position">
          {pageNumber} / {document?.numPages ?? "…"}
        </span>
        <button
          type="button"
          className="preview-icon-button"
          disabled={!document || pageNumber >= document.numPages}
          onClick={() => setPageNumber((value) => Math.min(document?.numPages ?? value, value + 1))}
          title={t("history.next")}
        >
          <ChevronRight size={15} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="preview-icon-button"
          onClick={() => setScale((value) => Math.max(0.4, Number((value - 0.1).toFixed(2))))}
          title={t("history.preview.zoomOut")}
        >
          <ZoomOut size={15} aria-hidden="true" />
        </button>
        <span className="preview-zoom-value">{Math.round(scale * 100)}%</span>
        <button
          type="button"
          className="preview-icon-button"
          onClick={() => setScale((value) => Math.min(3, Number((value + 0.1).toFixed(2))))}
          title={t("history.preview.zoomIn")}
        >
          <ZoomIn size={15} aria-hidden="true" />
        </button>
      </div>
      <div className="preview-pdf-body">
        {document && document.numPages > 1 && (
          <nav className="preview-pdf-pages" aria-label={t("history.preview.pages") }>
            {Array.from({ length: document.numPages }, (_, index) => index + 1).map((page) => (
              <button
                type="button"
                className={page === pageNumber ? "is-active" : ""}
                key={page}
                onClick={() => setPageNumber(page)}
              >
                <span className="preview-pdf-page-sheet" />
                <span>{page}</span>
              </button>
            ))}
          </nav>
        )}
        <div
          ref={stageRef}
          className="preview-pdf-stage"
        >
          {rendering && <div className="preview-render-loading" />}
          <div ref={pageRef} className="preview-pdf-page">
            <canvas ref={canvasRef} />
            <div ref={textLayerRef} className="textLayer preview-pdf-text-layer" />
          </div>
        </div>
      </div>
    </section>
  );
}
