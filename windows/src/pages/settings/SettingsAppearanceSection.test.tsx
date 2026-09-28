import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SettingsData } from "../../types/settings.generated";
import { SettingsAppearanceSection } from "./SettingsAppearanceSection";

const labels: Record<string, string> = {
  "settings.appearance": "Appearance",
  "settings.appearanceDescription": "Customise the look and language",
  "settings.theme": "Theme",
  "settings.themeDescription": "Follow the system appearance or choose manually",
  "settings.themeSystem": "Follow system",
  "settings.themeLight": "Light",
  "settings.themeDark": "Dark",
  "settings.colorTheme": "Visual theme",
  "settings.colorThemeDescription": "Choose a complete language of type, material, colour, and motion",
  "settings.customThemeImport": "Import theme",
  "settings.language": "Language",
  "settings.languageDescription": "Choose the interface language for TailSync",
};

describe("SettingsAppearanceSection", () => {
  it("renders theme segmented control and description caption", () => {
    const changeThemePreference = vi.fn();
    const changeLanguage = vi.fn();

    render(
      <SettingsAppearanceSection
        settings={{ language: "zh-CN" } as SettingsData}
        t={(key) => labels[key] ?? key}
        locale="zh-CN"
        themePreference="system"
        v2Themes={[]}
        v2Active="tailsync"
        setLocale={vi.fn()}
        changeThemePreference={changeThemePreference}
        selectV2Theme={vi.fn()}
        handleImportTheme={vi.fn()}
        handleUpdateTheme={vi.fn()}
        rollbackV2Theme={vi.fn()}
        deleteV2Theme={vi.fn()}
        changeLanguage={changeLanguage}
      />,
    );

    // Verify captions
    expect(screen.getByText("Follow the system appearance or choose manually")).toBeInTheDocument();
    expect(screen.getByText("Choose the interface language for TailSync")).toBeInTheDocument();

    // Verify segmented control
    const segmentedGroup = screen.getByRole("radiogroup", { name: "Theme" });
    expect(segmentedGroup).toBeInTheDocument();

    // Click light theme option
    const lightButton = screen.getByRole("radio", { name: "Light" });
    fireEvent.click(lightButton);
    expect(changeThemePreference).toHaveBeenCalledWith("light");

    // Click dark theme option
    const darkButton = screen.getByRole("radio", { name: "Dark" });
    fireEvent.click(darkButton);
    expect(changeThemePreference).toHaveBeenCalledWith("dark");
  });
});
