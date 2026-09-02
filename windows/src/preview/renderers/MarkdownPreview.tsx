import { useCallback, useMemo } from "react";
import DOMPurify from "dompurify";
import { marked } from "marked";
import { open } from "@tauri-apps/plugin-shell";
import { useModifierWheelZoom, usePreviewTextFontSize, zoomFromWheel } from "../previewPreferences";
import { limitTextSource } from "./textPreviewPolicy";

function explicitWebUrl(value: string): string | null {
  try {
    const parsed = new URL(value);
    return parsed.protocol === "http:" || parsed.protocol === "https:" ? parsed.href : null;
  } catch {
    return null;
  }
}

function sanitizeRenderedMarkdown(rendered: string): string {
  const sanitized = DOMPurify.sanitize(rendered, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: [
      "audio",
      "base",
      "embed",
      "form",
      "iframe",
      "img",
      "link",
      "meta",
      "object",
      "picture",
      "source",
      "style",
      "svg",
      "video",
    ],
    FORBID_ATTR: [
      "formaction",
      "ping",
      "src",
      "srcset",
      "style",
      "target",
      "xlink:href",
    ],
  });

  // DOMPurify blocks script-bearing URLs. This second, explicit policy also
  // removes relative, file, mail and custom-scheme navigation so a preview can
  // only hand an intentional http(s) click to the operating system.
  const template = document.createElement("template");
  template.innerHTML = sanitized;
  template.content.querySelectorAll<HTMLAnchorElement>("a[href]").forEach((anchor) => {
    const safe = explicitWebUrl(anchor.getAttribute("href") ?? "");
    if (safe === null) {
      anchor.removeAttribute("href");
      return;
    }
    anchor.setAttribute("href", safe);
    anchor.setAttribute("rel", "noopener noreferrer");
  });
  return template.innerHTML;
}

export function MarkdownPreview({
  data,
  t,
}: {
  data: Uint8Array;
  t?: (key: string) => string;
}) {
  const [fontSize, setFontSize] = usePreviewTextFontSize();
  const articleRef = useModifierWheelZoom<HTMLElement>((deltaY) => {
    setFontSize((value) => zoomFromWheel(value, deltaY, 12, 32));
  });
  const source = useMemo(() => new TextDecoder("utf-8").decode(data), [data]);
  const limited = useMemo(() => limitTextSource(source), [source]);
  const html = useMemo(() => {
    const rendered = marked.parse(limited.source);
    if (typeof rendered !== "string") return "";
    return sanitizeRenderedMarkdown(rendered);
  }, [limited.source]);

  const openLink = useCallback((event: React.MouseEvent<HTMLElement>) => {
    const anchor = event.target instanceof Element
      ? event.target.closest<HTMLAnchorElement>("a[href]")
      : null;
    if (anchor === null) return;
    event.preventDefault();
    const url = explicitWebUrl(anchor.getAttribute("href") ?? "");
    if (url === null) return;
    void open(url).catch((error: unknown) => {
      console.error("Could not open Markdown preview link:", error);
    });
  }, []);

  return (
    <>
      {t && limited.truncated && (
        <div className="preview-truncated-notice" role="status" data-testid="preview-truncated">
          {t("history.preview.truncated")}
        </div>
      )}
      <article
        ref={articleRef}
        className="preview-markdown"
        data-testid="preview-markdown"
        style={{ fontSize }}
        onClick={openLink}
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </>
  );
}
