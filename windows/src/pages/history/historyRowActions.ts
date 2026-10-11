import type { CompositeInfo } from "../../types/localContracts.generated";
import type { Translate } from "./HistoryViewTypes";

const ALLOWED_RESTORE_FORMATS = new Set(["text", "png"]);

export function explicitRestoreFormats(composite: CompositeInfo): string[] {
  if (composite.native_only) return [];
  const withheld = new Set(composite.withheld_formats.map((entry) => entry.format));
  const formats = composite.restorable_formats.filter(
    (format) => ALLOWED_RESTORE_FORMATS.has(format) && !withheld.has(format),
  );
  if (!hasRichContent(composite)) return formats.filter((format) => format !== "text");
  return formats;
}

export function hasRichContent(composite: CompositeInfo): boolean {
  return composite.image_count > 0 || composite.formats.some((format) => format !== "text");
}

export function showsAdvancedActions(
  composite: CompositeInfo,
  handlers: { authorize: boolean; importPacket: boolean; revoke: boolean },
): boolean {
  if (handlers.revoke) return true;
  if (!showsMaintenanceActions(composite)) return false;
  return handlers.authorize || handlers.importPacket;
}

export function showsMaintenanceActions(composite: CompositeInfo): boolean {
  return (
    !composite.native_only &&
    composite.formats.includes("html") &&
    composite.completeness !== "complete"
  );
}

export function rowStatusLabel(
  composite: CompositeInfo | null | undefined,
  translate: Translate,
): string | null {
  if (!composite) return null;
  if (composite.native_only) return translate("history.composite.localOnly");
  if (composite.completeness === "external_references") return translate("history.composite.resourcesMissing");
  if (composite.completeness !== "complete") return translate("history.composite.incomplete");
  return null;
}

export interface RowDiagnostic {
  label: string;
  value: string;
}

export function rowDiagnostics(composite: CompositeInfo, translate: Translate): RowDiagnostic[] {
  const rows: RowDiagnostic[] = [
    { label: translate("history.composite.diag.formats"), value: joined(composite.formats, translate) },
    {
      label: translate("history.composite.diag.representations"),
      value: `${composite.representation_count}`,
    },
    { label: translate("history.composite.diag.images"), value: `${composite.image_count}` },
    { label: translate("history.composite.diag.nativeItems"), value: `${composite.native_item_count}` },
    {
      label: translate("history.composite.diag.nativeOnly"),
      value: composite.native_only
        ? translate("history.composite.diag.yes")
        : translate("history.composite.diag.no"),
    },
    {
      label: translate("history.composite.diag.completeness"),
      value: completenessText(composite.completeness, translate),
    },
    {
      label: translate("history.composite.diag.restorable"),
      value: joined(composite.restorable_formats, translate),
    },
  ];
  if (composite.withheld_formats.length > 0) {
    rows.push({
      label: translate("history.composite.diag.withheld"),
      value: composite.withheld_formats
        .map((withheld) => `${withheld.format}: ${withheldReason(withheld.code, translate)}`)
        .join(", "),
    });
  }
  return rows;
}

function joined(values: string[], translate: Translate): string {
  return values.length === 0 ? translate("history.composite.diag.none") : values.join(", ");
}

function completenessText(completeness: CompositeInfo["completeness"], translate: Translate): string {
  const key = `history.composite.completeness.${completeness}`;
  const text = translate(key);
  return text === key ? translate("history.composite.diag.unknown") : text;
}

function withheldReason(code: string, translate: Translate): string {
  const key = `history.restore.code.${code}`;
  const text = translate(key);
  return text === key ? translate("history.composite.diag.unavailable") : text;
}
