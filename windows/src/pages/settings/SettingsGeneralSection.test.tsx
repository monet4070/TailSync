import { createRef } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SettingsData } from "../../types/settings.generated";
import { SettingsGeneralSection } from "./SettingsGeneralSection";
import type { ShortcutRecorder } from "./SettingsSectionTypes";

const recorder = (shortcut: string) => ({
  shortcutTriggerRef: createRef<HTMLButtonElement>(),
  shortcutBusy: false,
  shortcutRecording: false,
  shortcutDraft: shortcut,
  startShortcutRecording: vi.fn(),
  setShortcutDraft: vi.fn(),
  commitShortcut: vi.fn(),
}) as unknown as ShortcutRecorder;

const settings = {
  sync_enabled: true,
  sync_shortcut: "CommandOrControl+Shift+S",
  history_shortcut: "CommandOrControl+Shift+H",
  notifications_enabled: true,
  progress_bar_enabled: true,
} as SettingsData;

const labels: Record<string, string> = {
  "settings.general": "General",
  "settings.generalDescription": "General settings",
  "settings.syncEnabled": "Clipboard sync",
  "settings.syncEnabledDescription": "Broadcast new local clipboard content to enabled paired devices",
  "settings.launchAtLogin": "Launch at startup",
  "settings.launchAtLoginDescription": "Start TailSync automatically after you sign in to Windows",
  "settings.syncShortcut": "Sync shortcut",
  "settings.syncShortcutDescription": "Global shortcut for pausing or resuming clipboard sync",
  "settings.historyShortcut": "History shortcut",
  "settings.historyShortcutDescription": "Open clipboard history globally from any application",
  "settings.notifications": "Notifications",
  "settings.notificationsDescription": "Show a system notification when remote content arrives",
  "settings.progressBar": "Progress bar",
  "settings.progressBarDescription": "Show transfer feedback while large files are being sent",
};

describe("SettingsGeneralSection", () => {
  it("renders a system-backed launch-at-startup toggle", () => {
    const setEnabled = vi.fn().mockResolvedValue(true);
    render(
      <SettingsGeneralSection
        settings={settings}
        t={(key) => labels[key] ?? key}
        launchAtLogin={{
          enabled: false,
          loading: false,
          busy: false,
          error: "",
          refresh: vi.fn(),
          setEnabled,
        }}
        syncShortcutRecorder={recorder(settings.sync_shortcut)}
        historyShortcutRecorder={recorder(settings.history_shortcut)}
        setGlobalSync={vi.fn()}
        update={vi.fn()}
      />,
    );

    expect(screen.getByText("Launch at startup")).toBeInTheDocument();
    expect(screen.getByText("Start TailSync automatically after you sign in to Windows")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "Launch at startup" }));
    expect(setEnabled).toHaveBeenCalledWith(true);
  });

  it("renders captions for sync, notifications, and progress bar", () => {
    const setGlobalSync = vi.fn();
    const update = vi.fn();
    render(
      <SettingsGeneralSection
        settings={settings}
        t={(key) => labels[key] ?? key}
        launchAtLogin={{
          enabled: true,
          loading: false,
          busy: false,
          error: "",
          refresh: vi.fn(),
          setEnabled: vi.fn(),
        }}
        syncShortcutRecorder={recorder(settings.sync_shortcut)}
        historyShortcutRecorder={recorder(settings.history_shortcut)}
        setGlobalSync={setGlobalSync}
        update={update}
      />,
    );

    expect(screen.getByText("Broadcast new local clipboard content to enabled paired devices")).toBeInTheDocument();
    expect(screen.getByText("Show transfer feedback while large files are being sent")).toBeInTheDocument();
    expect(screen.getByText("Show a system notification when remote content arrives")).toBeInTheDocument();
  });
});
