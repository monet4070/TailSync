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
  const {
    shortcutTriggerRef,
    shortcutBusy,
    shortcutRecording,
    shortcutDraft,
    startShortcutRecording,
    setShortcutDraft,
    commitShortcut,
  } = recorder;
  return (
    <div className="setting-row shortcut-row">
      <div className="setting-row-info">
        <span>{title}</span>
        <small>{description}</small>
      </div>
      <div className="shortcut-control">
        <button
          ref={shortcutTriggerRef}
          type="button"
          className="shortcut-recorder"
          disabled={disabled || shortcutBusy}
          onClick={() => void startShortcutRecording()}
          aria-haspopup="dialog"
          aria-expanded={shortcutRecording}
          aria-label={recordLabel}
        >
          <Keyboard size={16} strokeWidth={1.7} aria-hidden="true" />
          {shortcutKeycaps(shortcutDraft).length > 0 ? (
            <span className="shortcut-keycaps" aria-label={shortcutDraft}>
              {shortcutKeycaps(shortcutDraft).map((key, index) => (
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
          disabled={disabled || shortcutBusy || shortcutRecording || shortcutDraft === defaultShortcut}
          onClick={() => {
            setShortcutDraft(defaultShortcut);
            void commitShortcut(defaultShortcut);
          }}
          title={t("settings.shortcutReset")}
          aria-label={t("settings.shortcutReset")}
        >
          <RotateCcw size={15} strokeWidth={1.8} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="shortcut-icon-button"
          disabled={disabled || shortcutBusy || shortcutRecording || !currentShortcut}
          onClick={() => {
            setShortcutDraft("");
            void commitShortcut("");
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
  const {
    shortcutRecording,
    shortcutDialogRef,
    shortcutCaptureRef,
    shortcutCaptureActive,
    restartShortcutCapture,
    handleShortcutCaptureEvent,
    shortcutBusy,
    shortcutPreviewKeys,
    shortcutDialogError,
    shortcutCandidate,
    cancelShortcutRecording,
    confirmShortcut,
  } = recorder;
  if (!shortcutRecording) return null;
  const titleId = `shortcut-dialog-title-${title.replace(/\s+/g, "-")}`;
  return (
    <div className="dialog-backdrop" onMouseDown={() => void cancelShortcutRecording()}>
      <div
        className="shortcut-dialog"
        ref={shortcutDialogRef}
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
          ref={shortcutCaptureRef}
          type="button"
          className={`shortcut-capture-target${shortcutCaptureActive ? " active" : " captured"}`}
          onClick={restartShortcutCapture}
          onKeyDown={handleShortcutCaptureEvent}
          disabled={shortcutBusy}
          aria-label={shortcutCaptureActive
            ? t("settings.shortcutRecording")
            : t("settings.shortcutRecordAgain")}
        >
          {shortcutPreviewKeys.length > 0 ? (
            <span className="shortcut-keycaps shortcut-dialog-keycaps">
              {shortcutPreviewKeys.map((key, index) => (
                <kbd key={`${key}-${index}`}>{key}</kbd>
              ))}
            </span>
          ) : (
            <span className="shortcut-capture-placeholder">{t("settings.shortcutRecording")}</span>
          )}
          {!shortcutCaptureActive && (
            <span className="shortcut-capture-again">{t("settings.shortcutRecordAgain")}</span>
          )}
        </button>
        <div className="shortcut-dialog-message" aria-live="polite">
          {shortcutDialogError && (
            <span className="error" role="alert">{shortcutDialogError}</span>
          )}
          {!shortcutDialogError && shortcutCandidate && (
            <span className="ready">{t("settings.shortcutCaptured")}</span>
          )}
        </div>
        <div className="shortcut-dialog-actions">
          <button
            type="button"
            onClick={() => void cancelShortcutRecording()}
            disabled={shortcutBusy}
          >
            {t("settings.shortcutCancel")}
          </button>
          <button
            type="button"
            className="primary"
            onClick={() => void confirmShortcut()}
            disabled={!shortcutCandidate || shortcutBusy}
          >
            {t("settings.shortcutSave")}
          </button>
        </div>
      </div>
    </div>
  );
}
