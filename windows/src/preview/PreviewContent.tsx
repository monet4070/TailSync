import { lazy, Suspense, useMemo } from "react";
import { useTheme } from "../hooks/useTheme";
import { previewRendererFor, type PreviewPayload } from "../utils/historyPreview";

// Keep PDF, DOCX and syntax-highlighting code out of the history window's
// initial bundle. The renderer is loaded only after the user opens that type.
const ImagePreview = lazy(async () => import("./renderers/ImagePreview").then((module) => ({ default: module.ImagePreview })));
const MarkdownPreview = lazy(async () => import("./renderers/MarkdownPreview").then((module) => ({ default: module.MarkdownPreview })));
const PdfPreview = lazy(async () => import("./renderers/PdfPreview").then((module) => ({ default: module.PdfPreview })));
const DocxPreview = lazy(async () => import("./renderers/DocxPreview").then((module) => ({ default: module.DocxPreview })));
const TextPreview = lazy(async () => import("./renderers/TextPreview").then((module) => ({ default: module.TextPreview })));

export function PreviewContent({
  payload,
  t,
  onCorrupt,
}: {
  payload: PreviewPayload;
  t: (key: string) => string;
  onCorrupt: () => void;
}) {
  const { themeAssetSlots } = useTheme();
  const renderer = useMemo(() => previewRendererFor(payload), [payload]);
  const content = (() => {
    switch (renderer) {
      case "image":
        return <ImagePreview payload={payload} t={t} onCorrupt={onCorrupt} />;
      case "text":
        return <TextPreview key={`${payload.entry_id ?? 0}-${payload.name}`} data={payload.data} name={payload.name} t={t} />;
      case "code":
        return <TextPreview key={`${payload.entry_id ?? 0}-${payload.name}`} data={payload.data} name={payload.name} forceCode t={t} />;
      case "markdown":
        return <MarkdownPreview data={payload.data} />;
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
  return <Suspense fallback={<div className="preview-render-loading" data-testid="preview-render-loading" />}>{content}</Suspense>;
}
