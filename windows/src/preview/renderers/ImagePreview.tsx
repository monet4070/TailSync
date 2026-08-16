import { useEffect, useRef, useState } from "react";
import { Grid2x2, Maximize2, RotateCw, ZoomIn, ZoomOut } from "lucide-react";
import { getPreviewMimeType, type PreviewPayload } from "../../utils/historyPreview";
import { useModifierWheelZoom, zoomFromWheel } from "../previewPreferences";

function useEncodedImageUrl(payload: PreviewPayload): { url: string | null; failed: boolean } {
  const [state, setState] = useState<{ url: string | null; failed: boolean }>({
    url: null,
    failed: false,
  });

  useEffect(() => {
    if (payload.kind !== "file") {
      setState({ url: null, failed: false });
      return undefined;
    }
    const mime = getPreviewMimeType(payload);
    if (!mime || typeof URL.createObjectURL !== "function") {
      setState({ url: null, failed: true });
      return undefined;
    }
    let url: string;
    try {
      url = URL.createObjectURL(new Blob([Uint8Array.from(payload.data)], { type: mime }));
      setState({ url, failed: false });
    } catch {
      setState({ url: null, failed: true });
      return undefined;
    }
    return () => {
      if (typeof URL.revokeObjectURL === "function") URL.revokeObjectURL(url);
    };
  }, [payload]);

  return state;
}

function RgbaCanvas({ payload, onCorrupt }: { payload: PreviewPayload; onCorrupt: () => void }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas || payload.width === null || payload.height === null) return;
    const context = canvas.getContext("2d");
    if (!context) {
      setFailed(true);
      onCorrupt();
      return;
    }
    canvas.width = payload.width;
    canvas.height = payload.height;
    try {
      const image = context.createImageData(payload.width, payload.height);
      image.data.set(payload.data);
      context.putImageData(image, 0, 0);
      setFailed(false);
    } catch {
      setFailed(true);
      onCorrupt();
    }
  }, [onCorrupt, payload]);

  return failed ? null : <canvas ref={ref} className="preview-image-media" data-testid="preview-rgba" />;
}

export function ImagePreview({
  payload,
  t,
  onCorrupt,
}: {
  payload: PreviewPayload;
  t: (key: string) => string;
  onCorrupt: () => void;
}) {
  const [zoom, setZoom] = useState(1);
  const [rotation, setRotation] = useState(0);
  const [showTransparency, setShowTransparency] = useState(true);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    originX: number;
    originY: number;
  } | null>(null);
  const [dimensions, setDimensions] = useState<{ width: number; height: number } | null>(
    payload.width && payload.height ? { width: payload.width, height: payload.height } : null,
  );
  const encoded = useEncodedImageUrl(payload);
  const stageRef = useModifierWheelZoom<HTMLDivElement>((deltaY) => {
    setZoom((value) => zoomFromWheel(value, deltaY, 0.1, 8));
  });

  useEffect(() => {
    setZoom(1);
    setRotation(0);
    setShowTransparency(true);
    setOffset({ x: 0, y: 0 });
    setDragging(false);
    drag.current = null;
    setDimensions(payload.width && payload.height ? { width: payload.width, height: payload.height } : null);
  }, [payload]);

  useEffect(() => {
    if (encoded.failed) onCorrupt();
  }, [encoded.failed, onCorrupt]);

  const mediaStyle = {
    transform: `translate(${offset.x}px, ${offset.y}px) rotate(${rotation}deg) scale(${zoom})`,
  };
  const isFitted = zoom === 1;

  const endDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    if (drag.current?.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    drag.current = null;
    setDragging(false);
  };

  return (
    <section className="preview-image-viewer" data-testid="preview-image">
      <div className="preview-content-toolbar">
        <button
          type="button"
          className="preview-icon-button"
          onClick={() => setZoom((value) => Math.max(0.1, Number((value - 0.1).toFixed(2))))}
          title={t("history.preview.zoomOut")}
        >
          <ZoomOut size={15} aria-hidden="true" />
        </button>
        <span className="preview-zoom-value">{Math.round(zoom * 100)}%</span>
        <button
          type="button"
          className="preview-icon-button"
          onClick={() => setZoom((value) => Math.min(8, Number((value + 0.1).toFixed(2))))}
          title={t("history.preview.zoomIn")}
        >
          <ZoomIn size={15} aria-hidden="true" />
        </button>
        <button
          type="button"
          className={isFitted ? "preview-icon-button is-active" : "preview-icon-button"}
          onClick={() => {
            setZoom(1);
            setOffset({ x: 0, y: 0 });
          }}
          title={t("history.preview.fit")}
        >
          <Maximize2 size={15} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="preview-icon-button"
          onClick={() => setRotation((value) => (value + 90) % 360)}
          title={t("history.preview.rotate")}
        >
          <RotateCw size={15} aria-hidden="true" />
        </button>
        <button
          type="button"
          className={showTransparency ? "preview-icon-button is-active" : "preview-icon-button"}
          onClick={() => setShowTransparency((value) => !value)}
          title={t("history.preview.transparency")}
        >
          <Grid2x2 size={15} aria-hidden="true" />
        </button>
        {dimensions && (
          <span className="preview-image-dimensions">
            {dimensions.width} × {dimensions.height}
          </span>
        )}
      </div>
      <div
        ref={stageRef}
        className={showTransparency ? "preview-image-stage" : "preview-image-stage is-opaque"}
        data-dragging={dragging ? "true" : undefined}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          drag.current = {
            pointerId: event.pointerId,
            startX: event.clientX,
            startY: event.clientY,
            originX: offset.x,
            originY: offset.y,
          };
          setDragging(true);
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          const current = drag.current;
          if (current?.pointerId !== event.pointerId) return;
          setOffset({
            x: current.originX + event.clientX - current.startX,
            y: current.originY + event.clientY - current.startY,
          });
        }}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        <div className="preview-image-transform" style={mediaStyle}>
          {payload.kind === "image" ? (
            <RgbaCanvas payload={payload} onCorrupt={onCorrupt} />
          ) : encoded.url ? (
            <img
              className="preview-image-media"
              src={encoded.url}
              alt={payload.name}
              onLoad={(event) => {
                setDimensions({
                  width: event.currentTarget.naturalWidth,
                  height: event.currentTarget.naturalHeight,
                });
              }}
              onError={onCorrupt}
            />
          ) : null}
        </div>
      </div>
    </section>
  );
}
