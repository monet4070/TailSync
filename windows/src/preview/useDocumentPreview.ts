import { useCallback, useEffect, useRef, useState } from "react";
import { cancelDocumentPreview, getDocumentPreview, validateDocumentPreview } from "../tailsyncClient";
import { DocumentParseError } from "../utils/documentPreview";
import {
  DocumentPrepareError,
  prepareDocumentAssets,
  type PreparedDocumentAssets,
} from "./documentAssets";
import { classifyPreviewFailure, type PreviewFailure } from "./usePreviewPayload";
import { demoDocumentFrame, isDemoEntry } from "../onboarding/demo";
import { readTour } from "../onboarding/tourState";

export interface DocumentTarget {
  entryId: number;
  owner: "history" | "favorites";
}

export type DocumentLoadState =
  | { status: "idle" }
  | { status: "loading"; target: DocumentTarget }
  | { status: "ready"; target: DocumentTarget; prepared: PreparedDocumentAssets }
  | { status: "error"; target: DocumentTarget; failure: PreviewFailure };

let documentGeneration = Date.now();
let documentRequestCounter = 0;
let documentJobRunning = false;
interface DocumentJobWaiter {
  start(): boolean;
  cancel(reason: unknown): void;
}
const waitingDocumentJobs = new Map<DocumentTarget["owner"], DocumentJobWaiter>();

function releaseDocumentJob(): void {
  documentJobRunning = false;
  while (waitingDocumentJobs.size > 0) {
    const [owner, waiter] = waitingDocumentJobs.entries().next().value!;
    waitingDocumentJobs.delete(owner);
    if (waiter.start()) return;
  }
}

function acquireDocumentJob(owner: DocumentTarget["owner"], signal: AbortSignal): Promise<void> {
  if (signal.aborted) return Promise.reject(new DocumentPrepareError("cancelled", "Document preparation cancelled"));
  if (!documentJobRunning) {
    documentJobRunning = true;
    return Promise.resolve();
  }
  return new Promise((resolve, reject) => {
    let timeout: number | undefined;
    const cleanup = () => {
      window.clearTimeout(timeout);
      signal.removeEventListener("abort", abort);
    };
    const waiter: DocumentJobWaiter = {
      start: () => {
        cleanup();
        if (signal.aborted) {
          reject(new DocumentPrepareError("cancelled", "Document preparation cancelled"));
          return false;
        }
        documentJobRunning = true;
        resolve();
        return true;
      },
      cancel: (reason) => {
        if (waitingDocumentJobs.get(owner) === waiter) waitingDocumentJobs.delete(owner);
        cleanup();
        reject(reason);
      },
    };
    const abort = () => waiter.cancel(new DocumentPrepareError("cancelled", "Document preparation cancelled"));
    waitingDocumentJobs.get(owner)?.cancel(new DocumentPrepareError("cancelled", "Document request superseded"));
    waitingDocumentJobs.set(owner, waiter);
    signal.addEventListener("abort", abort, { once: true });
    timeout = window.setTimeout(() => waiter.cancel({ code: "payload_unavailable", retryable: true }), 2000);
    if (signal.aborted) abort();
  });
}

export function nextDocumentGeneration(): number {
  documentGeneration = Math.max(documentGeneration + 1, Date.now());
  return documentGeneration;
}

function nextDocumentRequestId(): string {
  documentRequestCounter += 1;
  const random =
    typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
      ? crypto.randomUUID()
      : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  return `doc-${documentRequestCounter}-${random}`.slice(0, 128);
}

export function classifyDocumentFailure(error: unknown): PreviewFailure {
  if (error instanceof DocumentParseError) {
    return {
      kind: error.code === "payload_too_large" ? "too-large" : "corrupt",
      retryable: false,
      sizeBytes: null,
      limitBytes: null,
    };
  }
  if (error instanceof DocumentPrepareError) {
    return {
      kind: error.code === "too-large" ? "too-large" : "corrupt",
      retryable: false,
      sizeBytes: null,
      limitBytes: null,
    };
  }
  return classifyPreviewFailure(error);
}

export function useDocumentPreview(target: DocumentTarget | null) {
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState<DocumentLoadState>({ status: "idle" });
  const preparedRef = useRef<PreparedDocumentAssets | null>(null);
  const preparedRequestRef = useRef<Parameters<typeof validateDocumentPreview>[0] | null>(null);

  const releasePrepared = useCallback(() => {
    preparedRef.current?.release();
    preparedRef.current = null;
    preparedRequestRef.current = null;
  }, []);

  useEffect(() => releasePrepared, [releasePrepared]);

  useEffect(() => {
    releasePrepared();
    if (target === null) {
      setState({ status: "idle" });
      return undefined;
    }

    const controller = new AbortController();
    let active = true;
    let cancelled = false;
    let started = false;
    const demo = isDemoEntry(target.entryId);
    const request = {
      entry_id: target.entryId,
      owner: target.owner,
      generation: nextDocumentGeneration(),
      request_id: nextDocumentRequestId(),
    };
    const cancel = () => {
      if (demo || cancelled || !started) return;
      cancelled = true;
      void cancelDocumentPreview(request).catch(() => undefined);
    };
    setState({ status: "loading", target });

    void (async () => {
      let prepared: PreparedDocumentAssets | null = null;
      let admitted = false;
      try {
        await acquireDocumentJob(target.owner, controller.signal);
        admitted = true;
        if (!active || controller.signal.aborted) return;
        started = true;
        const frame = demo
          ? (readTour().active ? demoDocumentFrame(request) : null)
          : await getDocumentPreview(request, controller.signal);
        if (frame === null) throw new Error("Demo document is unavailable");
        if (!active || controller.signal.aborted) return;
        prepared = await prepareDocumentAssets(frame, controller.signal);
        if (!active || controller.signal.aborted) {
          prepared.release();
          return;
        }
        const current = demo ? readTour().active : await validateDocumentPreview(request, frame.metadata.stamp);
        if (!active || controller.signal.aborted) {
          prepared.release();
          return;
        }
        if (!current) throw new Error("Document request stale");
        const installed = prepared;
        prepared = null;
        releasePrepared();
        preparedRef.current = installed;
        preparedRequestRef.current = demo ? null : request;
        setState({ status: "ready", target, prepared: installed });
      } catch (error) {
        prepared?.release();
        if (admitted) cancel();
        if (!active || controller.signal.aborted) return;
        console.error("History document preview failed:", error);
        setState({ status: "error", target, failure: classifyDocumentFailure(error) });
      } finally {
        if (admitted) releaseDocumentJob();
      }
    })();

    return () => {
      active = false;
      controller.abort();
      cancel();
      releasePrepared();
    };
  }, [attempt, releasePrepared, target]);

  const retry = useCallback(() => setAttempt((value) => value + 1), []);
  const refresh = useCallback(async (canRefresh: () => boolean = () => true) => {
    if (target === null || isDemoEntry(target.entryId) || !canRefresh()) return;
    const installed = preparedRef.current;
    const installedRequest = preparedRequestRef.current;
    if (installed === null || installedRequest === null) {
      retry();
      return;
    }
    try {
      const current = await validateDocumentPreview(installedRequest, installed.frame.metadata.stamp);
      if (!current && preparedRef.current === installed && canRefresh()) retry();
    } catch {
      return;
    }
  }, [retry, target]);
  const visibleState: DocumentLoadState = target === null
    ? { status: "idle" }
    : state.status !== "idle" && state.target === target
      ? state
      : { status: "loading", target };
  return { state: visibleState, retry, refresh };
}
