import SwiftUI
import XCTest
@testable import TailSync

final class ThemeTests: XCTestCase {
    func testSettingsRoundTripPreservesColorTheme() throws {
        var settings = AppSettings()
        settings.color_theme = TailSyncColorTheme.rose.rawValue

        let data = try JSONEncoder().encode(settings)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        XCTAssertEqual(object["color_theme"] as? String, "rose")

        let decoded = try JSONDecoder().decode(AppSettings.self, from: data)
        XCTAssertEqual(decoded.color_theme, "rose")
    }

    func testMissingAndUnknownColorThemesFallBackToCanvas() throws {
        let missing = try JSONDecoder().decode(AppSettings.self, from: Data("{}".utf8))
        XCTAssertEqual(missing.color_theme, TailSyncColorTheme.tailsync.rawValue)
        XCTAssertEqual(missing.sync_shortcut, "CommandOrControl+Shift+S")
        XCTAssertEqual(missing.history_shortcut, "CommandOrControl+Shift+H")

        let unknown = TailSyncColorTheme(storedValue: "not-a-theme")
        XCTAssertEqual(unknown, .tailsync)
    }

    func testThemeIdentifiersMatchTheRustContract() {
        XCTAssertEqual(
            Set(TailSyncColorTheme.allCases.map(\.rawValue)),
            Set(["tailsync", "ocean", "forest", "rose", "high-contrast"])
        )
    }

    func testEveryThemeHasDistinctLightAndDarkPalettes() {
        let light = Set(TailSyncColorTheme.allCases.map { $0.palette(for: .light).signature })
        let dark = Set(TailSyncColorTheme.allCases.map { $0.palette(for: .dark).signature })

        XCTAssertEqual(light.count, TailSyncColorTheme.allCases.count)
        XCTAssertEqual(dark.count, TailSyncColorTheme.allCases.count)
    }

    func testKeyPaletteTokensMatchTheWindowsDesignSystem() {
        let expected: [TailSyncColorTheme: (lightAccent: UInt32, darkAccent: UInt32,
                                             lightWindow: UInt32, darkWindow: UInt32)] = [
            .tailsync: (0xD5684B, 0xEC8668, 0xFAF9F5, 0x191918),
            .ocean: (0x087F8C, 0x4CC9C0, 0xF1F7F7, 0x111E20),
            .forest: (0x287A55, 0x69CA91, 0xF3F7F3, 0x151F18),
            .rose: (0xB83E60, 0xEF86A2, 0xFAF4F6, 0x24191D),
            .highContrast: (0x005FCC, 0xFFD400, 0xFFFFFF, 0x000000),
        ]

        for theme in TailSyncColorTheme.allCases {
            let tokens = expected[theme]
            XCTAssertEqual(theme.palette(for: .light).accent, tokens?.lightAccent)
            XCTAssertEqual(theme.palette(for: .dark).accent, tokens?.darkAccent)
            XCTAssertEqual(theme.palette(for: .light).window, tokens?.lightWindow)
            XCTAssertEqual(theme.palette(for: .dark).window, tokens?.darkWindow)
        }
    }

    func testCanvasUsesTheWindowsEditorialMetrics() {
        let canvas = TailSyncColorTheme.tailsync

        XCTAssertEqual(canvas.metrics.cardRadius, 10)
        XCTAssertEqual(canvas.metrics.controlRadius, 9)
        XCTAssertEqual(canvas.metrics.rowPadding, 13)
        XCTAssertEqual(canvas.typography.sectionTitleSize, 25)
        XCTAssertFalse(canvas.typography.uppercasesSectionTitles)
        XCTAssertEqual(canvas.typography.searchSize, 18)
        XCTAssertTrue(canvas.typography.searchUsesDisplayFont)
        XCTAssertEqual(canvas.typography.historyContentSize, 15)
    }
}

// ── Custom-theme model (T007) ───────────────────────────────────────────

private func paletteSnapshot(
    accent: UInt32, accentContrast: UInt32, window: UInt32, surface: UInt32,
    softSurface: UInt32, raised: UInt32, textPrimary: UInt32, textSecondary: UInt32,
    textTertiary: UInt32, border: UInt32, divider: UInt32, positive: UInt32,
    warning: UInt32, toast: UInt32, toastText: UInt32,
    softSurfaceOpacity: Double = 1, textPrimaryOpacity: Double = 1,
    textSecondaryOpacity: Double = 1, textTertiaryOpacity: Double = 1,
    borderOpacity: Double = 1, dividerOpacity: Double = 1, toastOpacity: Double = 1
) -> TailSyncThemePalette {
    TailSyncThemePalette(
        accent: accent, accentContrast: accentContrast, window: window,
        surface: surface, softSurface: softSurface, raised: raised,
        textPrimary: textPrimary, textSecondary: textSecondary,
        textTertiary: textTertiary, border: border, divider: divider,
        positive: positive, warning: warning, toast: toast, toastText: toastText,
        softSurfaceOpacity: softSurfaceOpacity, textPrimaryOpacity: textPrimaryOpacity,
        textSecondaryOpacity: textSecondaryOpacity, textTertiaryOpacity: textTertiaryOpacity,
        borderOpacity: borderOpacity, dividerOpacity: dividerOpacity,
        toastOpacity: toastOpacity
    )
}

extension ThemeTests {
    /// Locks the built-in definitions against the pre-refactor switch
    /// literals (T007 regression snapshot), field by field.
    func testBuiltinDefinitionsMatchThePreRefactorSnapshot() {
        let expected: [TailSyncColorTheme: TailSyncThemeDefinition] = [
            .tailsync: TailSyncThemeDefinition(
                id: "tailsync", name: ["en": "Canvas"],
                lightPalette: paletteSnapshot(
                    accent: 0xD5684B, accentContrast: 0xFFFFFF, window: 0xFAF9F5,
                    surface: 0xFFFEFA, softSurface: 0x1A1916, raised: 0xFFFEFA,
                    textPrimary: 0x191918, textSecondary: 0x68665F, textTertiary: 0x98958B,
                    border: 0xD3CEC2, divider: 0xECE8DF, positive: 0x44745A,
                    warning: 0xB96536, toast: 0x171716, toastText: 0xFFFFFF,
                    softSurfaceOpacity: 0.045),
                darkPalette: paletteSnapshot(
                    accent: 0xEC8668, accentContrast: 0x181412, window: 0x191918,
                    surface: 0x232321, softSurface: 0xFFFDF5, raised: 0x262522,
                    textPrimary: 0xF4F1E9, textSecondary: 0xAAA69C, textTertiary: 0x77746C,
                    border: 0x48463F, divider: 0x2C2B28, positive: 0x75AA86,
                    warning: 0xDC9163, toast: 0xF8F5ED, toastText: 0x171716,
                    softSurfaceOpacity: 0.055),
                metrics: TailSyncThemeMetrics(cardRadius: 10, controlRadius: 9, rowPadding: 13, shadowRadius: 8),
                typography: TailSyncThemeTypography(
                    sectionTitleSize: 25, uppercasesSectionTitles: false,
                    searchSize: 18, searchUsesDisplayFont: true, historyContentSize: 15),
                displayFontName: "Songti SC", readingFontName: nil),
            .ocean: TailSyncThemeDefinition(
                id: "ocean", name: ["en": "Flux"],
                lightPalette: paletteSnapshot(
                    accent: 0x087F8C, accentContrast: 0xFFFFFF, window: 0xF1F7F7,
                    surface: 0xFCFEFE, softSurface: 0x085B65, raised: 0xFFFFFF,
                    textPrimary: 0x051F21, textSecondary: 0x051F21, textTertiary: 0x051F21,
                    border: 0x08474E, divider: 0x08474E, positive: 0x34C759,
                    warning: 0xFF9500, toast: 0x000000, toastText: 0xFFFFFF,
                    softSurfaceOpacity: 0.06, textPrimaryOpacity: 0.90,
                    textSecondaryOpacity: 0.60, textTertiaryOpacity: 0.40,
                    borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.88),
                darkPalette: paletteSnapshot(
                    accent: 0x4CC9C0, accentContrast: 0x071918, window: 0x111E20,
                    surface: 0x1A2A2D, softSurface: 0xD2FFFC, raised: 0x203235,
                    textPrimary: 0xF2FFFE, textSecondary: 0xE0F8F6, textTertiary: 0xE0F8F6,
                    border: 0xC0EFEB, divider: 0xC0EFEB, positive: 0x30D158,
                    warning: 0xFF9F0A, toast: 0xFFFFFF, toastText: 0x000000,
                    softSurfaceOpacity: 0.07, textPrimaryOpacity: 0.94,
                    textSecondaryOpacity: 0.62, textTertiaryOpacity: 0.40,
                    borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.92),
                metrics: TailSyncThemeMetrics(cardRadius: 5, controlRadius: 3, rowPadding: 11, shadowRadius: 7),
                typography: TailSyncThemeTypography(
                    sectionTitleSize: 12, uppercasesSectionTitles: true,
                    searchSize: 13, searchUsesDisplayFont: false, historyContentSize: 13),
                displayFontName: "Avenir Next", readingFontName: "Avenir Next"),
            .forest: TailSyncThemeDefinition(
                id: "forest", name: ["en": "Ledger"],
                lightPalette: paletteSnapshot(
                    accent: 0x287A55, accentContrast: 0xFFFFFF, window: 0xF3F7F3,
                    surface: 0xFCFDFB, softSurface: 0x275C3E, raised: 0xFFFFFF,
                    textPrimary: 0x13261B, textSecondary: 0x13261B, textTertiary: 0x13261B,
                    border: 0x235235, divider: 0x235235, positive: 0x34C759,
                    warning: 0xFF9500, toast: 0x000000, toastText: 0xFFFFFF,
                    softSurfaceOpacity: 0.06, textPrimaryOpacity: 0.90,
                    textSecondaryOpacity: 0.60, textTertiaryOpacity: 0.40,
                    borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.88),
                darkPalette: paletteSnapshot(
                    accent: 0x69CA91, accentContrast: 0x0B1A11, window: 0x151F18,
                    surface: 0x202D24, softSurface: 0xE1FFEA, raised: 0x26362A,
                    textPrimary: 0xF6FFF8, textSecondary: 0xE5F8EA, textTertiary: 0xE5F8EA,
                    border: 0xCFEFD8, divider: 0xCFEFD8, positive: 0x30D158,
                    warning: 0xFF9F0A, toast: 0xFFFFFF, toastText: 0x000000,
                    softSurfaceOpacity: 0.07, textPrimaryOpacity: 0.94,
                    textSecondaryOpacity: 0.62, textTertiaryOpacity: 0.40,
                    borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.92),
                metrics: TailSyncThemeMetrics(cardRadius: 4, controlRadius: 2, rowPadding: 12, shadowRadius: 5),
                typography: TailSyncThemeTypography(
                    sectionTitleSize: 12, uppercasesSectionTitles: true,
                    searchSize: 13, searchUsesDisplayFont: false, historyContentSize: 14),
                displayFontName: "Songti SC", readingFontName: "Songti SC"),
            .rose: TailSyncThemeDefinition(
                id: "rose", name: ["en": "Aura"],
                lightPalette: paletteSnapshot(
                    accent: 0xB83E60, accentContrast: 0xFFFFFF, window: 0xFAF4F6,
                    surface: 0xFFFDFD, softSurface: 0x802A42, raised: 0xFFFFFF,
                    textPrimary: 0x30131B, textSecondary: 0x30131B, textTertiary: 0x30131B,
                    border: 0x71263B, divider: 0x71263B, positive: 0x34C759,
                    warning: 0xFF9500, toast: 0x000000, toastText: 0xFFFFFF,
                    softSurfaceOpacity: 0.055, textPrimaryOpacity: 0.90,
                    textSecondaryOpacity: 0.60, textTertiaryOpacity: 0.40,
                    borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.88),
                darkPalette: paletteSnapshot(
                    accent: 0xEF86A2, accentContrast: 0x251016, window: 0x24191D,
                    surface: 0x322329, softSurface: 0xFFE2EA, raised: 0x3A2930,
                    textPrimary: 0xFFF7F9, textSecondary: 0xFAE5EB, textTertiary: 0xFAE5EB,
                    border: 0xF5D3DC, divider: 0xF5D3DC, positive: 0x30D158,
                    warning: 0xFF9F0A, toast: 0xFFFFFF, toastText: 0x000000,
                    softSurfaceOpacity: 0.07, textPrimaryOpacity: 0.94,
                    textSecondaryOpacity: 0.62, textTertiaryOpacity: 0.40,
                    borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.92),
                metrics: TailSyncThemeMetrics(cardRadius: 12, controlRadius: 9, rowPadding: 11, shadowRadius: 10),
                typography: TailSyncThemeTypography(
                    sectionTitleSize: 12, uppercasesSectionTitles: true,
                    searchSize: 13, searchUsesDisplayFont: false, historyContentSize: 13),
                displayFontName: nil, readingFontName: nil),
            .highContrast: TailSyncThemeDefinition(
                id: "high-contrast", name: ["en": "Mono"],
                lightPalette: paletteSnapshot(
                    accent: 0x005FCC, accentContrast: 0xFFFFFF, window: 0xFFFFFF,
                    surface: 0xFFFFFF, softSurface: 0xF0F0F0, raised: 0xFFFFFF,
                    textPrimary: 0x000000, textSecondary: 0x333333, textTertiary: 0x5A5A5A,
                    border: 0x8A8A8A, divider: 0xB0B0B0, positive: 0x087A32,
                    warning: 0x9A4D00, toast: 0x000000, toastText: 0xFFFFFF),
                darkPalette: paletteSnapshot(
                    accent: 0xFFD400, accentContrast: 0x000000, window: 0x000000,
                    surface: 0x101010, softSurface: 0x1C1C1C, raised: 0x181818,
                    textPrimary: 0xFFFFFF, textSecondary: 0xE3E3E3, textTertiary: 0xB8B8B8,
                    border: 0x777777, divider: 0x595959, positive: 0x65FF8F,
                    warning: 0xFFD166, toast: 0xFFFFFF, toastText: 0x000000),
                metrics: TailSyncThemeMetrics(cardRadius: 0, controlRadius: 0, rowPadding: 12, shadowRadius: 0),
                typography: TailSyncThemeTypography(
                    sectionTitleSize: 12, uppercasesSectionTitles: true,
                    searchSize: 13, searchUsesDisplayFont: false, historyContentSize: 13),
                displayFontName: "Arial", readingFontName: "Arial"),
        ]

        XCTAssertEqual(TailSyncColorTheme.allCases.count, expected.count)
        for theme in TailSyncColorTheme.allCases {
            let expectedDefinition = try! XCTUnwrap(expected[theme])
            XCTAssertEqual(theme.definition, expectedDefinition, "definition drift for \(theme.rawValue)")
            XCTAssertEqual(theme.palette(for: .light), expectedDefinition.lightPalette)
            XCTAssertEqual(theme.palette(for: .dark), expectedDefinition.darkPalette)
            XCTAssertEqual(theme.metrics, expectedDefinition.metrics)
            XCTAssertEqual(theme.typography, expectedDefinition.typography)
            XCTAssertEqual(theme.displayFontName, expectedDefinition.displayFontName)
            XCTAssertEqual(theme.readingFontName, expectedDefinition.readingFontName)
        }
    }

    func testCustomThemeDefinitionDecodesFromJson() throws {
        let json = """
        {
          "id": "studio",
          "name": { "en": "Studio", "zh-CN": "工作室" },
          "file": "studio.json",
          "palette": {
            "light": {
              "brand": "#d5684b",
              "brandHover": "#bb553b",
              "brandSoft": { "hex": "#d5684b", "opacity": 0.11 },
              "brandText": "#ffffff",
              "bgWindow": "#faf9f5",
              "bgCard": "#fffefa",
              "bgInput": { "hex": "#1a1916", "opacity": 0.045 },
              "bgHover": "#f0eee7",
              "bgActive": "#e8e4da",
              "bgRaised": "#fffefa",
              "bgToast": "#171716",
              "textPrimary": { "hex": "#191918", "opacity": 0.92 },
              "textSecondary": "#68665f",
              "textTertiary": "#98958b",
              "textToast": "#ffffff",
              "border": "#e7e3d9",
              "borderStrong": "#d3cec2",
              "divider": "#ece8df",
              "green": "#44745a",
              "greenSoft": "#44745a",
              "orange": "#b96536",
              "orangeSoft": "#b96536",
              "purple": "#765b8f",
              "purpleSoft": "#765b8f"
            },
            "dark": {
              "brand": "#ec8668",
              "brandHover": "#f29b80",
              "brandSoft": "#ec8668",
              "brandText": "#181412",
              "bgWindow": "#191918",
              "bgCard": "#232321",
              "bgInput": "#fffdf5",
              "bgHover": "#292825",
              "bgActive": "#33312d",
              "bgRaised": "#262522",
              "bgToast": "#f8f5ed",
              "textPrimary": "#f4f1e9",
              "textSecondary": "#aaa69c",
              "textTertiary": "#77746c",
              "textToast": "#171716",
              "border": "#32312d",
              "borderStrong": "#48463f",
              "divider": "#2c2b28",
              "green": "#75aa86",
              "greenSoft": "#75aa86",
              "orange": "#dc9163",
              "orangeSoft": "#dc9163",
              "purple": "#ad8cc6",
              "purpleSoft": "#ad8cc6"
            }
          },
          "metrics": { "cardRadius": 10, "controlRadius": 9, "rowPadding": 13, "shadowRadius": 8 },
          "typography": {
            "sectionTitleSize": 25,
            "uppercasesSectionTitles": false,
            "searchSize": 18,
            "searchUsesDisplayFont": true,
            "historyContentSize": 15
          },
          "fonts": { "display": "Songti SC", "reading": null },
          "structural": { "borderRadius": 10, "shadow": false }
        }
        """

        let definition = try JSONDecoder().decode(
            TailSyncThemeDefinition.self, from: Data(json.utf8))

        XCTAssertEqual(definition.id, "studio")
        XCTAssertEqual(definition.localizedName(preferred: "zh-CN"), "工作室")
        XCTAssertEqual(definition.localizedName(preferred: "fr"), "Studio")
        // CSS token mapping: brand -> accent, borderStrong -> border, etc.
        XCTAssertEqual(definition.lightPalette.accent, 0xD5684B)
        XCTAssertEqual(definition.darkPalette.accent, 0xEC8668)
        XCTAssertEqual(definition.lightPalette.softSurface, 0x1A1916)
        XCTAssertEqual(definition.lightPalette.softSurfaceOpacity, 0.045)
        XCTAssertEqual(definition.lightPalette.textPrimary, 0x191918)
        XCTAssertEqual(definition.lightPalette.textPrimaryOpacity, 0.92)
        XCTAssertEqual(definition.lightPalette.border, 0xD3CEC2)
        XCTAssertEqual(definition.lightPalette.borderOpacity, 1)
        XCTAssertEqual(definition.lightPalette.divider, 0xECE8DF)
        XCTAssertEqual(definition.lightPalette.positive, 0x44745A)
        XCTAssertEqual(definition.lightPalette.warning, 0xB96536)
        XCTAssertEqual(definition.lightPalette.toast, 0x171716)
        XCTAssertEqual(definition.lightPalette.toastText, 0xFFFFFF)
        XCTAssertEqual(definition.darkPalette.window, 0x191918)
        XCTAssertEqual(definition.metrics.cardRadius, 10)
        XCTAssertEqual(definition.typography.searchUsesDisplayFont, true)
        XCTAssertFalse(definition.typography.uppercasesSectionTitles)
        XCTAssertEqual(definition.displayFontName, "Songti SC")
        XCTAssertNil(definition.readingFontName)
    }

    func testCustomDefinitionRejectsBrokenPalettes() {
        let missingToken = """
        { "id": "broken", "name": { "en": "B" },
          "palette": { "light": { "brand": "#d5684b" }, "dark": { "brand": "#d5684b" } },
          "metrics": { "cardRadius": 10, "controlRadius": 9, "rowPadding": 13, "shadowRadius": 8 },
          "typography": { "sectionTitleSize": 25, "uppercasesSectionTitles": false,
            "searchSize": 18, "searchUsesDisplayFont": true, "historyContentSize": 15 },
          "fonts": { "display": null, "reading": null } }
        """
        XCTAssertThrowsError(
            try JSONDecoder().decode(TailSyncThemeDefinition.self, from: Data(missingToken.utf8)))

        let badHex = """
        { "id": "broken", "name": { "en": "B" },
          "palette": { "light": { "brand": "not-a-color", "brandText": "#ffffff",
            "bgWindow": "#faf9f5", "bgCard": "#fffefa", "bgInput": "#1a1916",
            "bgRaised": "#fffefa", "textPrimary": "#191918", "textSecondary": "#68665f",
            "textTertiary": "#98958b", "borderStrong": "#d3cec2", "divider": "#ece8df",
            "green": "#44745a", "orange": "#b96536", "bgToast": "#171716", "textToast": "#ffffff" },
            "dark": { "brand": "#ec8668", "brandText": "#181412", "bgWindow": "#191918",
            "bgCard": "#232321", "bgInput": "#fffdf5", "bgRaised": "#262522",
            "textPrimary": "#f4f1e9", "textSecondary": "#aaa69c", "textTertiary": "#77746c",
            "borderStrong": "#48463f", "divider": "#2c2b28", "green": "#75aa86",
            "orange": "#dc9163", "bgToast": "#f8f5ed", "textToast": "#171716" } },
          "metrics": { "cardRadius": 10, "controlRadius": 9, "rowPadding": 13, "shadowRadius": 8 },
          "typography": { "sectionTitleSize": 25, "uppercasesSectionTitles": false,
            "searchSize": 18, "searchUsesDisplayFont": true, "historyContentSize": 15 },
          "fonts": { "display": null, "reading": null } }
        """
        XCTAssertThrowsError(
            try JSONDecoder().decode(TailSyncThemeDefinition.self, from: Data(badHex.utf8)))
    }

    func testThemeSelectionResolvesStoredValuesAgainstTheCatalogue() {
        let studioJSON = """
        { "id": "studio", "name": { "en": "Studio" },
          "palette": { "light": { "brand": "#d5684b", "brandText": "#ffffff",
            "bgWindow": "#faf9f5", "bgCard": "#fffefa", "bgInput": "#1a1916",
            "bgRaised": "#fffefa", "textPrimary": "#191918", "textSecondary": "#68665f",
            "textTertiary": "#98958b", "borderStrong": "#d3cec2", "divider": "#ece8df",
            "green": "#44745a", "orange": "#b96536", "bgToast": "#171716", "textToast": "#ffffff" },
            "dark": { "brand": "#ec8668", "brandText": "#181412", "bgWindow": "#191918",
            "bgCard": "#232321", "bgInput": "#fffdf5", "bgRaised": "#262522",
            "textPrimary": "#f4f1e9", "textSecondary": "#aaa69c", "textTertiary": "#77746c",
            "borderStrong": "#48463f", "divider": "#2c2b28", "green": "#75aa86",
            "orange": "#dc9163", "bgToast": "#f8f5ed", "textToast": "#171716" } },
          "metrics": { "cardRadius": 10, "controlRadius": 9, "rowPadding": 13, "shadowRadius": 8 },
          "typography": { "sectionTitleSize": 25, "uppercasesSectionTitles": false,
            "searchSize": 18, "searchUsesDisplayFont": true, "historyContentSize": 15 },
          "fonts": { "display": "Songti SC", "reading": null } }
        """
        let studio = try! JSONDecoder().decode(
            TailSyncThemeDefinition.self, from: Data(studioJSON.utf8))
        let catalogue = [studio]

        // Built-in stored value resolves to the built-in theme.
        let builtinSelection = TailSyncThemeSelection(storedValue: "ocean", catalogue: catalogue)
        XCTAssertEqual(builtinSelection.id, "ocean")
        XCTAssertNil(builtinSelection.definition)
        XCTAssertEqual(builtinSelection.metrics.cardRadius, 5)

        // Custom stored value resolves against the catalogue.
        let customSelection = TailSyncThemeSelection(storedValue: "custom:studio", catalogue: catalogue)
        XCTAssertEqual(customSelection.id, "custom:studio")
        XCTAssertEqual(customSelection.definition, studio)
        XCTAssertEqual(customSelection.palette(for: .light).accent, 0xD5684B)
        XCTAssertEqual(customSelection.palette(for: .dark).accent, 0xEC8668)
        XCTAssertEqual(customSelection.metrics.cardRadius, 10)
        XCTAssertEqual(customSelection.typography.searchUsesDisplayFont, true)
        XCTAssertEqual(customSelection.displayFontName, "Songti SC")

        // Unknown custom ids fall back to the default theme at apply time.
        let missingSelection = TailSyncThemeSelection(storedValue: "custom:ghost", catalogue: catalogue)
        XCTAssertEqual(missingSelection.id, "tailsync")
        XCTAssertNil(missingSelection.definition)

        // Garbage stored values also fall back to the default theme.
        let garbageSelection = TailSyncThemeSelection(storedValue: "not-a-theme", catalogue: catalogue)
        XCTAssertEqual(garbageSelection.id, "tailsync")
    }
}

extension ThemeTests {
    func testColorThemeNormalizationKeepsCustomPreferences() {
        XCTAssertEqual(Loc.normalizeColorTheme("custom:studio"), "custom:studio")
        XCTAssertEqual(Loc.normalizeColorTheme("ocean"), "ocean")
        XCTAssertEqual(Loc.normalizeColorTheme("not-a-theme"), "tailsync")
    }
}

extension ThemeTests {
    /// Background metadata decodes from the slimmed listing shape (metadata
    /// only — no image bytes anywhere).
    func testDefinitionDecodesBackgroundMetadata() throws {
        let json = """
        { "id": "studio", "name": { "en": "Studio" },
          "palette": { "light": { "brand": "#d5684b", "brandText": "#ffffff",
            "bgWindow": "#faf9f5", "bgCard": "#fffefa", "bgInput": "#1a1916",
            "bgRaised": "#fffefa", "textPrimary": "#191918", "textSecondary": "#68665f",
            "textTertiary": "#98958b", "borderStrong": "#d3cec2", "divider": "#ece8df",
            "green": "#44745a", "orange": "#b96536", "bgToast": "#171716", "textToast": "#ffffff" },
            "dark": { "brand": "#ec8668", "brandText": "#181412", "bgWindow": "#191918",
            "bgCard": "#232321", "bgInput": "#fffdf5", "bgRaised": "#262522",
            "textPrimary": "#f4f1e9", "textSecondary": "#aaa69c", "textTertiary": "#77746c",
            "borderStrong": "#48463f", "divider": "#2c2b28", "green": "#75aa86",
            "orange": "#dc9163", "bgToast": "#f8f5ed", "textToast": "#171716" } },
          "metrics": { "cardRadius": 10, "controlRadius": 9, "rowPadding": 13, "shadowRadius": 8 },
          "typography": { "sectionTitleSize": 25, "uppercasesSectionTitles": false,
            "searchSize": 18, "searchUsesDisplayFont": true, "historyContentSize": 15 },
          "fonts": { "display": null, "reading": null },
          "background": {
            "light": { "hasImage": true, "scrim": { "hex": "#0f1526", "opacity": 0.82 }, "mimeType": "image/png" },
            "dark": { "hasImage": false }
          } }
        """
        let definition = try JSONDecoder().decode(
            TailSyncThemeDefinition.self, from: Data(json.utf8))
        let background = try XCTUnwrap(definition.background)
        let light = try XCTUnwrap(background.light)
        XCTAssertTrue(light.hasImage)
        XCTAssertEqual(light.scrim?.hex, "#0f1526")
        XCTAssertEqual(light.scrim?.opacity, 0.82)
        XCTAssertEqual(light.mimeType, "image/png")
        let dark = try XCTUnwrap(background.dark)
        XCTAssertFalse(dark.hasImage)
        XCTAssertNil(dark.scrim)
    }

    /// Definitions without a background field stay nil (old format).
    func testDefinitionWithoutBackgroundStaysNil() throws {
        let json = """
        { "id": "studio", "name": { "en": "Studio" },
          "palette": { "light": { "brand": "#d5684b", "brandText": "#ffffff",
            "bgWindow": "#faf9f5", "bgCard": "#fffefa", "bgInput": "#1a1916",
            "bgRaised": "#fffefa", "textPrimary": "#191918", "textSecondary": "#68665f",
            "textTertiary": "#98958b", "borderStrong": "#d3cec2", "divider": "#ece8df",
            "green": "#44745a", "orange": "#b96536", "bgToast": "#171716", "textToast": "#ffffff" },
            "dark": { "brand": "#ec8668", "brandText": "#181412", "bgWindow": "#191918",
            "bgCard": "#232321", "bgInput": "#fffdf5", "bgRaised": "#262522",
            "textPrimary": "#f4f1e9", "textSecondary": "#aaa69c", "textTertiary": "#77746c",
            "borderStrong": "#48463f", "divider": "#2c2b28", "green": "#75aa86",
            "orange": "#dc9163", "bgToast": "#f8f5ed", "textToast": "#171716" } },
          "metrics": { "cardRadius": 10, "controlRadius": 9, "rowPadding": 13, "shadowRadius": 8 },
          "typography": { "sectionTitleSize": 25, "uppercasesSectionTitles": false,
            "searchSize": 18, "searchUsesDisplayFont": true, "historyContentSize": 15 },
          "fonts": { "display": null, "reading": null } }
        """
        let definition = try JSONDecoder().decode(
            TailSyncThemeDefinition.self, from: Data(json.utf8))
        XCTAssertNil(definition.background)
    }

    /// The client-side second line of defence: dimensions are read from the
    /// container metadata before any pixel decode; garbage and impossible
    /// payloads return nil without crashing.
    func testBackgroundSafeDecodeRejectsGarbageAndAcceptsRealPng() {
        XCTAssertNil(Loc.safelyDecodeBackgroundImage(data: Data("not an image".utf8)))
        XCTAssertNil(Loc.safelyDecodeBackgroundImage(data: Data()))

        let pngB64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNgAAH//wMAAf8CAqU9iVkAAAAASUVORK5CYII="
        let data = Data(base64Encoded: pngB64)!
        let image = Loc.safelyDecodeBackgroundImage(data: data)
        XCTAssertNotNil(image)
        XCTAssertEqual(image?.size.width, 1)
        XCTAssertEqual(image?.size.height, 1)
    }
}

extension ThemeTests {
    /// Indicator scrim selection mirrors the Windows semantics: light
    /// preferred, dark fallback, nil when neither mode has an image.
    func testBackgroundIndicatorScrimSelection() throws {
        let base = """
        { "id": "t", "name": { "en": "T" },
          "palette": { "light": { "brand": "#d5684b", "brandText": "#ffffff",
            "bgWindow": "#faf9f5", "bgCard": "#fffefa", "bgInput": "#1a1916",
            "bgRaised": "#fffefa", "textPrimary": "#191918", "textSecondary": "#68665f",
            "textTertiary": "#98958b", "borderStrong": "#d3cec2", "divider": "#ece8df",
            "green": "#44745a", "orange": "#b96536", "bgToast": "#171716", "textToast": "#ffffff" },
            "dark": { "brand": "#ec8668", "brandText": "#181412", "bgWindow": "#191918",
            "bgCard": "#232321", "bgInput": "#fffdf5", "bgRaised": "#262522",
            "textPrimary": "#f4f1e9", "textSecondary": "#aaa69c", "textTertiary": "#77746c",
            "borderStrong": "#48463f", "divider": "#2c2b28", "green": "#75aa86",
            "orange": "#dc9163", "bgToast": "#f8f5ed", "textToast": "#171716" } },
          "metrics": { "cardRadius": 10, "controlRadius": 9, "rowPadding": 13, "shadowRadius": 8 },
          "typography": { "sectionTitleSize": 25, "uppercasesSectionTitles": false,
            "searchSize": 18, "searchUsesDisplayFont": true, "historyContentSize": 15 },
          "fonts": { "display": null, "reading": null },
          "background": %@ }
        """
        func definition(background: String) throws -> TailSyncThemeDefinition {
            try JSONDecoder().decode(
                TailSyncThemeDefinition.self,
                from: Data(String(format: base, background).utf8))
        }

        // Light only → light scrim.
        let lightOnly = try definition(background: """
            { "light": { "hasImage": true, "scrim": { "hex": "#0f1526", "opacity": 0.82 }, "mimeType": "image/png" } }
            """)
        XCTAssertEqual(lightOnly.backgroundIndicatorScrim?.hex, "#0f1526")

        // Dark only → dark scrim.
        let darkOnly = try definition(background: """
            { "light": { "hasImage": false }, "dark": { "hasImage": true, "scrim": { "hex": "#101820", "opacity": 0.9 }, "mimeType": "image/jpeg" } }
            """)
        XCTAssertEqual(darkOnly.backgroundIndicatorScrim?.hex, "#101820")

        // Both → light preferred.
        let both = try definition(background: """
            { "light": { "hasImage": true, "scrim": { "hex": "#0f1526", "opacity": 0.82 }, "mimeType": "image/png" },
              "dark": { "hasImage": true, "scrim": { "hex": "#101820", "opacity": 0.9 }, "mimeType": "image/jpeg" } }
            """)
        XCTAssertEqual(both.backgroundIndicatorScrim?.hex, "#0f1526")

        // Neither → nil.
        let none = try definition(background: """
            { "light": { "hasImage": false }, "dark": { "hasImage": false } }
            """)
        XCTAssertNil(none.backgroundIndicatorScrim)
    }

    // ── R006: font candidate lists ────────────────────────────────────

    func testFontCandidatesParseCommaSeparatedListsInOrder() {
        XCTAssertEqual(
            FontCandidates.parse("Avenir Next, Songti SC"),
            ["Avenir Next", "Songti SC"]
        )
        // Whitespace around separators is trimmed; empty segments are dropped.
        XCTAssertEqual(
            FontCandidates.parse(" Avenir Next ,  Songti SC ,"),
            ["Avenir Next", "Songti SC"]
        )
        // A single value is a one-element candidate list.
        XCTAssertEqual(FontCandidates.parse("Songti SC"), ["Songti SC"])
        // Empty input yields no candidates.
        XCTAssertEqual(FontCandidates.parse(""), [])
        XCTAssertEqual(FontCandidates.parse("   , ,  "), [])
    }

    func testFontCandidatesPickTheFirstAvailableFont() {
        // "Helvetica" is guaranteed installed on every macOS system.
        XCTAssertEqual(FontCandidates.firstAvailable(["Helvetica"]), "Helvetica")
        // The first candidate wins even when later ones also exist.
        XCTAssertEqual(
            FontCandidates.firstAvailable(["Helvetica", "Times New Roman"]),
            "Helvetica"
        )
        // A missing first candidate falls through to an installed one.
        XCTAssertEqual(
            FontCandidates.firstAvailable(["NoSuchFontXYZ", "Helvetica"]),
            "Helvetica"
        )
        // All candidates missing → nil (system font fallback).
        XCTAssertNil(FontCandidates.firstAvailable(["NoSuchFontXYZ", "AlsoMissing"]))
        XCTAssertNil(FontCandidates.firstAvailable([]))
    }

    func testFontCandidatesResolveRawValues() {
        // nil and empty values resolve to nil (system font fallback).
        XCTAssertNil(FontCandidates.resolve(nil))
        XCTAssertNil(FontCandidates.resolve(""))
        XCTAssertNil(FontCandidates.resolve("   ,  "))
        // A list whose head is missing still resolves to an installed tail.
        XCTAssertEqual(
            FontCandidates.resolve("NoSuchFontXYZ, Helvetica"),
            "Helvetica"
        )
        // A raw single value resolves when installed.
        XCTAssertEqual(FontCandidates.resolve("Helvetica"), "Helvetica")
    }

    func testDisplayAndReadingFontsResolveCandidateLists() throws {
        // Decode the same shape the daemon's list_themes entry produces,
        // with a candidate list for display and nil for reading.
        let json = """
        { "id": "candidate", "name": { "en": "Candidate" },
          "palette": { "light": { "brand": "#d5684b", "brandText": "#ffffff",
            "bgWindow": "#faf9f5", "bgCard": "#fffefa", "bgInput": "#1a1916",
            "bgRaised": "#fffefa", "textPrimary": "#191918", "textSecondary": "#68665f",
            "textTertiary": "#98958b", "borderStrong": "#d3cec2", "divider": "#ece8df",
            "green": "#44745a", "orange": "#b96536", "bgToast": "#171716", "textToast": "#ffffff" },
            "dark": { "brand": "#ec8668", "brandText": "#181412", "bgWindow": "#191918",
            "bgCard": "#232321", "bgInput": "#fffdf5", "bgRaised": "#262522",
            "textPrimary": "#f4f1e9", "textSecondary": "#aaa69c", "textTertiary": "#77746c",
            "borderStrong": "#48463f", "divider": "#2c2b28", "green": "#75aa86",
            "orange": "#dc9163", "bgToast": "#f8f5ed", "textToast": "#171716" } },
          "metrics": { "cardRadius": 10, "controlRadius": 9, "rowPadding": 13, "shadowRadius": 8 },
          "typography": { "sectionTitleSize": 25, "uppercasesSectionTitles": false,
            "searchSize": 18, "searchUsesDisplayFont": true, "historyContentSize": 15 },
          "fonts": { "display": "NoSuchFontXYZ, Helvetica", "reading": null } }
        """
        let definition = try JSONDecoder().decode(
            TailSyncThemeDefinition.self, from: Data(json.utf8))
        let selection = TailSyncThemeSelection(builtin: .tailsync, definition: definition)
        // displayFont resolves the first available candidate of the list…
        XCTAssertEqual(FontCandidates.resolve(selection.displayFontName), "Helvetica")
        // …and readingFont keeps the system fallback when no candidate exists.
        XCTAssertNil(FontCandidates.resolve(selection.readingFontName))
    }
}
