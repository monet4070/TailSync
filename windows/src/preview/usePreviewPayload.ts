import { useCallback, useEffect, useState } from "react";
import { getPreview } from "../tailsyncClient";
import {
  parsePreviewResponse,
  PreviewParseError,
  type PreviewPayload,
} from "../utils/historyPreview";

export interface PreviewTarget {
  entryId: number;
  batchId: string | null;
}

export type PreviewFailureKind =
  | "too-large"
  | "corrupt"
  | "unavailable"
  | "transport";

export interface PreviewFailure {
  kind: PreviewFailureKind;
  retryable: boolean;
  sizeBytes: number | null;
  limitBytes: number | null;
}

export type PreviewLoadState =
  | { status: "idle" }
  | { status: "loading"; target: PreviewTarget }
  | { status: "ready"; target: PreviewTarget; payload: PreviewPayload }
  | { status: "error"; target: PreviewTarget; failure: PreviewFailure };

interface CommandFailure {
  code?: unknown;
  retryable?: unknown;
  size_bytes?: unknown;
  limit_bytes?: unknown;
}

function finiteBytes(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0
    ? value
    : null;
}

export function classifyPreviewFailure(error: unknown): PreviewFailure {
  if (error instanceof PreviewParseError) {
    return {
      kind: error.code === "payload_too_large" ? "too-large" : "corrupt",
      retryable: false,
      sizeBytes: null,
      limitBytes: null,
    };
  }

  if (typeof error === "object" && error !== null) {
    const command = error as CommandFailure;
    const code = typeof command.code === "string" ? command.code : "";
    if (code === "preview_too_large") {
      return {
        kind: "too-large",
        retryable: false,
        sizeBytes: finiteBytes(command.size_bytes),
        limitBytes: finiteBytes(command.limit_bytes),
      };
    }
    if (code === "payload_unavailable" || code === "metadata_unavailable") {
      return {
        kind: "unavailable",
        retryable: command.retryable === true,
        sizeBytes: null,
        limitBytes: null,
      };
    }
    if (
      code === "entry_not_found" ||
      code === "batch_not_found" ||
      code === "entry_not_in_batch" ||
      code === "unsupported_type" ||
      code === "invalid_size"
    ) {
      return {
        kind: "unavailable",
        retryable: false,
        sizeBytes: null,
        limitBytes: null,
      };
    }
  }

  const message = error instanceof Error ? error.message : String(error ?? "");
  if (/too large|64\s*mi?b/i.test(message)) {
    return { kind: "too-large", retryable: false, sizeBytes: null, limitBytes: null };
  }
  return { kind: "transport", retryable: true, sizeBytes: null, limitBytes: null };
}

export function usePreviewPayload(target: PreviewTarget | null) {
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState<PreviewLoadState>({ status: "idle" });

  useEffect(() => {
    if (target === null) {
      setState({ status: "idle" });
      return undefined;
    }

    let active = true;
    setState({ status: "loading", target });
    void getPreview(target.entryId, target.batchId)
      .then((response) => parsePreviewResponse(response))
      .then((payload) => {
        if (active) setState({ status: "ready", target, payload });
      })
      .catch((error: unknown) => {
        if (active) {
          console.error("History preview request failed:", error);
          setState({ status: "error", target, failure: classifyPreviewFailure(error) });
        }
      });

    return () => {
      active = false;
    };
  }, [attempt, target]);

  const retry = useCallback(() => setAttempt((value) => value + 1), []);
  return { state, retry };
}
