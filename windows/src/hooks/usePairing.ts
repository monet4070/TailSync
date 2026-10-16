// Pairing feature hook (T258 extraction from Settings.tsx).
//
// Owns the pairing dialog state machine, the 1s status polling that
// auto-opens the dialog during verification, the dialog focus trap, and
// the enable/start/cancel/confirm handlers. The devices refresh that must
// run after a successful pairing is injected through options so the hook
// stays independent of the settings page's device list.

import { useEffect, useEffectEvent, useRef, useState } from "react";
import {
  cancelPairing,
  confirmPairing,
  enablePairing,
  getPairingStatus,
  startPairing,
  type PairingStatus,
  type PeerDevice,
} from "../tailsyncClient";
import { pairingAddressForPeer } from "../utils/pairingAddress";
import {
  pairingIssueForError,
  pairingIssueForStatus,
  type PairingIssue,
} from "../utils/pairingFailure";

export interface PairingOptions {
  refreshDevices: () => Promise<void>;
}

export function usePairing(options: PairingOptions) {
  const { refreshDevices } = options;
  const [pairingTarget, setPairingTarget] = useState<PeerDevice | null>(null);
  const [pairingStatus, setPairingStatus] = useState<PairingStatus | null>(null);
  const [pairingOpen, setPairingOpen] = useState(false);
  const [directError, setDirectError] = useState<PairingIssue | null>(null);
  const [statusError, setStatusError] = useState<PairingIssue | null>(null);
  const [pairingBusy, setPairingBusy] = useState(false);
  const previousPairingPhase = useRef<PairingStatus["phase"] | null>(null);
  const pairingBusyRef = useRef(pairingBusy);
  const pairDialogRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  // Bumped whenever a direct command starts, so an in-flight poll cannot
  // overwrite the status that command just produced.
  const operationGeneration = useRef(0);
  const refreshDeviceList = useEffectEvent(refreshDevices);

  const setBusy = (value: boolean) => {
    pairingBusyRef.current = value;
    setPairingBusy(value);
  };

  /** Applies a status returned by a command or poll, refreshing its issue. */
  const applyStatus = (status: PairingStatus) => {
    setPairingStatus(status);
    setStatusError(pairingIssueForStatus(status));
  };

  /** Starts a direct operation: supersedes in-flight polls and older errors. */
  const beginOperation = () => {
    operationGeneration.current += 1;
    setDirectError(null);
    return operationGeneration.current;
  };

  const superseded = (generation: number) => operationGeneration.current !== generation;

  const handleEnablePairing = async () => {
    const generation = beginOperation();
    setBusy(true);
    try {
      const status = await enablePairing();
      if (superseded(generation)) return;
      applyStatus(status);
      setPairingOpen(true);
    } catch (error) {
      if (superseded(generation)) return;
      setDirectError(pairingIssueForError(error));
    } finally {
      if (!superseded(generation)) setBusy(false);
    }
  };

  const openPairing = async (peer: PeerDevice) => {
    const address = pairingAddressForPeer(peer);
    if (!address) return;
    const generation = beginOperation();
    setPairingTarget(peer);
    setPairingOpen(true);
    setBusy(true);
    try {
      if (!pairingStatus?.pairing_enabled) {
        const enabled = await enablePairing();
        if (superseded(generation)) return;
        applyStatus(enabled);
      }
      const status = await startPairing(address);
      if (superseded(generation)) return;
      applyStatus(status);
    } catch (error) {
      if (superseded(generation)) return;
      setDirectError(pairingIssueForError(error));
      try {
        const status = await getPairingStatus();
        if (superseded(generation)) return;
        applyStatus(status);
      } catch {
        // Preserve the original pairing error.
      }
    } finally {
      if (!superseded(generation)) setBusy(false);
    }
  };

  const closePairing = async () => {
    if (pairingBusyRef.current) return;
    const generation = beginOperation();
    setBusy(true);
    try {
      const status = await cancelPairing();
      if (superseded(generation)) return;
      applyStatus(status);
      setPairingOpen(false);
      setPairingTarget(null);
    } catch (error) {
      if (superseded(generation)) return;
      setDirectError(pairingIssueForError(error));
    } finally {
      if (!superseded(generation)) setBusy(false);
    }
  };

  const handlePair = async () => {
    if (!pairingStatus?.peer) return;
    const generation = beginOperation();
    setBusy(true);
    try {
      const status = await confirmPairing();
      if (superseded(generation)) return;
      applyStatus(status);
    } catch (error) {
      if (superseded(generation)) return;
      setDirectError(pairingIssueForError(error));
    } finally {
      if (!superseded(generation)) setBusy(false);
    }
  };

  // Dialog focus trap: capture focus on open, restore it on close.
  useEffect(() => {
    if (!pairingOpen) return;
    const dialog = pairDialogRef.current;
    if (!dialog) return;
    previousFocus.current = document.activeElement as HTMLElement | null;
    const focusableSelector = [
      "button:not([disabled])",
      "[href]",
      "input:not([disabled])",
      "select:not([disabled])",
      "textarea:not([disabled])",
      "[tabindex]:not([tabindex='-1'])",
    ].join(",");
    const focusFirst = () => {
      const focusable = dialog.querySelectorAll<HTMLElement>(focusableSelector);
      (focusable[0] ?? dialog).focus();
    };
    const frame = window.requestAnimationFrame(focusFirst);
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        void closePairing();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = [...dialog.querySelectorAll<HTMLElement>(focusableSelector)];
      if (focusable.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      window.cancelAnimationFrame(frame);
      document.removeEventListener("keydown", handleKeyDown);
      previousFocus.current?.focus();
      previousFocus.current = null;
    };
  }, [pairingOpen]);

  // Status polling: keeps the dialog in sync and refreshes the device list
  // the moment a pairing completes.
  useEffect(() => {
    let active = true;
    const poll = async () => {
      // A direct command owns the pairing state while it runs; polling it
      // would race the command's own response.
      if (pairingBusyRef.current) return;
      const generation = operationGeneration.current;
      try {
        const status = await getPairingStatus();
        if (!active) return;
        if (superseded(generation) || pairingBusyRef.current) return;
        applyStatus(status);
        if (status.peer && ["verification", "waiting_for_peer", "finalizing"].includes(status.phase)) {
          setPairingOpen(true);
        }
        if (status.phase === "paired" && previousPairingPhase.current !== "paired") {
          setPairingOpen(false);
          setPairingTarget(null);
          setDirectError(null);
          void refreshDeviceList();
        }
        previousPairingPhase.current = status.phase;
      } catch (error) {
        if (!active || superseded(generation) || pairingBusyRef.current) return;
        setStatusError(pairingIssueForError(error));
      }
    };
    void poll();
    const timer = window.setInterval(poll, 1000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, []);

  return {
    pairingTarget,
    pairingStatus,
    pairingOpen,
    pairingError: directError ?? statusError,
    pairingBusy,
    pairDialogRef,
    handleEnablePairing,
    openPairing,
    closePairing,
    handlePair,
  };
}
