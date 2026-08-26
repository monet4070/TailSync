// Pairing feature hook (T258 extraction from Settings.tsx).
//
// Owns the pairing dialog state machine, the 1s status polling that
// auto-opens the dialog during verification, the dialog focus trap, and
  // the enable/start/cancel/confirm handlers. The devices refresh that must
// run after a successful pairing is injected through options so the hook
// stays independent of the settings page's device list.

import { useEffect, useRef, useState } from "react";
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

export interface PairingOptions {
  refreshDevices: () => Promise<void>;
}

export function usePairing(options: PairingOptions) {
  const { refreshDevices } = options;
  const [pairingTarget, setPairingTarget] = useState<PeerDevice | null>(null);
  const [pairingStatus, setPairingStatus] = useState<PairingStatus | null>(null);
  const [pairingOpen, setPairingOpen] = useState(false);
  const [pairingError, setPairingError] = useState("");
  const [pairingBusy, setPairingBusy] = useState(false);
  const previousPairingPhase = useRef<PairingStatus["phase"] | null>(null);
  const pairingBusyRef = useRef(pairingBusy);
  const pairDialogRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const refreshDevicesRef = useRef(refreshDevices);
  refreshDevicesRef.current = refreshDevices;

  useEffect(() => {
    pairingBusyRef.current = pairingBusy;
  }, [pairingBusy]);

  const handleEnablePairing = async () => {
    setPairingBusy(true);
    setPairingError("");
    try {
      const status = await enablePairing();
      setPairingStatus(status);
      setPairingOpen(true);
    } catch (error) {
      setPairingError(String(error));
    } finally {
      setPairingBusy(false);
    }
  };

  const openPairing = async (peer: PeerDevice) => {
    const address = pairingAddressForPeer(peer);
    if (!address) return;
    setPairingTarget(peer);
    setPairingOpen(true);
    setPairingBusy(true);
    setPairingError("");
    try {
      if (!pairingStatus?.pairing_enabled) {
        setPairingStatus(await enablePairing());
      }
      const status = await startPairing(address);
      setPairingStatus(status);
    } catch (error) {
      setPairingError(String(error));
      try {
        setPairingStatus(await getPairingStatus());
      } catch {
        // Preserve the original pairing error.
      }
    } finally {
      setPairingBusy(false);
    }
  };

  const closePairing = async () => {
    if (pairingBusyRef.current) return;
    pairingBusyRef.current = true;
    setPairingBusy(true);
    try {
      setPairingStatus(await cancelPairing());
      setPairingOpen(false);
      setPairingTarget(null);
    } catch (error) {
      setPairingError(String(error));
    } finally {
      pairingBusyRef.current = false;
      setPairingBusy(false);
    }
  };

  const handlePair = async () => {
    if (!pairingStatus?.peer) return;
    setPairingBusy(true);
    setPairingError("");
    try {
      setPairingStatus(await confirmPairing());
    } catch (error) {
      setPairingError(String(error));
    } finally {
      setPairingBusy(false);
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
      try {
        const status = await getPairingStatus();
        if (!active) return;
        setPairingStatus(status);
        if (status.peer && ["verification", "waiting_for_peer", "finalizing"].includes(status.phase)) {
          setPairingOpen(true);
        }
        if (status.phase === "paired" && previousPairingPhase.current !== "paired") {
          setPairingOpen(false);
          setPairingTarget(null);
          void refreshDevicesRef.current();
        }
        previousPairingPhase.current = status.phase;
      } catch (error) {
        if (active) setPairingError(String(error));
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
    pairingError,
    pairingBusy,
    pairDialogRef,
    handleEnablePairing,
    openPairing,
    closePairing,
    handlePair,
  };
}
