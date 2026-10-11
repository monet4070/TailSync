import { lazy, Suspense, useMemo } from "react";
import { useTheme } from "../hooks/useTheme";
import { previewRendererFor, type PreviewPayload } from "../utils/historyPreview";
import type { PreparedDocumentAssets } from "./documentAssets";
import { PreviewErrorBoundary } from "./ErrorBoundary";

// Keep PDF, DOCX and syntax-highlighting code out of the history window's
// initial bundle. The renderer is loaded only after the user opens that type.
const ImagePreview = lazy(async () => import("./renderers/ImagePreview").then((module) => ({ default: module.ImagePreview })));
const SvgPreview = lazy(async () => import("./renderers/SvgPreview").then((module) => ({ default: module.SvgPreview })));
const MarkdownPreview = lazy(async () => import("./renderers/MarkdownPreview").then((module) => ({ default: module.MarkdownPreview })));
const PdfPreview = lazy(async () => import("./renderers/PdfPreview").then((module) => ({ default: module.PdfPreview })));
const DocxPreview = lazy(async () => import("./renderers/DocxPreview").then((module) => ({ default: module.DocxPreview })));
const TextPreview = lazy(async () => import("./renderers/TextPreview").then((module) => ({ default: module.TextPreview })));
const DocumentPreview = lazy(async () => import("./renderers/DocumentPreview").then((module) => ({ default: module.DocumentPreview })));

export function PreviewContent({
  payload = null,
  documentPreview = null,
  t,
  onCorrupt,
  resetKey,
  onRetry,
}: {
  payload?: PreviewPayload | null;
  documentPreview?: PreparedDocumentAssets | null;
  t: (key: string) => string;
  onCorrupt: () => void;
  resetKey?: string | number;
  onRetry?: () => void;
}) {
  const { themeAssetSlots } = useTheme();
  const renderer = useMemo(() => (payload ? previewRendererFor(payload) : null), [payload]);
  const content = (() => {
    if (documentPreview) {
      return <DocumentPreview prepared={documentPreview} translate={t} />;
    }
    if (!payload) return null;
    switch (renderer) {
      case "image":
        return <ImagePreview payload={payload} t={t} onCorrupt={onCorrupt} />;
      case "svg":
        return <SvgPreview payload={payload} t={t} onCorrupt={onCorrupt} />;
      case "text":
        return <TextPreview key={`${payload.entry_id ?? 0}-${payload.name}`} data={payload.data} name={payload.name} t={t} />;
      case "code":
        return <TextPreview key={`${payload.entry_id ?? 0}-${payload.name}`} data={payload.data} name={payload.name} forceCode t={t} />;
      case "markdown":
        return <MarkdownPreview data={payload.data} t={t} />;
      case "pdf":
        return <PdfPreview data={payload.data} t={t} onCorrupt={onCorrupt} />;
      case "docx":
        return <DocxPreview data={payload.data} t={t} onCorrupt={onCorrupt} />;
      default:
        return (
          <div className="preview-unsupported" data-testid="preview-unsupported">
            <div className={`preview-empty-icon${themeAssetSlots.previewPlaceholder ? " has-theme-image" : ""}`}>
              {!themeAssetSlots.previewPlaceholder && "?"}
            </div>
            <h2>{t("history.preview.unsupported")}</h2>
            <p>{payload.name}</p>
          </div>
        );
    }
  })();
  const boundaryKey = resetKey ?? `${payload?.entry_id ?? 0}-${payload?.name ?? "document"}`;
  return (
    <PreviewErrorBoundary
      key={boundaryKey}
      t={t}
      onRetry={onRetry ?? (() => undefined)}
    >
      <Suspense fallback={<div className="preview-render-loading" data-testid="preview-render-loading" />}>
        {content}
      </Suspense>
    </PreviewErrorBoundary>
  );
}
