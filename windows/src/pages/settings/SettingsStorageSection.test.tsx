import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SettingsData } from "../../types/settings.generated";
import { SettingsStorageSection } from "./SettingsStorageSection";

const labels: Record<string, string> = {
  "settings.storage": "Storage",
  "settings.storageDescription": "History and file transfer data",
  "settings.storageChange": "Change",
  "settings.storageMoving": "Moving...",
  "settings.storageQuota": "Storage quota",
  "settings.storageQuotaDescription": "Automatically delete the oldest entries when this limit is exceeded",
  "settings.storageOldData": "Old data remains at the previous location",
  "settings.storageDeleteOld": "Delete old data",
  "settings.storageKeepOld": "Keep",
};

describe("SettingsStorageSection", () => {
  it("renders storage quota caption and handles input changes", () => {
    const setStorageQuotaDraft = vi.fn();
    const commitStorageQuota = vi.fn();
    const changeStorage = vi.fn();

    render(
      <SettingsStorageSection
        settings={{ storage_root: "C:\\TailSync", storage_quota_bytes: 5368709120 } as SettingsData}
        t={(key) => labels[key] ?? key}
        storageStatus={{
          root: "C:\\TailSync",
          used_bytes: 1048576,
          quota_bytes: 5368709120,
          available: true,
          error: null,
        }}
        storageBusy={false}
        storageQuotaDraft="5"
        oldStorage={null}
        setStorageQuotaDraft={setStorageQuotaDraft}
        setOldStorage={vi.fn()}
        changeStorage={changeStorage}
        commitStorageQuota={commitStorageQuota}
        handleDeleteOldStorage={vi.fn()}
      />,
    );

    // Check quota caption
    expect(
      screen.getByText("Automatically delete the oldest entries when this limit is exceeded"),
    ).toBeInTheDocument();

    // Check quota input
    const input = screen.getByRole("textbox", { name: "Storage quota" });
    fireEvent.change(input, { target: { value: "10" } });
    expect(setStorageQuotaDraft).toHaveBeenCalledWith("10");

    fireEvent.blur(input);
    expect(commitStorageQuota).toHaveBeenCalled();
  });
});
