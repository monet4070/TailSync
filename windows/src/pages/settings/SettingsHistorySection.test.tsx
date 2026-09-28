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
      />,
    );

    // Verify interpolated caption text
    expect(screen.getByText("Keep up to 150 clipboard history entries")).toBeInTheDocument();

    // Verify slider change
    const slider = screen.getByRole("slider", { name: "History limit" });
    fireEvent.change(slider, { target: { value: "200" } });
    expect(setHistoryLimitDraft).toHaveBeenCalledWith(200);
  });
});
