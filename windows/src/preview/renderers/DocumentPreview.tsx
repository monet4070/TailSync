import { useCallback, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { ImageOff } from "lucide-react";
import { open as openExternalUrl } from "@tauri-apps/plugin-shell";
import type * as Contract from "../../types/localContracts.generated";
import {
  groupDocumentBlocks,
  type DocumentBlockGroup,
  type DocumentImagePlaceholder,
  type DocumentImagePlan,
  type DocumentImagePlanEntry,
  type PreparedDocumentAssets,
} from "../documentAssets";
import { contrastRatio, cssColor, headingLevel, readableTextColor, safeDocumentLink } from "../documentPresentation";

type DocumentNoticeLevel = "warning" | "diagnostic";

interface DocumentNoticeStyle {
  readonly labelKey: string;
  readonly summaryKey: string;
  readonly level: DocumentNoticeLevel;
}

const NOTICE_STYLES: Record<string, DocumentNoticeStyle> = {
  text_limit: { labelKey: "history.preview.partial.limit", summaryKey: "history.preview.notice.truncated", level: "warning" },
  block_limit: { labelKey: "history.preview.partial.limit", summaryKey: "history.preview.notice.truncated", level: "warning" },
  run_limit: { labelKey: "history.preview.partial.limit", summaryKey: "history.preview.notice.truncated", level: "warning" },
  cell_limit: { labelKey: "history.preview.partial.limit", summaryKey: "history.preview.notice.truncated", level: "warning" },
  image_limit: { labelKey: "history.preview.partial.limit", summaryKey: "history.preview.notice.truncated", level: "warning" },
  depth_limit: { labelKey: "history.preview.partial.limit", summaryKey: "history.preview.notice.truncated", level: "warning" },
  pixel_budget: { labelKey: "history.preview.partial.pixelBudget", summaryKey: "history.preview.notice.pixelBudget", level: "warning" },
  malformed_html: { labelKey: "history.preview.partial.malformed", summaryKey: "history.preview.notice.malformed", level: "warning" },
  unsupported_dependency: { labelKey: "history.preview.partial.malformed", summaryKey: "history.preview.notice.malformed", level: "warning" },
  unsupported_markup: { labelKey: "history.preview.partial.unsupportedMarkup", summaryKey: "history.preview.notice.unsupportedMarkup", level: "warning" },
  unsupported_image: { labelKey: "history.preview.partial.unsupportedImage", summaryKey: "history.preview.notice.imageUnsupported", level: "warning" },
  missing_image_source: { labelKey: "history.preview.partial.missingImage", summaryKey: "history.preview.notice.missingImage", level: "warning" },
  external_reference: { labelKey: "history.preview.partial.missingImage", summaryKey: "history.preview.notice.missingImage", level: "warning" },
  native_analysis_unavailable: { labelKey: "history.preview.partial.nativeAnalysis", summaryKey: "history.preview.notice.nativeUnavailable", level: "warning" },
  native_styles_unavailable: { labelKey: "history.preview.partial.stylesSimplified", summaryKey: "history.preview.notice.simplified", level: "diagnostic" },
  native_table_structure_unavailable: { labelKey: "history.preview.partial.nativeTableStructure", summaryKey: "history.preview.notice.structureUnavailable", level: "warning" },
  native_partial: { labelKey: "history.preview.partial.nativePartial", summaryKey: "history.preview.notice.structureUnavailable", level: "warning" },
  other_native_items: { labelKey: "history.preview.partial.nativeOther", summaryKey: "history.preview.notice.structureUnavailable", level: "warning" },
  unverified_provenance: { labelKey: "history.preview.partial.unverified", summaryKey: "history.preview.notice.unverifiedSource", level: "warning" },
  styles_simplified: { labelKey: "history.preview.partial.simplified", summaryKey: "history.preview.notice.simplified", level: "diagnostic" },
  unsupported_run: { labelKey: "history.preview.partial.simplified", summaryKey: "history.preview.notice.simplified", level: "diagnostic" },
};

const FALLBACK_NOTICE_STYLE: DocumentNoticeStyle = {
  labelKey: "history.preview.partial.other",
  summaryKey: "history.preview.notice.other",
  level: "diagnostic",
};

interface DocumentNotice {
  readonly code: string;
  readonly labelKey: string;
  readonly summaryKey: string;
  readonly detail: string | null;
  readonly level: DocumentNoticeLevel;
}

function noticeStyleFor(code: string): DocumentNoticeStyle {
  return NOTICE_STYLES[code] ?? FALLBACK_NOTICE_STYLE;
}

function savedAsset(document: Contract.DisplayDocument, image: Contract.DocumentImage): Contract.DocumentAsset | null {
  const assetIndex = image.asset ?? null;
  return assetIndex === null ? null : document.assets[assetIndex] ?? null;
}

function placeholderNoticeCode(
  entry: DocumentImagePlaceholder,
  image: Contract.DocumentImage,
  document: Contract.DisplayDocument,
): string | null {
  switch (entry.availability) {
    case "missing_source":
    case "missing_resource":
      return "missing_image_source";
    case "external_reference":
      return "external_reference";
    case "pixel_budget":
      return "pixel_budget";
    case "unsupported":
      return "unsupported_image";
    case "available":
      return savedAsset(document, image) === null ? "missing_image_source" : "unsupported_image";
  }
}

function placeholderAvailability(
  entry: DocumentImagePlaceholder,
  image: Contract.DocumentImage,
  document: Contract.DisplayDocument,
): Contract.DocumentImageAvailability {
  if (entry.availability !== "available") return entry.availability;
  return savedAsset(document, image) === null ? "missing_resource" : "unsupported";
}

function documentNotices(
  document: Contract.DisplayDocument,
  plan: DocumentImagePlan,
  failedImages: ReadonlySet<number>,
): DocumentNotice[] {
  const warnings: DocumentNotice[] = [];
  const diagnostics: DocumentNotice[] = [];
  const seen = new Set<string>();
  const record = (code: string, detail: string | null) => {
    const style = noticeStyleFor(code);
    const normalized = detail && detail.length > 0 ? detail : null;
    const key = `${code}\u0000${normalized ?? ""}`;
    if (seen.has(key)) return;
    seen.add(key);
    const notice: DocumentNotice = {
      code,
      labelKey: style.labelKey,
      summaryKey: style.summaryKey,
      detail: normalized,
      level: style.level,
    };
    (style.level === "warning" ? warnings : diagnostics).push(notice);
  };
  for (const reason of document.partial) record(reason.code, reason.detail);
  const reported = new Set<string>(document.partial.map((reason) => reason.code));
  const imageCodes: string[] = [];
  document.blocks.forEach((block, index) => {
    if (block.kind !== "image" || !block.image) return;
    const entry = plan.placements.get(index);
    if (entry?.kind === "image") {
      if (failedImages.has(index)) imageCodes.push("unsupported_image");
      return;
    }
    const placeholder = entry ?? {
      kind: "placeholder",
      availability: block.image.availability,
      budgetLimited: false,
    };
    if (placeholder.budgetLimited) {
      imageCodes.push("pixel_budget");
      return;
    }
    const code = placeholderNoticeCode(placeholder, block.image, document);
    if (code) imageCodes.push(code);
  });
  for (const code of imageCodes) if (!reported.has(code)) record(code, null);
  if (plan.pixelBudgetExceeded && !reported.has("pixel_budget")) record("pixel_budget", null);
  const hasUnsupportedRuns = document.blocks.some((block) => block.paragraph?.runs.some((run) => run.unsupported)
    || block.table?.rows.some((row) => row.cells.some((cell) => cell.runs.some((run) => run.unsupported))));
  if (hasUnsupportedRuns && !reported.has("unsupported_markup") && !reported.has("malformed_html")) {
    record("unsupported_run", null);
  }
  if (document.styles_partial && !reported.has("native_styles_unavailable")) {
    record("styles_simplified", null);
  }
  return [...warnings, ...diagnostics];
}

function noticeSummary(notices: ReadonlyArray<DocumentNotice>, translate: (key: string) => string): string {
  const summaryKeys: string[] = [];
  for (const notice of notices) {
    if (!summaryKeys.includes(notice.summaryKey)) summaryKeys.push(notice.summaryKey);
  }
  const reasons = summaryKeys.map((key) => translate(key)).join(translate("history.preview.noticeSeparator"));
  return translate("history.preview.noticeSummary").replace("{reasons}", reasons);
}

function noticeList(notices: ReadonlyArray<DocumentNotice>, translate: (key: string) => string): ReactNode {
  return (
    <ul className="preview-document-notice-list">
      {notices.map((notice) => (
        <li key={`${notice.code}-${notice.detail ?? ""}`} data-partial-code={notice.code}>
          <span className="preview-document-notice-label">{translate(notice.labelKey)}</span>
          <code className="preview-document-notice-code">{notice.code}</code>
          {notice.detail ? <span className="preview-document-notice-detail">{notice.detail}</span> : null}
        </li>
      ))}
    </ul>
  );
}

function runStyle(style: Contract.DocumentRunStyle, background: Contract.DocumentColor | null): CSSProperties {
  const result: CSSProperties = {};
  if (style.bold) result.fontWeight = 700;
  if (style.italic) result.fontStyle = "italic";
  const decorations: string[] = [];
  if (style.underline) decorations.push("underline");
  if (style.strikethrough) decorations.push("line-through");
  if (decorations.length > 0) result.textDecorationLine = decorations.join(" ");
  if (style.superscript || style.subscript) {
    result.verticalAlign = style.superscript ? "super" : "sub";
    result.fontSize = "smaller";
  }
  if (style.color) {
    result.color = !style.highlight && background && contrastRatio(style.color, background) < 4.5
      ? readableTextColor(background)
      : cssColor(style.color);
  }
  if (style.monospace) {
    result.fontFamily = "var(--font-mono)";
  }
  if (style.highlight) {
    result.backgroundColor = cssColor(style.highlight);
    if (!style.color || contrastRatio(style.color, style.highlight) < 4.5) {
      result.color = readableTextColor(style.highlight);
    }
  }
  return result;
}

function renderRuns(
  runs: ReadonlyArray<Contract.DocumentRun>,
  keyPrefix: string,
  openLink: (url: string) => void,
  wrapInlineCode = true,
  background: Contract.DocumentColor | null = null,
): ReactNode {
  return runs.map((run, index) => {
    const key = `${keyPrefix}-run-${index}`;
    const style = runStyle(run.style, background);
    const target = safeDocumentLink(run.link);
    if (target !== null) {
      return (
        <a
          key={key}
          className="preview-document-link"
          role="link"
          tabIndex={0}
          data-href={target}
          title={target}
          style={style}
          data-unsupported={run.unsupported ? "true" : undefined}
          onClick={(event) => {
            event.preventDefault();
            openLink(target);
          }}
          onKeyDown={(event) => {
            if (event.key !== "Enter" && event.key !== " ") return;
            event.preventDefault();
            openLink(target);
          }}
        >
          {run.text}
        </a>
      );
    }
    if (run.style.monospace && wrapInlineCode) {
      return (
        <code key={key} className="preview-document-inline-code" style={style} data-unsupported={run.unsupported ? "true" : undefined}>
          {run.text}
        </code>
      );
    }
    return (
      <span key={key} style={style} data-unsupported={run.unsupported ? "true" : undefined}>
        {run.text}
      </span>
    );
  });
}

function imageReasonLabel(availability: Contract.DocumentImageAvailability, budgetLimited: boolean, translate: (key: string) => string): string {
  if (budgetLimited) return translate("history.preview.imageBudget");
  return translate(`history.preview.imageAvailability.${availability}`);
}

const UNSAVED_IMAGE_AVAILABILITIES: ReadonlySet<Contract.DocumentImageAvailability> = new Set([
  "missing_source",
  "missing_resource",
  "external_reference",
]);

function imagePlaceholderPrefix(
  availability: Contract.DocumentImageAvailability,
  budgetLimited: boolean,
): string {
  if (!budgetLimited && UNSAVED_IMAGE_AVAILABILITIES.has(availability)) {
    return "history.preview.imageUnavailable";
  }
  return "history.preview.imageCannotDisplay";
}

function ImagePlaceholder({
  availability,
  budgetLimited,
  ordinal,
  translate,
}: {
  availability: Contract.DocumentImageAvailability;
  budgetLimited: boolean;
  ordinal: number;
  translate: (key: string) => string;
}) {
  const label = `${translate(imagePlaceholderPrefix(availability, budgetLimited))} (${ordinal + 1}): ${imageReasonLabel(availability, budgetLimited, translate)}`;
  return (
    <div
      className="preview-document-image-placeholder"
      role="img"
      aria-label={label}
      data-testid="preview-document-image-placeholder"
      data-availability={availability}
      data-budget-limited={budgetLimited ? "true" : undefined}
      data-ordinal={ordinal}
    >
      <ImageOff size={22} aria-hidden="true" />
      <span>{label}</span>
    </div>
  );
}

function DocumentImage({
  url,
  ordinal,
  translate,
  onDecodeFailure,
}: {
  url: string;
  ordinal: number;
  translate: (key: string) => string;
  onDecodeFailure: () => void;
}) {
  const [failed, setFailed] = useState(false);
  if (failed) {
    return <ImagePlaceholder availability="unsupported" budgetLimited={false} ordinal={ordinal} translate={translate} />;
  }
  return (
    <img
      className="preview-document-image"
      src={url}
      alt={`${translate("history.preview.documentImage")} ${ordinal + 1}`}
      data-testid="preview-document-image"
      data-ordinal={ordinal}
      onError={() => {
        setFailed(true);
        onDecodeFailure();
      }}
    />
  );
}

export type DocumentExternalOpener = (url: string) => void;

function defaultOpenExternal(url: string): void {
  void openExternalUrl(url).catch((error: unknown) => {
    console.error("Could not open document preview link:", error);
  });
}

function DocumentHeading({ level, blockIndex, children }: { level: 1 | 2 | 3; blockIndex: number; children: ReactNode }) {
  if (level === 1) return <h1 className="preview-document-heading" data-heading="1" data-block={blockIndex}>{children}</h1>;
  if (level === 2) return <h2 className="preview-document-heading" data-heading="2" data-block={blockIndex}>{children}</h2>;
  return <h3 className="preview-document-heading" data-heading="3" data-block={blockIndex}>{children}</h3>;
}

interface DocumentPreviewProps {
  prepared: PreparedDocumentAssets;
  translate: (key: string) => string;
  openExternal?: DocumentExternalOpener;
}

export function DocumentPreview({ prepared, translate, openExternal = defaultOpenExternal }: DocumentPreviewProps) {
  const metadata = prepared.frame.metadata;
  const identity = [metadata.entry_id, metadata.owner, metadata.generation, metadata.request_id, metadata.stamp].join("\u0000");
  return <DocumentPreviewFrame key={identity} prepared={prepared} translate={translate} openExternal={openExternal} />;
}

function DocumentPreviewFrame({ prepared, translate, openExternal }: Required<DocumentPreviewProps>) {
  const document = prepared.frame.document;
  const plan = prepared.imagePlan;
  const root = useRef<HTMLElement>(null);
  const [background, setBackground] = useState<Contract.DocumentColor | null>(null);
  useLayoutEffect(() => {
    const element = root.current;
    if (!element) return;
    const refresh = () => {
      const raw = window.getComputedStyle(element).backgroundColor;
      const match = raw.match(/^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:\s*[,/]\s*([\d.]+))?\s*\)$/);
      if (!match || (match[4] !== undefined && Number(match[4]) !== 1)) {
        setBackground(null);
        return;
      }
      const color = { red: Number(match[1]), green: Number(match[2]), blue: Number(match[3]) };
      setBackground((previous) => previous?.red === color.red && previous.green === color.green && previous.blue === color.blue ? previous : color);
    };
    refresh();
    const observer = new MutationObserver(refresh);
    const options = { attributes: true, attributeFilter: ["class", "style"] };
    observer.observe(element, options);
    const themeRoot = element.closest(".app");
    if (themeRoot) observer.observe(themeRoot, options);
    observer.observe(globalThis.document.documentElement, options);
    return () => observer.disconnect();
  }, []);
  const [failedUrls, setFailedUrls] = useState<ReadonlySet<string>>(() => new Set<string>());
  const failedImages = useMemo(() => {
    const blocks = new Set<number>();
    plan.placements.forEach((entry, blockIndex) => {
      if (entry.kind === "image" && failedUrls.has(entry.url)) blocks.add(blockIndex);
    });
    return blocks;
  }, [plan, failedUrls]);
  const groups = useMemo(() => groupDocumentBlocks(document.blocks), [document]);
  const notices = useMemo(
    () => documentNotices(document, plan, failedImages),
    [document, plan, failedImages],
  );
  const warnings = notices.filter((notice) => notice.level === "warning");
  const diagnostics = notices.filter((notice) => notice.level === "diagnostic");

  const openLink = useCallback((url: string) => {
    if (safeDocumentLink(url) === null) return;
    openExternal(url);
  }, [openExternal]);

  const paragraphNode = (paragraph: Contract.DocumentParagraph, blockIndex: number, keyPrefix: string) => {
    if (paragraph.code_block) {
      return (
        <pre key={`block-${blockIndex}`} className="preview-document-code" data-code-block="true" data-block={blockIndex}>
          <code className="preview-document-code-content">{renderRuns(paragraph.runs, keyPrefix, openLink, false, background)}</code>
        </pre>
      );
    }
    const level = headingLevel(paragraph.heading);
    if (level !== null) {
      return (
        <DocumentHeading key={`block-${blockIndex}`} level={level} blockIndex={blockIndex}>
          {renderRuns(paragraph.runs, keyPrefix, openLink, true, background)}
        </DocumentHeading>
      );
    }
    return (
      <p key={`block-${blockIndex}`} className="preview-document-paragraph" data-block={blockIndex}>
        {renderRuns(paragraph.runs, keyPrefix, openLink, true, background)}
      </p>
    );
  };

  const listItemContent = (paragraph: Contract.DocumentParagraph | null, blockIndex: number, keyPrefix: string) => {
    if (!paragraph) return null;
    if (paragraph.code_block || headingLevel(paragraph.heading) !== null) {
      return paragraphNode(paragraph, blockIndex, keyPrefix);
    }
    return renderRuns(paragraph.runs, keyPrefix, openLink, true, background);
  };

  const renderImage = (group: Extract<DocumentBlockGroup, { kind: "image" }>) => {
    const image = group.block.image;
    if (!image) return null;
    const entry = plan.placements.get(group.blockIndex);
    if (entry && entry.kind === "image") {
      return (
        <DocumentImage
          key={`block-${group.blockIndex}-${entry.url}`}
          url={entry.url}
          ordinal={image.ordinal}
          translate={translate}
          onDecodeFailure={() => setFailedUrls((previous) => new Set(previous).add(entry.url))}
        />
      );
    }
    const fallback: DocumentImagePlanEntry = entry ?? {
      kind: "placeholder",
      availability: image.availability,
      budgetLimited: false,
    };
    return (
      <ImagePlaceholder
        key={`block-${group.blockIndex}`}
        availability={fallback.kind === "placeholder" ? placeholderAvailability(fallback, image, document) : "unsupported"}
        budgetLimited={fallback.kind === "placeholder" && fallback.budgetLimited}
        ordinal={image.ordinal}
        translate={translate}
      />
    );
  };

  const renderGroup = (group: DocumentBlockGroup) => {
    switch (group.kind) {
      case "paragraph": {
        const paragraph = group.block.paragraph;
        if (!paragraph) return null;
        return paragraphNode(paragraph, group.blockIndex, `block-${group.blockIndex}`);
      }
      case "list": {
        const ListTag = group.list === "ordered" ? "ol" : "ul";
        return (
          <ListTag
            key={`block-${group.items[0].blockIndex}`}
            className="preview-document-list"
            data-list={group.list}
            data-list-depth={group.depth}
            start={group.list === "ordered" ? group.start : undefined}
            style={group.depth === 0 && group.quoteDepth === 0
              ? undefined
              : { marginLeft: group.depth * 18 + group.quoteDepth * 14 }}
          >
            {group.items.map((item) => (
              <li key={`block-${item.blockIndex}`} data-block={item.blockIndex}>
                {listItemContent(item.block.paragraph ?? null, item.blockIndex, `block-${item.blockIndex}`)}
              </li>
            ))}
          </ListTag>
        );
      }
      case "quote": {
        return (
          <blockquote
            key={`block-${group.items[0].blockIndex}`}
            className="preview-document-quote"
            data-quote-depth={group.depth}
            style={{ marginLeft: (group.depth - 1) * 16 }}
          >
            {group.items.map((item) => {
              const paragraph = item.block.paragraph;
              if (!paragraph) return null;
              return paragraphNode(paragraph, item.blockIndex, `block-${item.blockIndex}`);
            })}
          </blockquote>
        );
      }
      case "image":
        return renderImage(group);
      case "table": {
        const table = group.block.table;
        if (!table) return null;
        return (
          <div key={`block-${group.blockIndex}`} className="preview-document-table-wrap">
            <table className="preview-document-table" aria-label={translate("history.preview.documentTable")}>
              <tbody>
                {table.rows.map((row, rowIndex) => (
                  <tr key={`row-${rowIndex}`}>
                    {row.cells.map((cell, cellIndex) =>
                      cell.header ? (
                        <th key={`cell-${cellIndex}`} scope="col" data-testid="preview-document-cell">
                          {renderRuns(cell.runs, `block-${group.blockIndex}-${rowIndex}-${cellIndex}`, openLink, true, background)}
                        </th>
                      ) : (
                        <td key={`cell-${cellIndex}`} data-testid="preview-document-cell">
                          {renderRuns(cell.runs, `block-${group.blockIndex}-${rowIndex}-${cellIndex}`, openLink, true, background)}
                        </td>
                      ),
                    )}
                  </tr>
                ))}
              </tbody>
            </table>
            {table.truncated ? (
              <p className="preview-document-table-note" role="note">
                {translate("history.preview.partial.limit")}
              </p>
            ) : null}
          </div>
        );
      }
    }
  };

  return (
    <article ref={root} className="preview-document" data-testid="preview-document">
      {warnings.length > 0 && (
        <aside className="preview-document-notices" role="note" aria-label={translate("history.preview.partialTitle")}>
          <p className="preview-document-notice-summary">{noticeSummary(warnings, translate)}</p>
          <details className="preview-document-notice-details">
            <summary>{translate("history.preview.noticeDetails")}</summary>
            {noticeList(notices, translate)}
          </details>
        </aside>
      )}
      {warnings.length === 0 && diagnostics.length > 0 && (
        <details className="preview-document-diagnostics" data-testid="preview-document-diagnostics">
          <summary>{translate("history.preview.noticeDiagnostics").replace("{count}", String(diagnostics.length))}</summary>
          {noticeList(diagnostics, translate)}
        </details>
      )}
      <div className="preview-document-body">{groups.map(renderGroup)}</div>
    </article>
  );
}
