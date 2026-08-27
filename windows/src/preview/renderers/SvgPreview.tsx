import { useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, Check, Grid2x2, Image as ImageIcon, Lock, Maximize2, RotateCw, ShieldCheck, ZoomIn, ZoomOut } from "lucide-react";
import type { PreviewPayload } from "../../utils/historyPreview";
import {
  buildSVGDocument,
  SVG_PREVIEW_MAX_BYTES,
  SVG_PREVIEW_TIMEOUT_MS,
  isSVGVisualEligible,
  svgViewport,
  summarizeExternalReferences,
} from "../../utils/svgPreview";
import { useModifierWheelZoom, zoomFromWheel } from "../previewPreferences";
import { TextPreview } from "./TextPreview";

type SVGMode = "visual" | "source";
type SVGTrustGrant = { identity: string; data: Uint8Array };

export function SvgPreview({
  payload,
  t,
  onCorrupt,
}: {
  payload: PreviewPayload;
  t: (key: string) => string;
  onCorrupt: () => void;
}) {
  const decodedSource = useMemo(() => {
    try {
      return new TextDecoder("utf-8", { fatal: true }).decode(payload.data);
    } catch {
      return null;
    }
  }, [payload.data]);
  const source = decodedSource ?? "";
  const sourceBytes = payload.data.byteLength;
  const visualEligible = useMemo(() => isSVGVisualEligible(source), [source]);
  const canRenderVisual = visualEligible && sourceBytes <= SVG_PREVIEW_MAX_BYTES;
  const previewIdentity = `${payload.entry_id ?? "legacy"}\u0000${payload.name}`;
  const viewport = useMemo(() => svgViewport(source), [source]);
  const summary = useMemo(() => summarizeExternalReferences(source), [source]);
  const [mode, setMode] = useState<SVGMode>(canRenderVisual ? "visual" : "source");
  const [trustedState, setTrustedState] = useState<SVGTrustGrant | null>(null);
  // Transactional trust: the dialog allows a *pending* re-render; trust only
  // commits when the trusted document actually loads, and a failure or
  // timeout rolls it back so the UI never claims trust it did not deliver.
  // The grant carries the entry identity and data object so a replacement
  // entry cannot inherit it during the render before reset effects run.
  const [pendingTrustState, setPendingTrustState] = useState<SVGTrustGrant | null>(null);
  const [trustDialogOpen, setTrustDialogOpen] = useState(false);
  const [rendering, setRendering] = useState(canRenderVisual);
  const [failed, setFailed] = useState(!canRenderVisual);
  const [retry, setRetry] = useState(0);
  const [zoom, setZoom] = useState(1);
  const [rotation, setRotation] = useState(0);
  const [showTransparency, setShowTransparency] = useState(false);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const timeoutRef = useRef<number | null>(null);
  const dragRef = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    originX: number;
    originY: number;
  } | null>(null);

  const validSVG = decodedSource !== null && /<svg\b/i.test(source);
  const trusted = trustedState?.identity === previewIdentity && trustedState.data === payload.data;
  const pendingTrust = pendingTrustState?.identity === previewIdentity && pendingTrustState.data === payload.data;
  // A pending trust re-render loads the trusted document; trust only
  // *commits* (and renders as trusted) after that document finishes loading.
  const trustedDocument = trusted || pendingTrust;
  const canTrust = visualEligible && summary.rejectedHosts.length === 0 && summary.allowedHosts.length > 0;
  const stageRef = useModifierWheelZoom<HTMLDivElement>((deltaY) => {
    setZoom((value) => zoomFromWheel(value, deltaY, 0.1, 8));
  });
  const documentHTML = useMemo(
    () => buildSVGDocument(source, trustedDocument),
    [source, trustedDocument],
  );

  useEffect(() => {
    setMode(canRenderVisual ? "visual" : "source");
    setTrustedState(null);
    setPendingTrustState(null);
    setTrustDialogOpen(false);
    setZoom(1);
    setRotation(0);
    setShowTransparency(false);
    setOffset({ x: 0, y: 0 });
    setDragging(false);
    setRendering(canRenderVisual);
    setFailed(!canRenderVisual);
    setRetry(0);
  }, [canRenderVisual, payload.entry_id, payload.name, source, sourceBytes]);

  useEffect(() => {
    if (!validSVG) onCorrupt();
  }, [onCorrupt, validSVG]);

  useEffect(() => () => {
    if (timeoutRef.current !== null) window.clearTimeout(timeoutRef.current);
  }, []);

  useEffect(() => {
    if (mode !== "visual" || failed || !canRenderVisual) return undefined;
    if (timeoutRef.current !== null) window.clearTimeout(timeoutRef.current);
    setRendering(true);
    timeoutRef.current = window.setTimeout(() => {
      timeoutRef.current = null;
      setRendering(false);
      setFailed(true);
      setMode("source");
      // Watchdog rollback mirrors failRender: a trusted document that never
      // finished loading leaves trust uncommitted.
      setPendingTrustState(null);
      setTrustedState(null);
    }, SVG_PREVIEW_TIMEOUT_MS);
    return () => {
      if (timeoutRef.current !== null) window.clearTimeout(timeoutRef.current);
      timeoutRef.current = null;
    };
  }, [canRenderVisual, failed, mode, previewIdentity, retry, source, trustedDocument]);

  const finishRender = () => {
    if (timeoutRef.current !== null) window.clearTimeout(timeoutRef.current);
    timeoutRef.current = null;
    setRendering(false);
    setFailed(false);
    // Transactional commit: a pending trust only becomes visible trust once
    // the trusted document actually finished loading.
    if (pendingTrust) {
      setTrustedState({ identity: previewIdentity, data: payload.data });
      setPendingTrustState(null);
    }
  };

  const failRender = () => {
    if (timeoutRef.current !== null) window.clearTimeout(timeoutRef.current);
    timeoutRef.current = null;
    setRendering(false);
    setFailed(true);
    setMode("source");
    // Rollback: the trusted document never loaded, so trust stays off and
    // the previous untrusted render (or the source fallback) remains shown.
    setPendingTrustState(null);
    setTrustedState(null);
  };

  const changeZoom = (value: number) => setZoom(Math.min(8, Math.max(0.1, Number(value.toFixed(2)))));

  const retryVisual = () => {
    if (!canRenderVisual) return;
    setFailed(false);
    setMode("visual");
    setRetry((value) => value + 1);
  };

  const endDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    if (dragRef.current?.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    dragRef.current = null;
    setDragging(false);
  };

  const visual = failed ? (
    <div className="preview-svg-fallback" role="status">
      <AlertTriangle size={24} aria-hidden="true" />
      <strong>{t("history.preview.svgFallback")}</strong>
      <span>{!visualEligible ? t("history.preview.svgBlockedContent") : sourceBytes > SVG_PREVIEW_MAX_BYTES ? t("history.preview.svgTooLarge") : t("history.preview.svgRetryHint")}</span>
      {canRenderVisual && (
        <button type="button" className="preview-secondary-button" onClick={retryVisual}>
          <RotateCw size={14} aria-hidden="true" />
          {t("history.preview.retry")}
        </button>
      )}
    </div>
  ) : (
    <div
      className={showTransparency ? "preview-svg-stage" : "preview-svg-stage is-opaque"}
      ref={stageRef}
      data-dragging={dragging ? "true" : undefined}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        dragRef.current = {
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
        const drag = dragRef.current;
        if (drag?.pointerId !== event.pointerId) return;
        setOffset({
          x: drag.originX + event.clientX - drag.startX,
          y: drag.originY + event.clientY - drag.startY,
        });
      }}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
    >
      <div
        className="preview-svg-transform"
        style={{ transform: `translate(${offset.x}px, ${offset.y}px) rotate(${rotation}deg) scale(${zoom})` }}
      >
        <iframe
          key={`${previewIdentity}-${retry}-${trustedDocument ? "trusted" : "locked"}`}
          className="preview-svg-frame"
          title={t("history.preview.svgVisual")}
          sandbox=""
          referrerPolicy="no-referrer"
          srcDoc={documentHTML}
          style={{ width: `${viewport.width}px`, height: `${viewport.height}px` }}
          onLoad={finishRender}
          onError={failRender}
        />
      </div>
      {rendering && <div className="preview-svg-loading" role="status">{t("history.preview.svgRendering")}</div>}
    </div>
  );

  return (
    <section className="preview-svg-viewer" data-testid="preview-svg">
      <div className="preview-content-toolbar preview-svg-toolbar">
        {mode === "visual" && (
          <>
            <button
              type="button"
              className="preview-icon-button"
              onClick={() => changeZoom(zoom - 0.1)}
              title={t("history.preview.zoomOut")}
              aria-label={t("history.preview.zoomOut")}
            >
              <ZoomOut size={15} aria-hidden="true" />
            </button>
            <span className="preview-zoom-value">{Math.round(zoom * 100)}%</span>
            <button
              type="button"
              className="preview-icon-button"
              onClick={() => changeZoom(zoom + 0.1)}
              title={t("history.preview.zoomIn")}
              aria-label={t("history.preview.zoomIn")}
            >
              <ZoomIn size={15} aria-hidden="true" />
            </button>
            <button
              type="button"
              className={zoom === 1 && offset.x === 0 && offset.y === 0 ? "preview-icon-button is-active" : "preview-icon-button"}
              onClick={() => { setZoom(1); setOffset({ x: 0, y: 0 }); }}
              title={t("history.preview.fit")}
              aria-label={t("history.preview.fit")}
            >
              <Maximize2 size={15} aria-hidden="true" />
            </button>
            <button
              type="button"
              className="preview-icon-button"
              onClick={() => setRotation((value) => (value + 90) % 360)}
              title={t("history.preview.rotate")}
              aria-label={t("history.preview.rotate")}
            >
              <RotateCw size={15} aria-hidden="true" />
            </button>
            <button
              type="button"
              className={showTransparency ? "preview-icon-button is-active" : "preview-icon-button"}
              onClick={() => setShowTransparency((value) => !value)}
              title={t("history.preview.transparency")}
              aria-label={t("history.preview.transparency")}
            >
              <Grid2x2 size={15} aria-hidden="true" />
            </button>
            <span className="preview-image-dimensions">{Math.round(viewport.width)} × {Math.round(viewport.height)}</span>
          </>
        )}
        <span className="preview-svg-toolbar-spacer" />
        {canTrust && !trusted && !pendingTrust && (
          <button type="button" className="preview-secondary-button preview-svg-trust-button" onClick={() => setTrustDialogOpen(true)}>
            <Lock size={14} aria-hidden="true" />
            {t("history.preview.svgTrustExternal")}
          </button>
        )}
        {summary.rejectedHosts.length > 0 && !trusted && (
          <button type="button" className="preview-secondary-button preview-svg-trust-button is-blocked" onClick={() => setTrustDialogOpen(true)}>
            <ShieldCheck size={14} aria-hidden="true" />
            {t("history.preview.svgExternalBlocked")}
          </button>
        )}
        {trusted && (
          <button type="button" className="preview-secondary-button preview-svg-trust-button is-trusted" onClick={() => setTrustedState(null)}>
            <Check size={14} aria-hidden="true" />
            {t("history.preview.svgTrustedExternal")}
          </button>
        )}
        <div className="preview-segmented" role="group" aria-label={t("history.preview.svgMode")}>
          <button
            type="button"
            className={mode === "visual" ? "is-active" : ""}
            onClick={() => setMode("visual")}
            title={t("history.preview.svgVisual")}
            aria-label={t("history.preview.svgVisual")}
            aria-pressed={mode === "visual"}
          >
            <ImageIcon size={14} aria-hidden="true" />
          </button>
          <button
            type="button"
            className={mode === "source" ? "is-active" : ""}
            onClick={() => setMode("source")}
            title={t("history.preview.svgSource")}
            aria-label={t("history.preview.svgSource")}
            aria-pressed={mode === "source"}
          >
            <span aria-hidden="true">&lt;/&gt;</span>
          </button>
        </div>
      </div>

      {mode === "source" ? (
        <div className="preview-svg-source-fallback">
          {failed && (
            <div className="preview-svg-source-notice" role="status">
              <span>{!visualEligible ? t("history.preview.svgBlockedContent") : sourceBytes > SVG_PREVIEW_MAX_BYTES ? t("history.preview.svgTooLarge") : t("history.preview.svgFallback")}</span>
              {canRenderVisual && (
                <button type="button" className="preview-secondary-button" onClick={retryVisual}>
                  <RotateCw size={14} aria-hidden="true" />
                  {t("history.preview.retry")}
                </button>
              )}
            </div>
          )}
          <TextPreview data={payload.data} name={payload.name} forceCode t={t} />
        </div>
      ) : visual}

      {trustDialogOpen && (
        <div className="preview-svg-dialog-backdrop" role="presentation">
          <section className="preview-svg-dialog" role="dialog" aria-modal="true" aria-labelledby="svg-trust-title">
            <h2 id="svg-trust-title">
              {summary.rejectedHosts.length > 0
                ? t("history.preview.svgTrustRejectedTitle")
                : t("history.preview.svgTrustAlertTitle")}
            </h2>
            <p>
              {summary.rejectedHosts.length > 0
                ? t("history.preview.svgTrustRejectedMessage")
                : t("history.preview.svgTrustAlertMessage")}
            </p>
            {summary.allowedHosts.length > 0 && (
              <div className="preview-svg-host-list">
                <strong>{t("history.preview.svgAllowedHosts")}</strong>
                {summary.allowedHosts.map((host) => <code key={host}>{host}</code>)}
              </div>
            )}
            {summary.rejectedHosts.length > 0 && (
              <div className="preview-svg-host-list is-rejected">
                <strong>{t("history.preview.svgRejectedHosts")}</strong>
                {summary.rejectedHosts.map((host) => <code key={host}>{host}</code>)}
              </div>
            )}
            <div className="preview-svg-dialog-actions">
              <button type="button" className="preview-secondary-button" onClick={() => setTrustDialogOpen(false)}>
                {t("history.preview.svgTrustAlertDeny")}
              </button>
              {canTrust && (
                <button type="button" className="preview-primary-button" onClick={() => { setTrustDialogOpen(false); setPendingTrustState({ identity: previewIdentity, data: payload.data }); }}>
                  {t("history.preview.svgTrustAlertAllow")}
                </button>
              )}
            </div>
          </section>
        </div>
      )}
    </section>
  );
}
