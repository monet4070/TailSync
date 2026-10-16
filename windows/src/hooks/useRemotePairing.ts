import { useCallback, useEffect, useRef, useState } from "react";
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
import { pairingIssueForError, type PairingIssue } from "../utils/pairingFailure";

export function useRemotePairing() {
  const [invite, setInvite] = useState<RemotePairingInvite | null>(null);
  const [linkDraft, setLinkDraft] = useState("");
  const [linkPreview, setLinkPreview] = useState<RemotePairingInvitePreview | null>(null);
  const [remotePairingBusy, setRemotePairingBusy] = useState(false);
  const [remotePairingError, setRemotePairingError] = useState<PairingIssue | null>(null);
  const [copied, setCopied] = useState(false);
  // Bumped by every user-visible transition (draft change, new operation), so
  // a superseded callback cannot write a preview, message or invite.
  const generation = useRef(0);

  const beginOperation = useCallback(() => {
    const current = generation.current + 1;
    generation.current = current;
    setRemotePairingBusy(true);
    return current;
  }, []);

  const endOperation = useCallback((current: number) => {
    if (generation.current === current) setRemotePairingBusy(false);
  }, []);

  const applyPendingLink = useCallback((link: string | null) => {
    if (!link) return;
    const current = generation.current + 1;
    generation.current = current;
    setRemotePairingBusy(false);
    setLinkDraft(link);
    setLinkPreview(null);
    setRemotePairingError(null);
    void inspectRemotePairingLink(link)
      .then((preview) => { if (generation.current === current) setLinkPreview(preview); })
      .catch(() => { if (generation.current === current) setLinkPreview(null); });
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
    const current = beginOperation();
    setRemotePairingError(null);
    try {
      const next = await createRemotePairingInvite();
      if (generation.current !== current) return;
      setInvite(next);
      setCopied(false);
    } catch (error) {
      if (generation.current !== current) return;
      setRemotePairingError(pairingIssueForError(error));
    } finally {
      endOperation(current);
    }
  }, [beginOperation, endOperation]);

  const handleLinkChange = useCallback((value: string) => {
    // Editing the draft invalidates any response still in flight for the
    // previous link, including a pending cold-start inspection.
    generation.current += 1;
    setRemotePairingBusy(false);
    setLinkDraft(value);
    setLinkPreview(null);
    setRemotePairingError(null);
  }, []);

  const handleInspectLink = useCallback(async () => {
    if (!linkDraft.trim()) return;
    const current = beginOperation();
    setRemotePairingError(null);
    try {
      const preview = await inspectRemotePairingLink(linkDraft);
      if (generation.current !== current) return;
      setLinkPreview(preview);
    } catch (error) {
      if (generation.current !== current) return;
      setLinkPreview(null);
      setRemotePairingError(pairingIssueForError(error));
    } finally {
      endOperation(current);
    }
  }, [beginOperation, endOperation, linkDraft]);

  const handleStartRemotePairing = useCallback(async () => {
    if (!linkDraft.trim()) return;
    const current = beginOperation();
    setRemotePairingError(null);
    try {
      await startRemotePairing(linkDraft);
    } catch (error) {
      if (generation.current !== current) return;
      setRemotePairingError(pairingIssueForError(error));
    } finally {
      endOperation(current);
    }
  }, [beginOperation, endOperation, linkDraft]);

  const handleCancelInvite = useCallback(async () => {
    const current = beginOperation();
    setRemotePairingError(null);
    try {
      await cancelRemotePairingInvite();
      if (generation.current !== current) return;
      setInvite(null);
    } catch (error) {
      if (generation.current !== current) return;
      setRemotePairingError(pairingIssueForError(error));
    } finally {
      endOperation(current);
    }
  }, [beginOperation, endOperation]);

  const handleCopyInvite = useCallback(async () => {
    if (!invite) return;
    try {
      await navigator.clipboard.writeText(invite.link);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1_500);
    } catch (error) {
      setRemotePairingError(pairingIssueForError(error));
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
