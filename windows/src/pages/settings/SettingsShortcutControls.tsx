import { Keyboard, Pencil, RotateCcw, X } from "lucide-react";
import { shortcutKeycaps } from "../../utils/shortcut";
import type { ShortcutRecorder } from "./SettingsSectionTypes";

export function ShortcutSettingRow({
  recorder,
  currentShortcut,
  defaultShortcut,
  title,
  description,
  recordLabel,
  t,
  disabled = false,
}: {
  recorder: ShortcutRecorder;
  currentShortcut: string;
  defaultShortcut: string;
  title: string;
  description: string;
  recordLabel: string;
  t: (key: string) => string;
  disabled?: boolean;
}) {
  return (
    <div className="setting-row shortcut-row">
      <div className="setting-row-info">
        <span>{title}</span>
        <small>{description}</small>
      </div>
      <div className="shortcut-control">
        <button
          ref={recorder.shortcutTriggerRef}
          type="button"
          className="shortcut-recorder"
          disabled={disabled || recorder.shortcutBusy}
          onClick={() => void recorder.startShortcutRecording()}
          aria-haspopup="dialog"
          aria-expanded={recorder.shortcutRecording}
          aria-label={recordLabel}
        >
          <Keyboard size={16} strokeWidth={1.7} aria-hidden="true" />
          {shortcutKeycaps(recorder.shortcutDraft).length > 0 ? (
            <span className="shortcut-keycaps" aria-label={recorder.shortcutDraft}>
              {shortcutKeycaps(recorder.shortcutDraft).map((key, index) => (
                <kbd key={`${key}-${index}`}>{key}</kbd>
              ))}
            </span>
          ) : (
            <span className="shortcut-empty">{t("settings.shortcutDisabled")}</span>
          )}
          <Pencil className="shortcut-edit-icon" size={13} strokeWidth={1.8} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="shortcut-icon-button"
          disabled={disabled || recorder.shortcutBusy || recorder.shortcutRecording || recorder.shortcutDraft === defaultShortcut}
          onClick={() => {
            recorder.setShortcutDraft(defaultShortcut);
            void recorder.commitShortcut(defaultShortcut);
          }}
          title={t("settings.shortcutReset")}
          aria-label={t("settings.shortcutReset")}
        >
          <RotateCcw size={15} strokeWidth={1.8} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="shortcut-icon-button"
          disabled={disabled || recorder.shortcutBusy || recorder.shortcutRecording || !currentShortcut}
          onClick={() => {
            recorder.setShortcutDraft("");
            void recorder.commitShortcut("");
          }}
          title={t("settings.shortcutClear")}
          aria-label={t("settings.shortcutClear")}
        >
          <X size={15} strokeWidth={1.8} aria-hidden="true" />
        </button>
      </div>
    </div>
  );
}

export function ShortcutRecorderDialog({
  recorder,
  title,
  prompt,
  t,
}: {
  recorder: ShortcutRecorder;
  title: string;
  prompt: string;
  t: (key: string) => string;
}) {
  if (!recorder.shortcutRecording) return null;
  const titleId = `shortcut-dialog-title-${title.replace(/\s+/g, "-")}`;
  return (
    <div className="dialog-backdrop" onMouseDown={() => void recorder.cancelShortcutRecording()}>
      <div
        className="shortcut-dialog"
        ref={recorder.shortcutDialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="shortcut-dialog-header">
          <div className="shortcut-dialog-icon">
            <Keyboard size={20} strokeWidth={1.7} aria-hidden="true" />
          </div>
          <div>
            <h2 id={titleId}>{title}</h2>
            <p>{prompt}</p>
          </div>
        </div>
        <button
          ref={recorder.shortcutCaptureRef}
          type="button"
          className={`shortcut-capture-target${recorder.shortcutCaptureActive ? " active" : " captured"}`}
          onClick={recorder.restartShortcutCapture}
          onKeyDown={recorder.handleShortcutCaptureEvent}
          disabled={recorder.shortcutBusy}
          aria-label={recorder.shortcutCaptureActive
            ? t("settings.shortcutRecording")
            : t("settings.shortcutRecordAgain")}
        >
          {recorder.shortcutPreviewKeys.length > 0 ? (
            <span className="shortcut-keycaps shortcut-dialog-keycaps">
              {recorder.shortcutPreviewKeys.map((key, index) => (
                <kbd key={`${key}-${index}`}>{key}</kbd>
              ))}
            </span>
          ) : (
            <span className="shortcut-capture-placeholder">{t("settings.shortcutRecording")}</span>
          )}
          {!recorder.shortcutCaptureActive && (
            <span className="shortcut-capture-again">{t("settings.shortcutRecordAgain")}</span>
          )}
        </button>
        <div className="shortcut-dialog-message" aria-live="polite">
          {recorder.shortcutDialogError && (
            <span className="error" role="alert">{recorder.shortcutDialogError}</span>
          )}
          {!recorder.shortcutDialogError && recorder.shortcutCandidate && (
            <span className="ready">{t("settings.shortcutCaptured")}</span>
          )}
        </div>
        <div className="shortcut-dialog-actions">
          <button
            type="button"
            onClick={() => void recorder.cancelShortcutRecording()}
            disabled={recorder.shortcutBusy}
          >
            {t("settings.shortcutCancel")}
          </button>
          <button
            type="button"
            className="primary"
            onClick={() => void recorder.confirmShortcut()}
            disabled={!recorder.shortcutCandidate || recorder.shortcutBusy}
          >
            {t("settings.shortcutSave")}
          </button>
        </div>
      </div>
    </div>
  );
}
