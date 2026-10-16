// F10-C3 regression: the Windows pairing surfaces must consume the finite
// classification the shared IPC boundary already normalized.
//
// Only the Tauri command bridge (`@tauri-apps/api/core`) and the native event
// bridge (`@tauri-apps/api/event`) are mocked. `diagnostics/localIpcMetrics`,
// `normalizeIpcError`, `tailsyncClient`, `usePairing`/`useRemotePairing` and the
// real `PairingDialog` / `RemotePairingPanel` all run for real, so a versioned
// rejection travels the production path from the raw Tauri promise to the DOM.

import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { useCallback } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { tauriInvokeMock, listenMock } = vi.hoisted(() => ({
  tauriInvokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauriInvokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import en from "../../i18n/en.json";
import zhCN from "../../i18n/zh-CN.json";
import { useI18n } from "../../hooks/useI18n";
import { usePairing } from "../../hooks/usePairing";
import { useRemotePairing } from "../../hooks/useRemotePairing";
import type { PeerDevice } from "../../tailsyncClient";
import { PairingDialog } from "./SettingsDialogs";
import { RemotePairingPanel } from "./RemotePairingPanel";

const PEER: PeerDevice = {
  hostname: "MacBook",
  tailscale_ip: "100.64.0.5",
  address: "192.168.1.10",
  online: true,
  enabled: true,
  connection_mode: "auto",
  trusted: false,
  fingerprint: "SHA256:peer",
};

const INVITE_LINK = "tailsync://pair/v1/aaa";

/** Wire shape of `crate::pairing::PairingStatus`: every field is always sent. */
function wireStatus(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    pairing_enabled: true,
    phase: "waiting",
    expires_at: null,
    remaining_seconds: 60,
    failed_attempts: 0,
    max_failures: 3,
    peer: null,
    error: null,
    pending: [],
    pending_store_unavailable: false,
    ...overrides,
  };
}

function wirePeer() {
  return {
    hostname: "MacBook",
    address: "192.168.1.10",
    fingerprint: "SHA256:peer",
    verification_code: "123456",
    local_confirmed: false,
    remote_confirmed: false,
  };
}

/** Serialized `CommandError`: a transparent, versioned StableErrorEnvelope. */
function commandError(code: string, overrides: Record<string, unknown> = {}) {
  return {
    schema_version: 1,
    code,
    retryable: false,
    message_key: "error.internal",
    detail_class: "internal",
    ...overrides,
  };
}

type Handler = (args?: Record<string, unknown>) => unknown;
let handlers: Record<string, Handler> = {};

function installHandlers(next: Record<string, Handler>) {
  handlers = next;
  tauriInvokeMock.mockImplementation((command: string, args?: Record<string, unknown>) => {
    const handler = handlers[command];
    if (!handler) return Promise.reject(new Error(`unexpected command ${command}`));
    return Promise.resolve().then(() => handler(args));
  });
}

function callsTo(command: string) {
  return tauriInvokeMock.mock.calls.filter(([name]) => name === command);
}

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (error: unknown) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  return { promise, resolve, reject };
}

let remoteLinkHandler: (() => void) | undefined;

function setLocale(locale: "en" | "zh-CN") {
  localStorage.setItem("tailsync-lang", locale);
}

async function settle() {
  await act(async () => {});
}

function PairingHarness() {
  const { t, locale, setLocale: applyLocale } = useI18n();
  const refreshDevices = useCallback(async () => undefined, []);
  const pairing = usePairing({ refreshDevices });
  return (
    <div>
      <button type="button" onClick={() => void pairing.openPairing(PEER)}>open-pairing</button>
      <button type="button" onClick={() => applyLocale(locale === "en" ? "zh-CN" : "en")}>switch-locale</button>
      <PairingDialog
        t={t}
        pairingOpen={pairing.pairingOpen}
        pairingStatus={pairing.pairingStatus}
        pairingTarget={pairing.pairingTarget}
        pairingError={pairing.pairingError}
        pairingBusy={pairing.pairingBusy}
        pairDialogRef={pairing.pairDialogRef}
        closePairing={pairing.closePairing}
        handlePair={pairing.handlePair}
      />
    </div>
  );
}

function RemoteHarness() {
  const { t } = useI18n();
  const remote = useRemotePairing();
  return (
    <RemotePairingPanel
      t={t}
      defaultExpanded
      invite={remote.invite}
      linkDraft={remote.linkDraft}
      linkPreview={remote.linkPreview}
      busy={remote.remotePairingBusy}
      error={remote.remotePairingError}
      copied={remote.copied}
      onCreateInvite={remote.handleCreateInvite}
      onLinkChange={remote.handleLinkChange}
      onInspectLink={remote.handleInspectLink}
      onStartPairing={remote.handleStartRemotePairing}
      onCancelInvite={remote.handleCancelInvite}
      onCopyInvite={remote.handleCopyInvite}
    />
  );
}

function linkInput() {
  // Only one editable text box exists while no invite is on screen, so the
  // helper stays locale independent.
  return screen.getByRole("textbox");
}

function remoteAlert() {
  return screen.queryByRole("alert");
}

beforeEach(() => {
  handlers = {};
  remoteLinkHandler = undefined;
  tauriInvokeMock.mockReset();
  listenMock.mockReset().mockImplementation(async (_event: string, handler: () => void) => {
    remoteLinkHandler = handler;
    return () => undefined;
  });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("pairing dialog failure consumption", () => {
  it("parent: an obsolete cancel cannot release the newer operation's polling guard", async () => {
    setLocale("en");
    const cancelled = deferred<Record<string, unknown>>();
    const nextStart = deferred<Record<string, unknown>>();
    let starts = 0;
    installHandlers({
      get_pairing_status: () => wireStatus(),
      start_pairing: () => ++starts === 1 ? wireStatus({ phase: "verification", peer: wirePeer() }) : nextStart.promise,
      cancel_pairing: () => cancelled.promise,
    });
    vi.useFakeTimers();
    render(<PairingHarness />);
    await settle();
    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    fireEvent.click(screen.getByRole("button", { name: en["settings.cancel"] }));
    await settle();
    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await act(async () => cancelled.resolve(wireStatus({ phase: "disabled", pairing_enabled: false })));
    const polls = callsTo("get_pairing_status").length;
    try {
      await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
      expect(callsTo("get_pairing_status")).toHaveLength(polls);
      expect(screen.getByText("123456")).toBeInTheDocument();
    } finally {
      await act(async () => nextStart.resolve(wireStatus({ phase: "verification", peer: wirePeer() })));
    }
  });

  it("parent: changing the input releases UI busy before the obsolete request returns", async () => {
    setLocale("en");
    const oldInspect = deferred<Record<string, unknown>>();
    installHandlers({
      take_pending_remote_pairing_link: () => null,
      inspect_remote_pairing_link: () => oldInspect.promise,
    });
    render(<RemoteHarness />);
    await settle();
    fireEvent.change(linkInput(), { target: { value: INVITE_LINK } });
    fireEvent.click(screen.getByRole("button", { name: en["settings.checkInvite"] }));
    await settle();
    expect(screen.getByRole("button", { name: en["settings.checkInvite"] })).toBeDisabled();
    fireEvent.change(linkInput(), { target: { value: "tailsync://pair/v1/replacement" } });
    try {
      expect(screen.getByRole("button", { name: en["settings.checkInvite"] })).not.toBeDisabled();
      expect(remoteAlert()).toBeNull();
    } finally {
      await act(async () => oldInspect.reject(commandError("pairing_timed_out")));
    }
    expect(remoteAlert()).toBeNull();
  });

  it("parent: an obsolete remote completion cannot finish the replacement request", async () => {
    setLocale("en");
    const first = deferred<Record<string, unknown>>();
    const second = deferred<Record<string, unknown>>();
    let inspections = 0;
    installHandlers({
      take_pending_remote_pairing_link: () => null,
      inspect_remote_pairing_link: () => ++inspections === 1 ? first.promise : second.promise,
    });
    render(<RemoteHarness />);
    await settle();
    fireEvent.change(linkInput(), { target: { value: INVITE_LINK } });
    fireEvent.click(screen.getByRole("button", { name: en["settings.checkInvite"] }));
    await settle();
    fireEvent.change(linkInput(), { target: { value: "tailsync://pair/v1/replacement" } });
    fireEvent.click(screen.getByRole("button", { name: en["settings.checkInvite"] }));
    await settle();
    expect(callsTo("inspect_remote_pairing_link")).toHaveLength(2);
    try {
      await act(async () => first.reject(commandError("pairing_timed_out")));
      expect(screen.getByRole("button", { name: en["settings.checkInvite"] })).toBeDisabled();
      expect(remoteAlert()).toBeNull();
    } finally {
      await act(async () => second.resolve({ endpoint_id: "replacement", expires_at: 1, remaining_seconds: 45 }));
    }
    expect(screen.getByRole("button", { name: en["settings.checkInvite"] })).not.toBeDisabled();
    expect(screen.getByText("Link is valid for 45s")).toBeInTheDocument();
  });

  it("renders the finite direct code instead of a stale status failure (en)", async () => {
    setLocale("en");
    let statusCalls = 0;
    installHandlers({
      get_pairing_status: () => {
        statusCalls += 1;
        return statusCalls === 1
          ? wireStatus()
          : wireStatus({ phase: "timed_out", failure_kind: "identity_mismatch", error: "Connection reset by peer" });
      },
      start_pairing: () => { throw commandError("pairing_window_closed"); },
    });
    render(<PairingHarness />);
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await settle();

    const alert = screen.getByRole("alert");
    expect(alert.textContent).toBe(en["error.pairing_window_closed"]);
    expect(alert.textContent).not.toContain("Connection reset by peer");
    expect(alert.textContent).not.toContain(en["error.pairing_identity_mismatch"]);
    expect(callsTo("start_pairing")).toHaveLength(1);
    expect(statusCalls).toBe(2);
  });

  it("renders the fixed policy text for the same code in zh-CN", async () => {
    setLocale("zh-CN");
    installHandlers({
      get_pairing_status: () => wireStatus(),
      start_pairing: () => { throw commandError("pairing_window_closed"); },
    });
    render(<PairingHarness />);
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await settle();

    expect(screen.getByRole("alert").textContent).toBe(zhCN["error.pairing_window_closed"]);
  });

  it("prefers the finite status failure_kind over contradictory status text", async () => {
    setLocale("en");
    let statusCalls = 0;
    installHandlers({
      get_pairing_status: () => {
        statusCalls += 1;
        return statusCalls === 1
          ? wireStatus({ phase: "verification", peer: wirePeer() })
          : wireStatus({ phase: "timed_out", failure_kind: "timed_out", error: "identity mismatch reported by the peer" });
      },
      start_pairing: () => wireStatus({ phase: "verification", peer: wirePeer() }),
    });
    vi.useFakeTimers();
    render(<PairingHarness />);
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });

    const alert = screen.getByRole("alert");
    expect(alert.textContent).toBe(en["error.pairing_timed_out"]);
    expect(alert.textContent).not.toContain("identity mismatch");
  });

  it("falls back to the legacy status text when the kind is missing", async () => {
    setLocale("en");
    let statusCalls = 0;
    installHandlers({
      get_pairing_status: () => {
        statusCalls += 1;
        return statusCalls === 1 ? wireStatus() : wireStatus({ phase: "timed_out", error: "Connection reset by peer" });
      },
      start_pairing: () => wireStatus({ phase: "verification", peer: wirePeer() }),
    });
    vi.useFakeTimers();
    render(<PairingHarness />);
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });

    expect(screen.getByRole("alert").textContent).toBe("Connection reset by peer");
  });

  it("keeps the legacy identity-mismatch fallback when the kind is null", async () => {
    setLocale("en");
    let statusCalls = 0;
    installHandlers({
      get_pairing_status: () => {
        statusCalls += 1;
        return statusCalls === 1
          ? wireStatus()
          : wireStatus({ phase: "timed_out", failure_kind: null, error: "identity mismatch while confirming" });
      },
      start_pairing: () => wireStatus({ phase: "verification", peer: wirePeer() }),
    });
    vi.useFakeTimers();
    render(<PairingHarness />);
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });

    expect(screen.getByRole("alert").textContent).toBe(en["settings.identityMismatch"]);
  });

  it("treats a future kind string as unknown and keeps the legacy text", async () => {
    setLocale("en");
    let statusCalls = 0;
    installHandlers({
      get_pairing_status: () => {
        statusCalls += 1;
        return statusCalls === 1
          ? wireStatus()
          : wireStatus({ phase: "timed_out", failure_kind: "future_kind_from_newer_core", error: "peer hung up" });
      },
      start_pairing: () => wireStatus({ phase: "verification", peer: wirePeer() }),
    });
    vi.useFakeTimers();
    render(<PairingHarness />);
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });

    const alert = screen.getByRole("alert");
    expect(alert.textContent).toBe("peer hung up");
    expect(alert.textContent).not.toBe(en["error.internal"]);
  });

  it("rejects a non-string kind and reports the malformed status", async () => {
    setLocale("en");
    let statusCalls = 0;
    installHandlers({
      get_pairing_status: () => {
        statusCalls += 1;
        return statusCalls === 1
          ? wireStatus()
          : wireStatus({ phase: "timed_out", failure_kind: 42, error: null });
      },
      start_pairing: () => wireStatus({ phase: "verification", peer: wirePeer() }),
    });
    vi.useFakeTimers();
    render(<PairingHarness />);
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });

    expect(screen.getByRole("alert").textContent).toContain("Invalid PairingStatus response");
  });

  it("maps a future stable code to the internal policy", async () => {
    setLocale("en");
    installHandlers({
      get_pairing_status: () => wireStatus(),
      start_pairing: () => { throw commandError("future_code_from_newer_core"); },
    });
    render(<PairingHarness />);
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await settle();

    expect(screen.getByRole("alert").textContent).toBe(en["error.internal"]);
  });

  it("re-resolves the fixed text from the finite code when the locale changes", async () => {
    setLocale("en");
    installHandlers({
      get_pairing_status: () => wireStatus(),
      start_pairing: () => { throw commandError("pairing_window_closed"); },
    });
    render(<PairingHarness />);
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await settle();
    expect(screen.getByRole("alert").textContent).toBe(en["error.pairing_window_closed"]);

    fireEvent.click(screen.getByRole("button", { name: "switch-locale" }));
    await settle();
    expect(screen.getByRole("alert").textContent).toBe(zhCN["error.pairing_window_closed"]);
  });

  it("keeps the direct typed error when the status poll then fails", async () => {
    setLocale("en");
    let statusCalls = 0;
    installHandlers({
      get_pairing_status: () => {
        statusCalls += 1;
        if (statusCalls === 1) return wireStatus();
        throw new Error("offline");
      },
      start_pairing: () => { throw commandError("pairing_busy"); },
    });
    vi.useFakeTimers();
    render(<PairingHarness />);
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await settle();
    expect(screen.getByRole("alert").textContent).toBe(en["error.pairing_busy"]);

    const statusBefore = callsTo("get_pairing_status").length;
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(callsTo("get_pairing_status").length).toBeGreaterThan(statusBefore);
    expect(screen.getByRole("alert").textContent).toBe(en["error.pairing_busy"]);
  });

  it("never retries a pairing mutation, even for a retryable code", async () => {
    setLocale("en");
    installHandlers({
      get_pairing_status: () => wireStatus(),
      start_pairing: () => { throw commandError("temporarily_busy", { retryable: true }); },
    });
    render(<PairingHarness />);
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await settle();

    expect(screen.getByRole("alert").textContent).toBe(en["error.temporarily_busy"]);
    expect(callsTo("start_pairing")).toHaveLength(1);
    expect(callsTo("confirm_pairing")).toHaveLength(0);
    expect(callsTo("cancel_pairing")).toHaveLength(0);
  });

  it("drops the previous direct error when a new operation succeeds", async () => {
    setLocale("en");
    let failStart = true;
    installHandlers({
      get_pairing_status: () => wireStatus(),
      start_pairing: () => {
        if (failStart) throw commandError("pairing_busy");
        return wireStatus({ phase: "verification", peer: wirePeer() });
      },
      cancel_pairing: () => wireStatus({ phase: "cancelled" }),
    });
    render(<PairingHarness />);
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await settle();
    expect(screen.getByRole("alert").textContent).toBe(en["error.pairing_busy"]);

    fireEvent.click(screen.getByRole("button", { name: en["settings.cancel"] }));
    await settle();
    await settle();
    expect(screen.queryByRole("alert")).toBeNull();

    failStart = false;
    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await settle();
    expect(screen.getByText("123456")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("clears a superseded direct error when pairing completes", async () => {
    let statusCalls = 0;
    installHandlers({
      get_pairing_status: () => {
        statusCalls += 1;
        return statusCalls === 1 ? wireStatus() : wireStatus({ phase: "paired" });
      },
      start_pairing: () => { throw commandError("pairing_busy"); },
    });
    vi.useFakeTimers();
    const refreshDevices = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => usePairing({ refreshDevices }));
    await settle();

    await act(async () => { await result.current.openPairing(PEER); });
    expect(result.current.pairingError).not.toBeNull();

    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(result.current.pairingError).toBeNull();
    expect(result.current.pairingOpen).toBe(false);
    expect(refreshDevices).toHaveBeenCalledTimes(1);
  });

  it("does not let an in-flight poll overwrite a newer start_pairing status", async () => {
    setLocale("en");
    const stalePoll = deferred<Record<string, unknown>>();
    let statusCalls = 0;
    installHandlers({
      get_pairing_status: () => {
        statusCalls += 1;
        return statusCalls === 1 ? wireStatus() : stalePoll.promise;
      },
      start_pairing: () => wireStatus({ phase: "verification", peer: wirePeer() }),
    });
    vi.useFakeTimers();
    render(<PairingHarness />);
    await settle();
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await settle();
    expect(screen.getByText("123456")).toBeInTheDocument();

    await act(async () => { stalePoll.resolve(wireStatus()); });
    await settle();
    expect(screen.getByText("123456")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("does not poll while a direct pairing operation owns the state", async () => {
    setLocale("en");
    const start = deferred<Record<string, unknown>>();
    let statusCalls = 0;
    installHandlers({
      get_pairing_status: () => { statusCalls += 1; return wireStatus(); },
      start_pairing: () => start.promise,
    });
    vi.useFakeTimers();
    render(<PairingHarness />);
    await settle();
    const before = statusCalls;

    fireEvent.click(screen.getByRole("button", { name: "open-pairing" }));
    await settle();
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(statusCalls).toBe(before);

    await act(async () => { start.resolve(wireStatus({ phase: "verification", peer: wirePeer() })); });
    await settle();
    expect(screen.getByText("123456")).toBeInTheDocument();
  });
});

describe("remote pairing panel failure consumption", () => {
  it("renders the finite remote code instead of the raw rejection (en)", async () => {
    setLocale("en");
    installHandlers({
      take_pending_remote_pairing_link: () => null,
      start_remote_pairing: () => {
        throw commandError("pairing_busy", { retryable: true, message_key: "peer at C:\\private\\device" });
      },
    });
    render(<RemoteHarness />);
    await settle();

    fireEvent.change(linkInput(), { target: { value: INVITE_LINK } });
    fireEvent.click(screen.getByRole("button", { name: en["settings.startRemotePairing"] }));
    await settle();
    await settle();

    const alert = remoteAlert();
    expect(alert?.textContent).toBe(en["error.pairing_busy"]);
    expect(alert?.textContent).not.toContain("private");
    expect(callsTo("start_remote_pairing")).toHaveLength(1);
  });

  it("renders the fixed policy text for the same remote code in zh-CN", async () => {
    setLocale("zh-CN");
    installHandlers({
      take_pending_remote_pairing_link: () => null,
      start_remote_pairing: () => { throw commandError("pairing_busy"); },
    });
    render(<RemoteHarness />);
    await settle();

    fireEvent.change(linkInput(), { target: { value: INVITE_LINK } });
    fireEvent.click(screen.getByRole("button", { name: zhCN["settings.startRemotePairing"] }));
    await settle();
    await settle();

    expect(remoteAlert()?.textContent).toBe(zhCN["error.pairing_busy"]);
  });

  it("ignores a stale inspect response after the link draft changed", async () => {
    setLocale("en");
    const inspect = deferred<Record<string, unknown>>();
    installHandlers({
      take_pending_remote_pairing_link: () => null,
      inspect_remote_pairing_link: () => inspect.promise,
    });
    render(<RemoteHarness />);
    await settle();

    fireEvent.change(linkInput(), { target: { value: INVITE_LINK } });
    fireEvent.click(screen.getByRole("button", { name: en["settings.checkInvite"] }));
    await settle();
    fireEvent.change(linkInput(), { target: { value: "tailsync://pair/v1/bbb" } });

    await act(async () => { inspect.resolve({ endpoint_id: "endpoint-a", expires_at: 1, remaining_seconds: 120 }); });
    await settle();

    expect(screen.queryByText(/Link is valid for/)).toBeNull();
    expect(remoteAlert()).toBeNull();
    expect(screen.getByRole("button", { name: en["settings.checkInvite"] })).not.toBeDisabled();
    expect(callsTo("inspect_remote_pairing_link")).toHaveLength(1);
    expect(callsTo("inspect_remote_pairing_link")[0][1]).toEqual({ link: INVITE_LINK });
  });

  it("ignores a stale inspect rejection after the link draft changed", async () => {
    setLocale("en");
    const inspect = deferred<Record<string, unknown>>();
    installHandlers({
      take_pending_remote_pairing_link: () => null,
      inspect_remote_pairing_link: () => inspect.promise,
    });
    render(<RemoteHarness />);
    await settle();

    fireEvent.change(linkInput(), { target: { value: INVITE_LINK } });
    fireEvent.click(screen.getByRole("button", { name: en["settings.checkInvite"] }));
    await settle();
    fireEvent.change(linkInput(), { target: { value: "tailsync://pair/v1/bbb" } });

    await act(async () => { inspect.reject(commandError("pairing_timed_out")); });
    await settle();

    expect(remoteAlert()).toBeNull();
    expect(screen.getByRole("button", { name: en["settings.checkInvite"] })).not.toBeDisabled();
  });

  it("lets a native pending link supersede an in-flight inspect", async () => {
    setLocale("en");
    const inspections: Array<Deferred<Record<string, unknown>>> = [];
    let pendingLink: string | null = null;
    installHandlers({
      take_pending_remote_pairing_link: () => pendingLink,
      inspect_remote_pairing_link: () => {
        const next = deferred<Record<string, unknown>>();
        inspections.push(next);
        return next.promise;
      },
    });
    render(<RemoteHarness />);
    await settle();

    fireEvent.change(linkInput(), { target: { value: INVITE_LINK } });
    fireEvent.click(screen.getByRole("button", { name: en["settings.checkInvite"] }));
    await settle();

    pendingLink = "tailsync://pair/v1/native";
    await act(async () => { remoteLinkHandler?.(); });
    await settle();
    expect(linkInput()).toHaveValue("tailsync://pair/v1/native");
    expect(inspections).toHaveLength(2);

    await act(async () => { inspections[0].resolve({ endpoint_id: "endpoint-a", expires_at: 1, remaining_seconds: 120 }); });
    await settle();
    expect(screen.queryByText("Link is valid for 120s")).toBeNull();

    await act(async () => { inspections[1].resolve({ endpoint_id: "endpoint-b", expires_at: 1, remaining_seconds: 300 }); });
    await settle();
    expect(screen.getByText("Link is valid for 300s")).toBeInTheDocument();
  });

  it("drops the previous remote error when a new operation succeeds", async () => {
    setLocale("en");
    let failCreate = true;
    installHandlers({
      take_pending_remote_pairing_link: () => null,
      create_remote_pairing_invite: () => {
        if (failCreate) throw commandError("pairing_busy");
        return { link: INVITE_LINK, expires_at: 1, remaining_seconds: 60 };
      },
    });
    render(<RemoteHarness />);
    await settle();

    fireEvent.click(screen.getByRole("button", { name: en["settings.createRemoteInvite"] }));
    await settle();
    await settle();
    expect(remoteAlert()?.textContent).toBe(en["error.pairing_busy"]);

    failCreate = false;
    fireEvent.click(screen.getByRole("button", { name: en["settings.createRemoteInvite"] }));
    await settle();
    await settle();

    expect(remoteAlert()).toBeNull();
    expect(screen.getByDisplayValue(INVITE_LINK)).toBeInTheDocument();
  });
});
