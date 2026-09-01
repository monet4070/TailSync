import { Clipboard, Link2, RefreshCw, X } from "lucide-react";
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
  onCreateInvite,
  onLinkChange,
  onInspectLink,
  onStartPairing,
  onCancelInvite,
  onCopyInvite,
}: RemotePairingPanelProps) {
  return (
    <div className="remote-pairing-panel">
      <div className="remote-pairing-heading">
        <div>
          <strong>{t("settings.remotePairing")}</strong>
          <span>{t("settings.remotePairingDescription")}</span>
        </div>
        <Link2 size={16} strokeWidth={1.7} aria-hidden="true" />
      </div>

      <div className="remote-pairing-columns">
        <div className="remote-pairing-card">
          <strong>{t("settings.createRemoteInvite")}</strong>
          <span>{t("settings.createRemoteInviteDescription")}</span>
          <button type="button" className="pair-device-action remote-invite-create-action" onClick={() => void onCreateInvite()} disabled={busy}>
            {busy && !invite ? <RefreshCw className="spin" size={14} aria-hidden="true" /> : <Link2 size={14} aria-hidden="true" />}
            {t("settings.createRemoteInvite")}
          </button>
          {invite && (
            <div className="remote-invite-result" role="status">
              <input readOnly value={invite.link} aria-label={t("settings.remoteInviteLink")} />
              <div className="remote-invite-actions">
                <button type="button" className="icon-button" onClick={() => void onCopyInvite()} disabled={busy} title={t(copied ? "settings.copied" : "settings.copyInvite")} aria-label={t(copied ? "settings.copied" : "settings.copyInvite")}>
                  <Clipboard size={14} aria-hidden="true" />
                </button>
                <button type="button" className="icon-button" onClick={() => void onCancelInvite()} disabled={busy} title={t("settings.cancelRemoteInvite")} aria-label={t("settings.cancelRemoteInvite")}>
                  <X size={14} aria-hidden="true" />
                </button>
              </div>
              <small>{t("settings.remoteInviteExpires").replace("{seconds}", String(invite.remaining_seconds))}</small>
            </div>
          )}
        </div>

        <div className="remote-pairing-card">
          <strong>{t("settings.useRemoteInvite")}</strong>
          <span>{t("settings.useRemoteInviteDescription")}</span>
          <input
            className="remote-pairing-input"
            type="text"
            value={linkDraft}
            onChange={(event) => onLinkChange(event.target.value)}
            placeholder={t("settings.remoteInvitePlaceholder")}
            aria-label={t("settings.remoteInviteLink")}
          />
          <div className="remote-pairing-actions">
            <button type="button" className="secondary-action" onClick={() => void onInspectLink()} disabled={busy || !linkDraft.trim()}>
              {t("settings.checkInvite")}
            </button>
            <button type="button" className="pair-device-action" onClick={() => void onStartPairing()} disabled={busy || !linkDraft.trim()}>
              {t("settings.startRemotePairing")}
            </button>
          </div>
          {linkPreview && (
            <small className="remote-pairing-preview" title={linkPreview.endpoint_id}>
              {t("settings.remoteInviteValid").replace("{seconds}", String(linkPreview.remaining_seconds))}
            </small>
          )}
        </div>
      </div>

      {error && <p className="remote-pairing-error" role="alert">{error}</p>}
    </div>
  );
}
