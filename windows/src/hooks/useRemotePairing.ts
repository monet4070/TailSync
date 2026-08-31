import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  cancelRemotePairingInvite,
  createRemotePairingInvite,
  inspectRemotePairingLink,
  startRemotePairing,
  takePendingRemotePairingLink,
  type RemotePairingInvite,
  type RemotePairingInvitePreview,
} from "../tailsyncClient";

export function useRemotePairing() {
  const [invite, setInvite] = useState<RemotePairingInvite | null>(null);
  const [linkDraft, setLinkDraft] = useState("");
  const [linkPreview, setLinkPreview] = useState<RemotePairingInvitePreview | null>(null);
  const [remotePairingBusy, setRemotePairingBusy] = useState(false);
  const [remotePairingError, setRemotePairingError] = useState("");
  const [copied, setCopied] = useState(false);

  const applyPendingLink = useCallback((link: string | null) => {
    if (!link) return;
    setLinkDraft(link);
    setRemotePairingError("");
    void inspectRemotePairingLink(link)
      .then(setLinkPreview)
      .catch(() => setLinkPreview(null));
  }, []);

  useEffect(() => {
    let active = true;
    void takePendingRemotePairingLink()
      .then((link) => { if (active) applyPendingLink(link); })
      .catch(() => undefined);
    let stop: (() => void) | undefined;
    void listen("remote-pairing-link-received", () => {
      void takePendingRemotePairingLink()
        .then((link) => { if (active) applyPendingLink(link); })
        .catch(() => undefined);
    }).then((unlisten) => {
      if (active) stop = unlisten;
      else unlisten();
    });
    return () => {
      active = false;
      stop?.();
    };
  }, [applyPendingLink]);

  const handleCreateInvite = useCallback(async () => {
    setRemotePairingBusy(true);
    setRemotePairingError("");
    try {
      const next = await createRemotePairingInvite();
      setInvite(next);
      setCopied(false);
    } catch (error) {
      setRemotePairingError(String(error));
    } finally {
      setRemotePairingBusy(false);
    }
  }, []);

  const handleLinkChange = useCallback((value: string) => {
    setLinkDraft(value);
    setLinkPreview(null);
    setRemotePairingError("");
  }, []);

  const handleInspectLink = useCallback(async () => {
    if (!linkDraft.trim()) return;
    setRemotePairingBusy(true);
    setRemotePairingError("");
    try {
      setLinkPreview(await inspectRemotePairingLink(linkDraft));
    } catch (error) {
      setLinkPreview(null);
      setRemotePairingError(String(error));
    } finally {
      setRemotePairingBusy(false);
    }
  }, [linkDraft]);

  const handleStartRemotePairing = useCallback(async () => {
    if (!linkDraft.trim()) return;
    setRemotePairingBusy(true);
    setRemotePairingError("");
    try {
      await startRemotePairing(linkDraft);
    } catch (error) {
      setRemotePairingError(String(error));
    } finally {
      setRemotePairingBusy(false);
    }
  }, [linkDraft]);

  const handleCancelInvite = useCallback(async () => {
    setRemotePairingBusy(true);
    try {
      await cancelRemotePairingInvite();
      setInvite(null);
    } catch (error) {
      setRemotePairingError(String(error));
    } finally {
      setRemotePairingBusy(false);
    }
  }, []);

  const handleCopyInvite = useCallback(async () => {
    if (!invite) return;
    try {
      await navigator.clipboard.writeText(invite.link);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1_500);
    } catch (error) {
      setRemotePairingError(String(error));
    }
  }, [invite]);

  return {
    invite,
    linkDraft,
    linkPreview,
    remotePairingBusy,
    remotePairingError,
    copied,
    handleCreateInvite,
    handleLinkChange,
    handleInspectLink,
    handleStartRemotePairing,
    handleCancelInvite,
    handleCopyInvite,
  };
}
