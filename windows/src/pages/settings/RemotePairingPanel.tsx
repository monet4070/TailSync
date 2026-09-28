import { useState } from "react";
import { ChevronRight, Clipboard, Globe, Link2, RefreshCw, X } from "lucide-react";
import type { RemotePairingInvite, RemotePairingInvitePreview } from "../../tailsyncClient";
import type { Translate } from "./SettingsSectionTypes";

interface RemotePairingPanelProps {
  t: Translate;
  invite: RemotePairingInvite | null;
  linkDraft: string;
  linkPreview: RemotePairingInvitePreview | null;
  busy: boolean;
  error: string;
  copied: boolean;
  defaultExpanded?: boolean;
  onCreateInvite: () => Promise<void>;
  onLinkChange: (value: string) => void;
  onInspectLink: () => Promise<void>;
  onStartPairing: () => Promise<void>;
  onCancelInvite: () => Promise<void>;
  onCopyInvite: () => Promise<void>;
}

export function RemotePairingPanel({
  t,
  invite,
  linkDraft,
  linkPreview,
  busy,
  error,
  copied,
  defaultExpanded = false,
  onCreateInvite,
  onLinkChange,
  onInspectLink,
  onStartPairing,
  onCancelInvite,
  onCopyInvite,
}: RemotePairingPanelProps) {
  const [expanded, setExpanded] = useState(defaultExpanded);

  return (
    <div className={`remote-pairing-panel remote-pairing-drawer${expanded ? " is-expanded" : ""}`}>
      <button
        type="button"
        className="remote-pairing-drawer-header"
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
      >
        <div className="remote-pairing-header-content">
          <Globe size={18} strokeWidth={1.7} className="remote-pairing-globe-icon" aria-hidden="true" />
          <div className="remote-pairing-header-text">
            <strong>{t("settings.remotePairing")}</strong>
            {!expanded && <span>{t("settings.remotePairingDescription")}</span>}
          </div>
        </div>
        <ChevronRight
          size={16}
          strokeWidth={2}
          className={`remote-pairing-chevron${expanded ? " expanded" : ""}`}
          aria-hidden="true"
        />
      </button>

      {expanded && (
        <div className="remote-pairing-body">
          <div className="remote-pairing-flow">
            <div className="remote-pairing-card">
              <div className="remote-card-header">
                <div className="remote-card-title-group">
                  <strong>{t("settings.createRemoteInvite")}</strong>
                  <span>{t("settings.createRemoteInviteDescription")}</span>
                </div>
                {!invite && (
                  <button
                    type="button"
                    className="pair-device-action remote-invite-create-action"
                    onClick={() => void onCreateInvite()}
                    disabled={busy}
                  >
                    {busy ? <RefreshCw className="spin" size={14} aria-hidden="true" /> : <Link2 size={14} aria-hidden="true" />}
                    {t("settings.createRemoteInvite")}
                  </button>
                )}
              </div>
              {invite && (
                <div className="remote-invite-result" role="status">
                  <input readOnly value={invite.link} aria-label={t("settings.remoteInviteLink")} />
                  <div className="remote-invite-actions">
                    <button
                      type="button"
                      className="icon-button"
                      onClick={() => void onCopyInvite()}
                      disabled={busy}
                      title={t(copied ? "settings.copied" : "settings.copyInvite")}
                      aria-label={t(copied ? "settings.copied" : "settings.copyInvite")}
                    >
                      <Clipboard size={14} aria-hidden="true" />
                    </button>
                    <button
                      type="button"
                      className="icon-button"
                      onClick={() => void onCancelInvite()}
                      disabled={busy}
                      title={t("settings.cancelRemoteInvite")}
                      aria-label={t("settings.cancelRemoteInvite")}
                    >
                      <X size={14} aria-hidden="true" />
                    </button>
                  </div>
                  <small>{t("settings.remoteInviteExpires").replace("{seconds}", String(invite.remaining_seconds))}</small>
                </div>
              )}
            </div>

            <div className="remote-pairing-card">
              <div className="remote-card-title-group">
                <strong>{t("settings.useRemoteInvite")}</strong>
                <span>{t("settings.useRemoteInviteDescription")}</span>
              </div>
              <input
                className="remote-pairing-input"
                type="text"
                value={linkDraft}
                onChange={(event) => onLinkChange(event.target.value)}
                placeholder={t("settings.remoteInvitePlaceholder")}
                aria-label={t("settings.remoteInviteLink")}
              />
              <div className="remote-pairing-actions">
                {linkPreview ? (
                  <small className="remote-pairing-preview" title={linkPreview.endpoint_id}>
                    {t("settings.remoteInviteValid").replace("{seconds}", String(linkPreview.remaining_seconds))}
                  </small>
                ) : <span />}
                <div className="remote-pairing-button-group">
                  <button
                    type="button"
                    className="secondary-action"
                    onClick={() => void onInspectLink()}
                    disabled={busy || !linkDraft.trim()}
                  >
                    {t("settings.checkInvite")}
                  </button>
                  <button
                    type="button"
                    className="pair-device-action"
                    onClick={() => void onStartPairing()}
                    disabled={busy || !linkDraft.trim()}
                  >
                    {t("settings.startRemotePairing")}
                  </button>
                </div>
              </div>
            </div>
          </div>

          {error && <p className="remote-pairing-error" role="alert">{error}</p>}
        </div>
      )}
    </div>
  );
}
