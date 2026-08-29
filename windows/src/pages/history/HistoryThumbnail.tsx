import { useEffect, useRef, type ReactNode } from "react";
import type { ThumbnailData } from "../../hooks/useThumbnailCache";

/* ── Thumbnail canvas (renders raw RGBA data) ───────────────────── */

export function ThumbnailCanvas({ data }: { data: ThumbnailData }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const expectedLength = data.width * data.height * 4;
    if (
      data.width <= 0 ||
      data.height <= 0 ||
      !Number.isSafeInteger(expectedLength)
    ) return;
    canvas.width = data.width;
    canvas.height = data.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    let binaryStr: string;
    try {
      binaryStr = atob(data.b64);
    } catch {
      return;
    }
    if (binaryStr.length !== expectedLength) return;
    const bytes = new Uint8ClampedArray(expectedLength);
    for (let i = 0; i < expectedLength; i++) {
      bytes[i] = binaryStr.charCodeAt(i);
    }
    try {
      const imageData = new ImageData(bytes, data.width, data.height);
      ctx.putImageData(imageData, 0, 0);
    } catch {
      // Keep the placeholder when the WebView rejects malformed image data.
    }
  }, [data]);

  return <canvas ref={canvasRef} className="item-thumb" />;
}

export function LazyThumbnail({
  id,
  data,
  onVisible,
  fallback,
}: {
  id: number;
  data?: ThumbnailData;
  onVisible: (id: number) => void;
  fallback: ReactNode;
}) {
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (data) return;
    const element = rootRef.current;
    if (!element || typeof IntersectionObserver === "undefined") {
      onVisible(id);
      return;
    }
    const observer = new IntersectionObserver(
      (records) => {
        if (records.some((record) => record.isIntersecting)) {
          onVisible(id);
          observer.disconnect();
        }
      },
      { root: element.closest(".history-list"), rootMargin: "96px" },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [data, id, onVisible]);

  return (
    <div className="item-preview" ref={rootRef}>
      {data ? <ThumbnailCanvas data={data} /> : fallback}
    </div>
  );
}
