import AppKit
import SwiftUI

/// R006: font-candidate list resolution. A theme's `fonts.display` /
/// `fonts.reading` value may be a comma-separated list of candidates
/// (e.g. `"Songti SC, PingFang SC"`): the first candidate actually
/// available on this Mac wins; when none is available the caller falls back
/// to the system font. Parsing is a pure function (unit-tested); the
/// availability probe needs AppKit.
enum FontCandidates {
    /// Split a comma-separated font value into trimmed, non-empty
    /// candidates, preserving order. `"Avenir Next, Songti SC ,"` →
    /// `["Avenir Next", "Songti SC"]`.
    static func parse(_ value: String) -> [String] {
        value
            .split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    }

    /// The first candidate installed on this Mac, or nil when none is.
    static func firstAvailable(_ candidates: [String]) -> String? {
        candidates.first { NSFont(name: $0, size: 12) != nil }
    }

    /// Resolve a raw font value to the first available candidate, or nil
    /// when the value is empty or no candidate is installed.
    static func resolve(_ value: String?) -> String? {
        guard let value else { return nil }
        return firstAvailable(parse(value))
    }
}

/// Built-in theme identities. The public API of this enum (raw values,
/// CaseIterable, `init(storedValue:)`, all accessors) is unchanged; the data
/// behind the accessors now comes from the shared `definition` lookup table
/// so custom runtime themes can reuse the same shape.
enum TailSyncColorTheme: String, CaseIterable, Identifiable {
    case tailsync
    case ocean
    case forest
    case rose
    case highContrast = "high-contrast"

    var id: String { rawValue }

    init(storedValue: String) {
        self = Self(rawValue: storedValue) ?? .tailsync
    }

    var localizationKey: String {
        "settings.colorTheme.\(rawValue)"
    }

    var symbolName: String {
        switch self {
        case .tailsync: return "square.text.square"
        case .ocean: return "wave.3.right"
        case .forest: return "book.closed"
        case .rose: return "sparkles"
        case .highContrast: return "circle.lefthalf.filled"
        }
    }

    /// Built-in theme data as a table (palette x2, metrics, typography,
    /// fonts). Values are the same ones the pre-refactor switch statements
    /// returned; the regression snapshot test locks them field by field.
    var definition: TailSyncThemeDefinition {
        switch self {
        case .tailsync:
            return TailSyncThemeDefinition(
                id: "tailsync",
                name: ["en": "Canvas"],
                lightPalette: TailSyncThemePalette(
                    accent: 0xD5684B, accentContrast: 0xFFFFFF, window: 0xFAF9F5,
                    surface: 0xFFFEFA, softSurface: 0x1A1916, raised: 0xFFFEFA,
                    textPrimary: 0x191918, textSecondary: 0x68665F, textTertiary: 0x98958B,
                    border: 0xD3CEC2, divider: 0xECE8DF, positive: 0x44745A,
                    warning: 0xB96536, toast: 0x171716, toastText: 0xFFFFFF,
                    softSurfaceOpacity: 0.045),
                darkPalette: TailSyncThemePalette(
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
                displayFontName: "Songti SC",
                readingFontName: nil
            )
        case .ocean:
            return TailSyncThemeDefinition(
                id: "ocean",
                name: ["en": "Flux"],
                lightPalette: TailSyncThemePalette(
                    accent: 0x087F8C, accentContrast: 0xFFFFFF, window: 0xF1F7F7,
                    surface: 0xFCFEFE, softSurface: 0x085B65, raised: 0xFFFFFF,
                    textPrimary: 0x051F21, textSecondary: 0x051F21, textTertiary: 0x051F21,
                    border: 0x08474E, divider: 0x08474E, positive: 0x34C759,
                    warning: 0xFF9500, toast: 0x000000, toastText: 0xFFFFFF,
                    softSurfaceOpacity: 0.06, textPrimaryOpacity: 0.90,
                    textSecondaryOpacity: 0.60, textTertiaryOpacity: 0.40,
                    borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.88),
                darkPalette: TailSyncThemePalette(
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
                displayFontName: "Avenir Next",
                readingFontName: "Avenir Next"
            )
        case .forest:
            return TailSyncThemeDefinition(
                id: "forest",
                name: ["en": "Ledger"],
                lightPalette: TailSyncThemePalette(
                    accent: 0x287A55, accentContrast: 0xFFFFFF, window: 0xF3F7F3,
                    surface: 0xFCFDFB, softSurface: 0x275C3E, raised: 0xFFFFFF,
                    textPrimary: 0x13261B, textSecondary: 0x13261B, textTertiary: 0x13261B,
                    border: 0x235235, divider: 0x235235, positive: 0x34C759,
                    warning: 0xFF9500, toast: 0x000000, toastText: 0xFFFFFF,
                    softSurfaceOpacity: 0.06, textPrimaryOpacity: 0.90,
                    textSecondaryOpacity: 0.60, textTertiaryOpacity: 0.40,
                    borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.88),
                darkPalette: TailSyncThemePalette(
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
                displayFontName: "Songti SC",
                readingFontName: "Songti SC"
            )
        case .rose:
            return TailSyncThemeDefinition(
                id: "rose",
                name: ["en": "Aura"],
                lightPalette: TailSyncThemePalette(
                    accent: 0xB83E60, accentContrast: 0xFFFFFF, window: 0xFAF4F6,
                    surface: 0xFFFDFD, softSurface: 0x802A42, raised: 0xFFFFFF,
                    textPrimary: 0x30131B, textSecondary: 0x30131B, textTertiary: 0x30131B,
                    border: 0x71263B, divider: 0x71263B, positive: 0x34C759,
                    warning: 0xFF9500, toast: 0x000000, toastText: 0xFFFFFF,
                    softSurfaceOpacity: 0.055, textPrimaryOpacity: 0.90,
                    textSecondaryOpacity: 0.60, textTertiaryOpacity: 0.40,
                    borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.88),
                darkPalette: TailSyncThemePalette(
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
                displayFontName: nil,
                readingFontName: nil
            )
        case .highContrast:
            return TailSyncThemeDefinition(
                id: "high-contrast",
                name: ["en": "Mono"],
                lightPalette: TailSyncThemePalette(
                    accent: 0x005FCC, accentContrast: 0xFFFFFF, window: 0xFFFFFF,
                    surface: 0xFFFFFF, softSurface: 0xF0F0F0, raised: 0xFFFFFF,
                    textPrimary: 0x000000, textSecondary: 0x333333, textTertiary: 0x5A5A5A,
                    border: 0x8A8A8A, divider: 0xB0B0B0, positive: 0x087A32,
                    warning: 0x9A4D00, toast: 0x000000, toastText: 0xFFFFFF),
                darkPalette: TailSyncThemePalette(
                    accent: 0xFFD400, accentContrast: 0x000000, window: 0x000000,
                    surface: 0x101010, softSurface: 0x1C1C1C, raised: 0x181818,
                    textPrimary: 0xFFFFFF, textSecondary: 0xE3E3E3, textTertiary: 0xB8B8B8,
                    border: 0x777777, divider: 0x595959, positive: 0x65FF8F,
                    warning: 0xFFD166, toast: 0xFFFFFF, toastText: 0x000000),
                metrics: TailSyncThemeMetrics(cardRadius: 0, controlRadius: 0, rowPadding: 12, shadowRadius: 0),
                typography: TailSyncThemeTypography(
                    sectionTitleSize: 12, uppercasesSectionTitles: true,
                    searchSize: 13, searchUsesDisplayFont: false, historyContentSize: 13),
                displayFontName: "Arial",
                readingFontName: "Arial"
            )
        }
    }

    var metrics: TailSyncThemeMetrics { definition.metrics }

    var typography: TailSyncThemeTypography { definition.typography }

    var displayFontName: String? { definition.displayFontName }

    var readingFontName: String? { definition.readingFontName }

    func palette(for scheme: ColorScheme) -> TailSyncThemePalette {
        scheme == .light ? definition.lightPalette : definition.darkPalette
    }

    func displayFont(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        // R006: the stored value may be a comma-separated candidate list;
        // the first available candidate is applied, otherwise the fallback.
        if let resolved = FontCandidates.resolve(displayFontName) {
            return .custom(resolved, size: size).weight(weight)
        }
        switch self {
        case .rose:
            return .system(size: size, weight: weight, design: .rounded)
        default:
            return .system(size: size, weight: weight)
        }
    }

    func readingFont(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        if let resolved = FontCandidates.resolve(readingFontName) {
            return .custom(resolved, size: size).weight(weight)
        }
        switch self {
        case .rose:
            return .system(size: size, weight: weight, design: .rounded)
        default:
            return .system(size: size, weight: weight)
        }
    }
}

/// One colour token in a theme file: a `#rrggbb` hex string, optionally with
/// an opacity in [0, 1]. JSON accepts either a bare string or an object
/// `{ "hex": "...", "opacity": 0.11 }`.
struct ThemeColorSpec: Equatable {
    let hex: String
    let opacity: Double?

    init(hex: String, opacity: Double? = nil) {
        self.hex = hex
        self.opacity = opacity
    }

    private struct ObjectShape: Decodable {
        let hex: String
        let opacity: Double?
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let hex = try? container.decode(String.self) {
            self.init(hex: hex)
            return
        }
        let object = try container.decode(ObjectShape.self)
        self.init(hex: object.hex, opacity: object.opacity)
    }

    /// The palette colour as a UInt32 RGB value, or nil for a malformed hex.
    var rgb: UInt32? {
        let body = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        guard body.count == 6, let value = UInt32(body, radix: 16) else { return nil }
        return value
    }

    /// SwiftUI colour for this validated spec, or nil for a malformed hex.
    var uiColor: Color? {
        guard let rgb else { return nil }
        return Color(rgb: rgb).opacity(opacity ?? 1)
    }
}

extension ThemeColorSpec: Decodable {}

/// A complete theme definition shared by built-in and custom themes:
/// palettes for both modes, metrics, typography, fonts, and localised names.
/// Decodes directly from the daemon's `list_themes` entry JSON (THEMING.md
/// §2.2 token names; palette colours as hex strings or `{hex, opacity}`).
/// Background metadata for one theme mode: image presence, the validated
/// scrim colour, and the payload MIME type. Deliberately carries no image
/// bytes — the decoded payload is fetched on demand.
struct TailSyncThemeBackgroundMeta: Equatable, Decodable {
    let hasImage: Bool
    let scrim: ThemeColorSpec?
    let mimeType: String?
}

/// Per-mode background metadata for a theme definition. Either mode may be
/// absent; the two sides are independent.
struct TailSyncThemeBackground: Equatable, Decodable {
    let light: TailSyncThemeBackgroundMeta?
    let dark: TailSyncThemeBackgroundMeta?

    enum CodingKeys: String, CodingKey {
        case light, dark
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        light = try container.decodeIfPresent(TailSyncThemeBackgroundMeta.self, forKey: .light)
        dark = try container.decodeIfPresent(TailSyncThemeBackgroundMeta.self, forKey: .dark)
    }

    init(light: TailSyncThemeBackgroundMeta?, dark: TailSyncThemeBackgroundMeta?) {
        self.light = light
        self.dark = dark
    }
}

struct TailSyncThemeDefinition: Equatable, Decodable {
    let id: String
    let name: [String: String]
    let lightPalette: TailSyncThemePalette
    let darkPalette: TailSyncThemePalette
    let metrics: TailSyncThemeMetrics
    let typography: TailSyncThemeTypography
    let displayFontName: String?
    let readingFontName: String?
    /// Background metadata per mode (presence/scrim/MIME only — never image
    /// bytes; the payload arrives via ApiClient.getThemeBackground).
    let background: TailSyncThemeBackground?

    func localizedName(preferred: String) -> String {
        name[preferred] ?? name["en"] ?? id
    }

    init(
        id: String,
        name: [String: String],
        lightPalette: TailSyncThemePalette,
        darkPalette: TailSyncThemePalette,
        metrics: TailSyncThemeMetrics,
        typography: TailSyncThemeTypography,
        displayFontName: String?,
        readingFontName: String?,
        background: TailSyncThemeBackground? = nil
    ) {
        self.id = id
        self.name = name
        self.lightPalette = lightPalette
        self.darkPalette = darkPalette
        self.metrics = metrics
        self.typography = typography
        self.displayFontName = displayFontName
        self.readingFontName = readingFontName
        self.background = background
    }

    private enum CodingKeys: String, CodingKey {
        case id, name, palette, metrics, typography, fonts, background
    }

    private enum PaletteKeys: String, CodingKey {
        case light, dark
    }

    private enum FontKeys: String, CodingKey {
        case display, reading
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        name = try container.decode([String: String].self, forKey: .name)
        let paletteContainer = try container.nestedContainer(keyedBy: PaletteKeys.self, forKey: .palette)
        lightPalette = try Self.decodePalette(from: paletteContainer, key: .light)
        darkPalette = try Self.decodePalette(from: paletteContainer, key: .dark)
        metrics = try container.decode(TailSyncThemeMetrics.self, forKey: .metrics)
        typography = try container.decode(TailSyncThemeTypography.self, forKey: .typography)
        let fonts = try container.nestedContainer(keyedBy: FontKeys.self, forKey: .fonts)
        displayFontName = try fonts.decodeIfPresent(String.self, forKey: .display)
        readingFontName = try fonts.decodeIfPresent(String.self, forKey: .reading)
        background = try container.decodeIfPresent(
            TailSyncThemeBackground.self, forKey: .background)
    }

    /// Map the CSS token names (THEMING.md §2.2) onto the Swift palette.
    /// Missing or malformed required tokens reject the whole definition so a
    /// broken entry can never render as a half-applied theme.
    private static func decodePalette(
        from container: KeyedDecodingContainer<PaletteKeys>,
        key: PaletteKeys
    ) throws -> TailSyncThemePalette {
        let tokens = try container.decode([String: ThemeColorSpec].self, forKey: key)
        func required(_ token: String) throws -> ThemeColorSpec {
            guard let spec = tokens[token] else {
                throw DecodingError.keyNotFound(
                    DynamicKey(token),
                    DecodingError.Context(
                        codingPath: container.codingPath,
                        debugDescription: "Palette is missing token \(token)"
                    )
                )
            }
            guard spec.rgb != nil else {
                throw DecodingError.dataCorrupted(
                    DecodingError.Context(
                        codingPath: container.codingPath,
                        debugDescription: "Palette token \(token) has invalid hex \(spec.hex)"
                    )
                )
            }
            return spec
        }
        func color(_ token: String) throws -> UInt32 {
            try required(token).rgb!
        }
        func opacity(_ token: String) throws -> Double {
            try required(token).opacity ?? 1
        }
        return TailSyncThemePalette(
            accent: try color("brand"),
            accentContrast: try color("brandText"),
            window: try color("bgWindow"),
            surface: try color("bgCard"),
            softSurface: try color("bgInput"),
            raised: try color("bgRaised"),
            textPrimary: try color("textPrimary"),
            textSecondary: try color("textSecondary"),
            textTertiary: try color("textTertiary"),
            border: try color("borderStrong"),
            divider: try color("divider"),
            positive: try color("green"),
            warning: try color("orange"),
            toast: try color("bgToast"),
            toastText: try color("textToast"),
            softSurfaceOpacity: try opacity("bgInput"),
            textPrimaryOpacity: try opacity("textPrimary"),
            textSecondaryOpacity: try opacity("textSecondary"),
            textTertiaryOpacity: try opacity("textTertiary"),
            borderOpacity: try opacity("borderStrong"),
            dividerOpacity: try opacity("divider"),
            toastOpacity: try opacity("bgToast")
        )
    }

    private struct DynamicKey: CodingKey {
        let stringValue: String
        init(_ string: String) { self.stringValue = string }
        init?(stringValue: String) { self.stringValue = stringValue }
        init?(intValue: Int) { nil }
        var intValue: Int? { nil }
    }
}

extension TailSyncThemeDefinition {
    /// Scrim of any mode that carries an image (light preferred), for the
    /// settings-page background indicator. Metadata only — never triggers an
    /// image fetch. Mirrors the Windows-side `backgroundIndicator` semantics.
    var backgroundIndicatorScrim: ThemeColorSpec? {
        if let light = background?.light, light.hasImage, light.scrim != nil {
            return light.scrim
        }
        if let dark = background?.dark, dark.hasImage, dark.scrim != nil {
            return dark.scrim
        }
        return nil
    }
}

/// A resolved theme selection: a built-in theme, or a custom theme carrying
/// its definition from the daemon catalogue. Stored values use the
/// `custom:{id}` namespace; unknown stored values fall back to the default
/// theme at apply time (the stored value itself is never rewritten).
struct TailSyncThemeSelection: Equatable {
    let builtin: TailSyncColorTheme
    let definition: TailSyncThemeDefinition?

    init(builtin: TailSyncColorTheme, definition: TailSyncThemeDefinition? = nil) {
        self.builtin = builtin
        self.definition = definition
    }

    init(storedValue: String, catalogue: [TailSyncThemeDefinition]) {
        if let definition = catalogue.first(where: { "custom:\($0.id)" == storedValue }) {
            self.init(builtin: .tailsync, definition: definition)
        } else if let builtin = TailSyncColorTheme(rawValue: storedValue) {
            self.init(builtin: builtin)
        } else {
            self.init(builtin: .tailsync)
        }
    }

    /// Storage namespace value (`custom:{id}` for custom themes).
    var id: String {
        definition.map { "custom:\($0.id)" } ?? builtin.rawValue
    }

    var localizationKey: String {
        definition.map { "settings.colorTheme.\($0.id)" } ?? builtin.localizationKey
    }

    var symbolName: String {
        builtin.symbolName
    }

    var metrics: TailSyncThemeMetrics {
        definition?.metrics ?? builtin.metrics
    }

    var typography: TailSyncThemeTypography {
        definition?.typography ?? builtin.typography
    }

    var displayFontName: String? {
        definition?.displayFontName ?? builtin.displayFontName
    }

    var readingFontName: String? {
        definition?.readingFontName ?? builtin.readingFontName
    }

    func palette(for scheme: ColorScheme) -> TailSyncThemePalette {
        if let definition {
            return scheme == .light ? definition.lightPalette : definition.darkPalette
        }
        return builtin.palette(for: scheme)
    }

    func displayFont(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        if let displayFontName {
            return .custom(displayFontName, size: size).weight(weight)
        }
        if definition == nil && builtin == .rose {
            return .system(size: size, weight: weight, design: .rounded)
        }
        return .system(size: size, weight: weight)
    }

    func readingFont(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        if let readingFontName {
            return .custom(readingFontName, size: size).weight(weight)
        }
        if definition == nil && builtin == .rose {
            return .system(size: size, weight: weight, design: .rounded)
        }
        return .system(size: size, weight: weight)
    }
}

struct TailSyncThemeMetrics: Equatable, Decodable {
    let cardRadius: CGFloat
    let controlRadius: CGFloat
    let rowPadding: CGFloat
    let shadowRadius: CGFloat
}

struct TailSyncThemeTypography: Equatable, Decodable {
    let sectionTitleSize: CGFloat
    let uppercasesSectionTitles: Bool
    let searchSize: CGFloat
    let searchUsesDisplayFont: Bool
    let historyContentSize: CGFloat
}

struct TailSyncThemePalette: Equatable {
    let accent: UInt32
    let accentContrast: UInt32
    let window: UInt32
    let surface: UInt32
    let softSurface: UInt32
    let raised: UInt32
    let textPrimary: UInt32
    let textSecondary: UInt32
    let textTertiary: UInt32
    let border: UInt32
    let divider: UInt32
    let positive: UInt32
    let warning: UInt32
    let toast: UInt32
    let toastText: UInt32
    let softSurfaceOpacity: Double
    let textPrimaryOpacity: Double
    let textSecondaryOpacity: Double
    let textTertiaryOpacity: Double
    let borderOpacity: Double
    let dividerOpacity: Double
    let toastOpacity: Double

    init(
        accent: UInt32,
        accentContrast: UInt32,
        window: UInt32,
        surface: UInt32,
        softSurface: UInt32,
        raised: UInt32,
        textPrimary: UInt32,
        textSecondary: UInt32,
        textTertiary: UInt32,
        border: UInt32,
        divider: UInt32,
        positive: UInt32,
        warning: UInt32,
        toast: UInt32,
        toastText: UInt32,
        softSurfaceOpacity: Double = 1,
        textPrimaryOpacity: Double = 1,
        textSecondaryOpacity: Double = 1,
        textTertiaryOpacity: Double = 1,
        borderOpacity: Double = 1,
        dividerOpacity: Double = 1,
        toastOpacity: Double = 1
    ) {
        self.accent = accent
        self.accentContrast = accentContrast
        self.window = window
        self.surface = surface
        self.softSurface = softSurface
        self.raised = raised
        self.textPrimary = textPrimary
        self.textSecondary = textSecondary
        self.textTertiary = textTertiary
        self.border = border
        self.divider = divider
        self.positive = positive
        self.warning = warning
        self.toast = toast
        self.toastText = toastText
        self.softSurfaceOpacity = softSurfaceOpacity
        self.textPrimaryOpacity = textPrimaryOpacity
        self.textSecondaryOpacity = textSecondaryOpacity
        self.textTertiaryOpacity = textTertiaryOpacity
        self.borderOpacity = borderOpacity
        self.dividerOpacity = dividerOpacity
        self.toastOpacity = toastOpacity
    }

    var signature: String {
        [accent, window, surface, textPrimary, border].map { String($0, radix: 16) }.joined(separator: ":")
    }

    var accentColor: Color { Color(rgb: accent) }
    var accentContrastColor: Color { Color(rgb: accentContrast) }
    var windowColor: Color { Color(rgb: window) }
    var surfaceColor: Color { Color(rgb: surface) }
    var softSurfaceColor: Color { Color(rgb: softSurface).opacity(softSurfaceOpacity) }
    var raisedColor: Color { Color(rgb: raised) }
    var primaryColor: Color { Color(rgb: textPrimary).opacity(textPrimaryOpacity) }
    var secondaryColor: Color { Color(rgb: textSecondary).opacity(textSecondaryOpacity) }
    var tertiaryColor: Color { Color(rgb: textTertiary).opacity(textTertiaryOpacity) }
    var borderColor: Color { Color(rgb: border).opacity(borderOpacity) }
    var dividerColor: Color { Color(rgb: divider).opacity(dividerOpacity) }
    var positiveColor: Color { Color(rgb: positive) }
    var warningColor: Color { Color(rgb: warning) }
    var toastColor: Color { Color(rgb: toast).opacity(toastOpacity) }
    var toastTextColor: Color { Color(rgb: toastText) }
}

private struct TailSyncThemeKey: EnvironmentKey {
    static let defaultValue = TailSyncColorTheme.tailsync
}

private struct TailSyncPaletteKey: EnvironmentKey {
    static let defaultValue = TailSyncColorTheme.tailsync.palette(for: .light)
}

private struct TailSyncSelectionKey: EnvironmentKey {
    static let defaultValue = TailSyncThemeSelection(builtin: .tailsync)
}

struct TailSyncWindowBackground: View {
    let palette: TailSyncThemePalette
    let image: NSImage?
    let scrim: Color?

    var body: some View {
        ZStack {
            palette.windowColor
            if let image {
                Image(nsImage: image)
                    .resizable()
                    .scaledToFill()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .clipped()
            }
            if let scrim {
                scrim
            }
        }
        .ignoresSafeArea()
    }
}

extension EnvironmentValues {
    var tailSyncTheme: TailSyncColorTheme {
        get { self[TailSyncThemeKey.self] }
        set { self[TailSyncThemeKey.self] = newValue }
    }

    var tailSyncPalette: TailSyncThemePalette {
        get { self[TailSyncPaletteKey.self] }
        set { self[TailSyncPaletteKey.self] = newValue }
    }

    var tailSyncSelection: TailSyncThemeSelection {
        get { self[TailSyncSelectionKey.self] }
        set { self[TailSyncSelectionKey.self] = newValue }
    }
}

private struct TailSyncThemeModifier: ViewModifier {
    @ObservedObject private var loc = Loc.shared
    @Environment(\.colorScheme) private var colorScheme

    func body(content: Content) -> some View {
        // Resolve the stored value against the custom-theme catalogue;
        // unknown custom ids fall back to the default theme at apply time.
        let selection = TailSyncThemeSelection(
            storedValue: loc.colorTheme,
            catalogue: loc.customThemes
        )
        let palette = selection.palette(for: colorScheme)
        let light = colorScheme == .light
        let backgroundMode = light ? "light" : "dark"
        return content
            .environment(\.tailSyncTheme, selection.builtin)
            .environment(\.tailSyncPalette, palette)
            .environment(\.tailSyncSelection, selection)
            .tint(palette.accentColor)
            .foregroundStyle(palette.primaryColor)
            .background(
                // Layered window background: window colour at the bottom,
                // then the custom theme's image (cover) and its scrim on
                // top. With no image loaded this is exactly the previous
                // `palette.windowColor` background.
                TailSyncWindowBackground(
                    palette: palette,
                    image: loc.themeBackgroundImage,
                    scrim: loc.themeBackgroundScrim
                )
            )
            .task(id: "\(selection.id):\(backgroundMode)") {
                await loc.loadThemeBackground(for: selection, light: light)
            }
    }
}

extension View {
    func tailSyncThemed() -> some View {
        modifier(TailSyncThemeModifier())
    }
}

private extension Color {
    init(rgb: UInt32) {
        self.init(
            red: Double((rgb >> 16) & 0xFF) / 255,
            green: Double((rgb >> 8) & 0xFF) / 255,
            blue: Double(rgb & 0xFF) / 255
        )
    }
}
