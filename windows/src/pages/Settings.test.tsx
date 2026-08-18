import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ThemePackagePreview } from "../components/ThemePackagePreview";
import {
  applyThemePackageOperation,
  compareThemeVersions,
  updateOptionsFor,
  validateThemePackageForPreview,
  type PendingThemePackage,
} from "../utils/themePackageWorkflow";
import type {
  ResolvedThemeV2,
  ThemeV2Descriptor,
  ThemeValidationV2,
} from "../tailsyncClient";
import { decodeThemeDiagnostic, formatThemeError } from "../tailsyncClient";

const resolved = (mode: "light" | "dark", highContrast = false): ResolvedThemeV2 => ({
  id: "custom:studio.night",
  digest: "package-digest",
  mode,
  highContrast,
  provenance: {},
  assetSlots: {},
  tokens: {
    colors: {
      background: { canvas: "#101010", input: "#202020" },
      text: { primary: "#ffffff", secondary: "#dddddd" },
    },
    components: {
      history: {
        hover: { background: "#123456", foreground: "#ffffff" },
        selected: { background: "#345678", foreground: "#ffffff" },
      },
      search: {
        focus: { background: "#222222", foreground: "#ffffff", focusRing: "#ffff00" },
      },
      button: {
        default: { background: "#111111", foreground: "#ffffff" },
        hover: { background: "#222222", foreground: "#ffffff" },
        active: { background: "#333333", foreground: "#ffffff" },
        selected: { background: "#444444", foreground: "#ffffff" },
        disabled: { background: "#555555", foreground: "#cccccc" },
        focus: { background: "#666666", foreground: "#ffffff", focusRing: "#ffff00" },
      },
    },
  },
});

const validation = (preview: ResolvedThemeV2): ThemeValidationV2 => ({
  valid: true,
  digest: "package-digest",
  candidateVersion: "1.1.0",
  preview,
  diagnostics: [],
});

const descriptor: ThemeV2Descriptor = {
  id: "custom:studio.night",
  storageHandle: "studio_night",
  source: "custom",
  version: "1.1.0",
  digest: "package-digest",
  name: { en: "Night" },
  status: "valid",
  diagnostics: [],
};

describe("Settings Theme V2 workflow", () => {
  it("validates and retains all four isolated preview modes", async () => {
    const validate = vi.fn()
      .mockResolvedValueOnce(validation(resolved("light")))
      .mockResolvedValueOnce(validation(resolved("dark")))
      .mockResolvedValueOnce(validation(resolved("light", true)))
      .mockResolvedValueOnce(validation(resolved("dark", true)));

    const pending = await validateThemePackageForPreview(
      "/tmp/night.tailsync-theme",
      { kind: "install" },
      validate,
    );

    expect(validate.mock.calls).toEqual([
      ["/tmp/night.tailsync-theme", "light"],
      ["/tmp/night.tailsync-theme", "dark"],
      ["/tmp/night.tailsync-theme", "light", true],
      ["/tmp/night.tailsync-theme", "dark", true],
    ]);
    expect(pending.previews.highContrastLight.highContrast).toBe(true);
    expect(pending.previews.highContrastDark).toMatchObject({ mode: "dark", highContrast: true });
  });

  it("rejects an update package belonging to a different theme", async () => {
    const wrong = { ...resolved("light"), id: "custom:other.theme" };
    const validate = vi.fn().mockResolvedValue(validation(wrong));
    await expect(validateThemePackageForPreview(
      "/tmp/other.tailsync-theme",
      { kind: "update", themeId: "custom:studio.night", installedVersion: "1.0.0" },
      validate,
    )).rejects.toThrow("does not match");
  });

  it("installs or updates, refreshes the catalogue, and never activates implicitly", async () => {
    const base: PendingThemePackage = {
      path: "/tmp/night.tailsync-theme",
      digest: "package-digest",
      previews: {
        light: resolved("light"),
        dark: resolved("dark"),
        highContrastLight: resolved("light", true),
        highContrastDark: resolved("dark", true),
      },
      diagnostics: [],
      candidateVersion: "1.1.0",
      operation: { kind: "install" },
    };
    const install = vi.fn().mockResolvedValue(descriptor);
    const update = vi.fn().mockResolvedValue(descriptor);
    const refresh = vi.fn().mockResolvedValue(undefined);

    await applyThemePackageOperation(base, { install, update, refresh });
    await applyThemePackageOperation(
      { ...base, operation: { kind: "update", themeId: descriptor.id, installedVersion: "1.0.0" }, versionRelation: "upgrade" },
      { install, update, refresh },
    );

    expect(install).toHaveBeenCalledOnce();
    expect(update).toHaveBeenCalledOnce();
    expect(update).toHaveBeenCalledWith("/tmp/night.tailsync-theme", "package-digest", {});
    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it("classifies SemVer updates and supplies only the explicitly required Core flag", () => {
    expect(compareThemeVersions("1.0.0", "1.0.0")).toBe("same");
    expect(compareThemeVersions("1.0.0-beta.2", "1.0.0-beta.11")).toBe("downgrade");
    expect(compareThemeVersions("1.0.0", "1.0.0-rc.1")).toBe("upgrade");
    expect(compareThemeVersions("2.0.0", "3.0.0")).toBe("downgrade");

    const base = {
      path: "/tmp/night.tailsync-theme",
      digest: "digest",
      previews: {
        light: resolved("light"), dark: resolved("dark"),
        highContrastLight: resolved("light", true), highContrastDark: resolved("dark", true),
      },
      diagnostics: [],
      candidateVersion: "1.0.0",
      operation: { kind: "update", themeId: descriptor.id, installedVersion: "1.0.0" } as const,
    };
    expect(updateOptionsFor({ ...base, versionRelation: "same" })).toEqual({ allowSameVersion: true });
    expect(updateOptionsFor({ ...base, versionRelation: "downgrade" })).toEqual({ allowDowngrade: true });
    expect(updateOptionsFor({ ...base, versionRelation: "upgrade" })).toEqual({});
  });

  it("decodes and formats the complete structured theme error contract", () => {
    const thrown = {
      code: "THEME_ID",
      message: "theme id does not match storage handle",
      jsonPointer: "/id",
      severity: "error",
      platforms: ["windows"],
      recoverable: true,
      fallbackApplied: false,
    };
    expect(decodeThemeDiagnostic(thrown)).toEqual(thrown);
    expect(decodeThemeDiagnostic(JSON.stringify(thrown))).toEqual(thrown);
    expect(decodeThemeDiagnostic({ message: JSON.stringify(thrown) })).toEqual(thrown);
    expect(formatThemeError(thrown)).toContain("[error] THEME_ID");
    expect(formatThemeError(thrown)).toContain("/id; windows; recoverable; fallback not applied");
    expect(formatThemeError(thrown)).not.toContain("[object Object]");
    expect(formatThemeError(undefined)).toBe("Unknown theme error");
  });

  it("retains structured diagnostics returned by theme validation", async () => {
    const diagnostic = {
      code: "THEME_ID",
      message: "custom id must use the custom namespace",
      jsonPointer: "/id",
      severity: "error" as const,
      platforms: ["windows"],
      recoverable: true,
      fallbackApplied: false,
    };
    const invalid: ThemeValidationV2 = {
      valid: false,
      diagnostics: [diagnostic],
    };
    const validate = vi.fn().mockResolvedValue(invalid);

    await expect(validateThemePackageForPreview(
      "/tmp/reserved.tailsync-theme",
      { kind: "install" },
      validate,
    )).rejects.toEqual(diagnostic);
  });

  it("renders all component states inside the isolated preview context", () => {
    render(<ThemePackagePreview
      resolved={resolved("light")}
      path="/tmp/night.tailsync-theme"
      digest="package-digest"
      label="Light"
    />);
    const preview = screen.getByLabelText("Light theme preview");
    expect(preview.style.getPropertyValue("--theme-history-hover-background")).toBe("#123456");
    expect(preview.style.getPropertyValue("--theme-history-selected-background")).toBe("#345678");
    expect(preview.style.getPropertyValue("--theme-search-focus-focus-ring")).toBe("#ffff00");
    expect(preview.style.getPropertyValue("--theme-button-active-background")).toBe("#333333");
    for (const state of ["default", "hover", "active", "selected", "disabled", "focus"]) {
      expect(screen.getAllByText(state).length).toBeGreaterThan(0);
    }
    expect(document.documentElement.style.getPropertyValue("--theme-history-hover-background")).toBe("");
    expect(document.documentElement.style.getPropertyValue("--theme-button-active-background")).toBe("");
  });

  it("previews every declared semantic image slot without installing", async () => {
    const withAssets = resolved("light");
    withAssets.assetSlots = Object.fromEntries(["logo", "emptyState", "previewPlaceholder"].map((slot) => [slot, {
      slot,
      key: `assets/${slot}.png`,
      digest: `${slot}-digest`,
      mimeType: "image/png",
      bytes: 8,
      width: 1,
      height: 1,
    }]));
    const loadAsset = vi.fn().mockResolvedValue(new ArrayBuffer(8));
    const createObjectURL = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:preview");

    const { container } = render(<ThemePackagePreview
      resolved={withAssets}
      path="/tmp/night.tailsync-theme"
      digest="package-digest"
      label="Light"
      loadAsset={loadAsset}
    />);

    await waitFor(() => expect(container.querySelectorAll("img")).toHaveLength(3));
    expect(loadAsset).toHaveBeenCalledTimes(3);
    createObjectURL.mockRestore();
  });
});
