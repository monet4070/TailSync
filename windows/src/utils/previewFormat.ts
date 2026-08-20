import type { PreviewKind, PreviewMetadata, PreviewRenderer } from "./previewTypes";

export function previewFileExtension(name: string): string {
  const basename = name.split(/[\\/]/).pop() ?? name;
  const dot = basename.lastIndexOf(".");
  if (dot <= 0 || dot === basename.length - 1) return "";
  return basename.slice(dot + 1).toLowerCase();
}

function previewInput(
  kindOrPayload: PreviewKind | Pick<PreviewMetadata, "kind" | "name">,
  name?: string,
): { kind: PreviewKind; name: string } {
  return typeof kindOrPayload === "string"
    ? { kind: kindOrPayload, name: name ?? "" }
    : kindOrPayload;
}

export function selectPreviewRenderer(
  kindOrPayload: PreviewKind | Pick<PreviewMetadata, "kind" | "name">,
  name?: string,
): PreviewRenderer {
  const { kind, name: filename } = previewInput(kindOrPayload, name);
  if (kind === "image" || kind === "text") return kind === "image" ? "image" : "text";
  if (kind !== "file") return "unsupported";

  switch (previewFileExtension(filename)) {
    case "png":
    case "jpg":
    case "jpeg":
    case "gif":
    case "webp": return "image";
    case "txt":
    case "svg": return "text";
    case "md":
    case "markdown": return "markdown";
    case "c":
    case "cc":
    case "cpp":
    case "cs":
    case "css":
    case "go":
    case "h":
    case "hpp":
    case "html":
    case "java":
    case "js":
    case "jsx":
    case "json":
    case "kt":
    case "kts":
    case "mjs":
    case "php":
    case "ps1":
    case "py":
    case "rb":
    case "rs":
    case "sh":
    case "sql":
    case "swift":
    case "toml":
    case "ts":
    case "tsx":
    case "xml":
    case "yaml":
    case "yml": return "code";
    case "pdf": return "pdf";
    case "docx": return "docx";
    default: return "unsupported";
  }
}

export function getPreviewMimeType(
  kindOrPayload: PreviewKind | Pick<PreviewMetadata, "kind" | "name">,
  name?: string,
): string | null {
  const { kind, name: filename } = previewInput(kindOrPayload, name);
  if (kind === "image") return null;
  if (kind === "text") return "text/plain";

  switch (previewFileExtension(filename)) {
    case "txt": return "text/plain";
    case "md":
    case "markdown": return "text/markdown";
    case "pdf": return "application/pdf";
    case "docx": return "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    case "png": return "image/png";
    case "jpg":
    case "jpeg": return "image/jpeg";
    case "gif": return "image/gif";
    case "webp": return "image/webp";
    case "svg": return "text/plain";
    default: return null;
  }
}

export const previewRendererFor = selectPreviewRenderer;
export const previewMimeTypeFor = getPreviewMimeType;
