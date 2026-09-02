import { Settings2 } from "lucide-react";
import { ThemePackagePreview } from "../../components/ThemePackagePreview";
import type {
  PairingDialogProps,
  ThemeImportDialogProps,
} from "./SettingsSectionTypes";

export function PairingDialog({
  t,
  pairingOpen,
  pairingStatus,
  pairingTarget,
  pairingError,
  pairingBusy,
  pairDialogRef,
  closePairing,
  handlePair,
}: PairingDialogProps) {
  if (!pairingOpen) return null;
  const rawPairingError = pairingError || pairingStatus?.error || "";
  const visiblePairingError = /identity\s+mismatch/i.test(rawPairingError)
    ? t("settings.identityMismatch")
    : rawPairingError;

  return (
    <div className="dialog-backdrop" onMouseDown={() => void closePairing()}>
      <div
        className="pair-dialog"
        ref={pairDialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="pair-dialog-title"
        aria-describedby="pair-dialog-status"
        tabIndex={-1}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="confirm-dialog-icon pair-dialog-icon">
          <Settings2 size={22} strokeWidth={1.6} aria-hidden="true" />
        </div>
        <h2 id="pair-dialog-title">
          {t("settings.devicePairing")}
          {(pairingStatus?.peer?.hostname || pairingTarget?.hostname) && ` · ${pairingStatus?.peer?.hostname || pairingTarget?.hostname}`}
        </h2>
        {pairingStatus?.peer ? (
          <>
            <div className="pairing-code" aria-label={t("settings.pairingCode")}>
              {pairingStatus.peer.verification_code}
            </div>
            <p className="pairing-check-copy" id="pair-dialog-status">
              {t("settings.compareCode")}
            </p>
            <div className="pairing-peer-fingerprint">{pairingStatus.peer.fingerprint}</div>
            {pairingStatus.phase === "finalizing" ? (
              <p className="pairing-progress">{t("settings.pairingFinalizing")}</p>
            ) : pairingStatus.phase === "waiting_for_peer" && (
              <p className="pairing-progress">{t("settings.waitingPeerConfirm")}</p>
            )}
          </>
        ) : (
          <div className="pairing-waiting">
            {pairingBusy || pairingStatus?.phase === "handshaking" ? (
              <>
                <span className="pairing-spinner" />
                <p id="pair-dialog-status">{t("settings.secureHandshake")}</p>
              </>
            ) : (
              <div className="pairing-instruction">
                <span>{t("settings.pairingReady")}</span>
                <strong id="pair-dialog-status">{t("settings.pairingInstruction")}</strong>
                <small>
                  {t("settings.pairingExpiresPrefix")} {pairingStatus?.remaining_seconds ?? 0}{" "}
                  {t("settings.pairingExpiresSuffix")}
                </small>
              </div>
            )}
          </div>
        )}
        {visiblePairingError && (
          <p className="pair-dialog-error" role="alert">{visiblePairingError}</p>
        )}
        <div className="confirm-dialog-actions">
          <button type="button" onClick={() => void closePairing()} disabled={pairingBusy}>
            {t("settings.cancel")}
          </button>
          <button
            type="button"
            className="pair-submit"
            onClick={() => void handlePair()}
            disabled={pairingBusy || pairingStatus?.phase === "finalizing" || !pairingStatus?.peer || pairingStatus.peer.local_confirmed}
          >
            {t(pairingStatus?.peer?.local_confirmed
              ? "settings.confirmed"
              : "settings.codesMatch")}
          </button>
        </div>
      </div>
    </div>
  );
}

export function ThemeImportDialog({
  t,
  pendingThemeImport,
  setPendingThemeImport,
  confirmThemeImport,
}: ThemeImportDialogProps) {
  if (!pendingThemeImport) return null;

  return (
    <div className="dialog-backdrop" onMouseDown={() => setPendingThemeImport(null)}>
      <div
        className="shortcut-dialog theme-import-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={t("settings.themePreviewTitle")}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="shortcut-dialog-header">
          <div><h2>{t("settings.themePreviewTitle")}</h2></div>
        </div>
        <div className="theme-package-preview-modes">
          {[
            {
              label: t("settings.themePreviewLight"),
              resolved: pendingThemeImport.previews.light,
            },
            {
              label: t("settings.themePreviewDark"),
              resolved: pendingThemeImport.previews.dark,
            },
            {
              label: t("settings.themePreviewHighContrastLight"),
              resolved: pendingThemeImport.previews.highContrastLight,
            },
            {
              label: t("settings.themePreviewHighContrastDark"),
              resolved: pendingThemeImport.previews.highContrastDark,
            },
          ].map(({ label, resolved }) => (
            <ThemePackagePreview
              key={label}
              label={label}
              resolved={resolved}
              path={pendingThemeImport.path}
              digest={pendingThemeImport.digest}
              stateLabels={{
                default: t("settings.themePreviewStateDefault"),
                hover: t("settings.themePreviewStateHover"),
                active: t("settings.themePreviewStateActive"),
                selected: t("settings.themePreviewStateSelected"),
                disabled: t("settings.themePreviewStateDisabled"),
                focus: t("settings.themePreviewStateFocus"),
              }}
            />
          ))}
        </div>
        <div className="shortcut-dialog-message" role="status">
          {pendingThemeImport.operation.kind === "update"
            ? `${pendingThemeImport.operation.installedVersion} → ${pendingThemeImport.candidateVersion}`
            : `${t("settings.customThemeCandidateVersion")}: ${pendingThemeImport.candidateVersion}`}
        </div>
        {pendingThemeImport.diagnostics.length > 0 && (
          <div className="shortcut-dialog-message" role="status">
            {pendingThemeImport.diagnostics.map((diagnostic) => (
              <div key={`${diagnostic.code}-${diagnostic.message}`}>{diagnostic.message}</div>
            ))}
          </div>
        )}
        <div className="confirm-dialog-actions">
          <button type="button" onClick={() => setPendingThemeImport(null)}>{t("settings.cancel")}</button>
          <button type="button" className="pair-submit" onClick={() => void confirmThemeImport()}>
            {t(pendingThemeImport.operation.kind === "install" ? "settings.customThemeInstall" : "settings.customThemeUpdate")}
          </button>
        </div>
      </div>
    </div>
  );
}
