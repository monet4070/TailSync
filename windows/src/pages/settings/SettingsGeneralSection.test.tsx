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
  "settings.launchAtLogin": "Launch at startup",
};

describe("SettingsGeneralSection launch at startup", () => {
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
    fireEvent.click(screen.getByRole("checkbox", { name: "Launch at startup" }));
    expect(setEnabled).toHaveBeenCalledWith(true);
  });
});
