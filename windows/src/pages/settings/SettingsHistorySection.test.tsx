import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SettingsHistorySection } from "./SettingsHistorySection";

const labels: Record<string, string> = {
  "settings.history": "History",
  "settings.historyDescription": "Manage clipboard history storage",
  "settings.historyLimit": "History limit",
  "settings.limitDescription": "Keep up to {value} clipboard history entries",
  "settings.historyLimitDescriptionPrefix": "Keep up to",
  "settings.historyLimitDescriptionSuffix": "entries",
  "settings.autoSaveNetworkImages": "Auto-save network images",
  "settings.autoSaveNetworkImagesDescription": "Only affects new copies on this device",
};

describe("SettingsHistorySection", () => {
  it("interpolates history limit value into limit description", () => {
    const setHistoryLimitDraft = vi.fn();
    const commitHistoryLimit = vi.fn();

    render(
      <SettingsHistorySection
        t={(key) => labels[key] ?? key}
        historyLimitDraft={150}
        setHistoryLimitDraft={setHistoryLimitDraft}
        commitHistoryLimit={commitHistoryLimit}
        autoSaveNetworkImages={false}
        setAutoSaveNetworkImages={vi.fn()}
      />,
    );

    // Verify interpolated caption text
    expect(screen.getByText("Keep up to 150 clipboard history entries")).toBeInTheDocument();

    // Verify slider change
    const slider = screen.getByRole("slider", { name: "History limit" });
    fireEvent.change(slider, { target: { value: "200" } });
    expect(setHistoryLimitDraft).toHaveBeenCalledWith(200);
  });

  it("renders the auto-save network images switch as disabled by default", () => {
    render(
      <SettingsHistorySection
        t={(key) => labels[key] ?? key}
        historyLimitDraft={100}
        setHistoryLimitDraft={vi.fn()}
        commitHistoryLimit={vi.fn()}
        autoSaveNetworkImages={false}
        setAutoSaveNetworkImages={vi.fn()}
      />,
    );

    const toggle = screen.getByRole("checkbox", { name: "Auto-save network images" });
    expect(toggle).not.toBeChecked();
    expect(screen.getByText("Only affects new copies on this device")).toBeInTheDocument();
  });

  it("reflects the persisted value and reports user toggles", () => {
    const setAutoSaveNetworkImages = vi.fn().mockResolvedValue(undefined);
    render(
      <SettingsHistorySection
        t={(key) => labels[key] ?? key}
        historyLimitDraft={100}
        setHistoryLimitDraft={vi.fn()}
        commitHistoryLimit={vi.fn()}
        autoSaveNetworkImages
        setAutoSaveNetworkImages={setAutoSaveNetworkImages}
      />,
    );

    const toggle = screen.getByRole("checkbox", { name: "Auto-save network images" });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    expect(setAutoSaveNetworkImages).toHaveBeenCalledWith(false);
  });
});
