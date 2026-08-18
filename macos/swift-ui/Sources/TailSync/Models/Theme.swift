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
/// Swift-native adapter for already-resolved V2 tokens. Core is the sole
/// parser for theme packages; this type intentionally has no V1 decoder.
struct TailSyncThemeComponentTokens: Equatable {
    let background: UInt32?
    let backgroundOpacity: Double?
    let foreground: UInt32?
    let foregroundOpacity: Double?
    let secondaryText: UInt32?
    let secondaryTextOpacity: Double?
    let border: UInt32?
    let borderOpacity: Double?
    let focusRing: UInt32?
    let focusRingOpacity: Double?
    let icon: UInt32?
    let iconOpacity: Double?
    let accent: UInt32?
    let accentOpacity: Double?
    let radius: CGFloat?
    let padding: CGFloat?
    let spacing: CGFloat?
    let fontSize: CGFloat?
    let fontWeight: CGFloat?
    let shadowRadius: CGFloat?
    let shadowY: CGFloat?
    let shadowOpacity: CGFloat?

    var backgroundColor: Color? { background.map { Color(rgb: $0).opacity(backgroundOpacity ?? 1) } }
    var foregroundColor: Color? { foreground.map { Color(rgb: $0).opacity(foregroundOpacity ?? 1) } }
    var secondaryTextColor: Color? { secondaryText.map { Color(rgb: $0).opacity(secondaryTextOpacity ?? 1) } }
    var borderColor: Color? { border.map { Color(rgb: $0).opacity(borderOpacity ?? 1) } }
    var focusRingColor: Color? { focusRing.map { Color(rgb: $0).opacity(focusRingOpacity ?? 1) } }
    var iconColor: Color? { icon.map { Color(rgb: $0).opacity(iconOpacity ?? 1) } }
    var accentColor: Color? { accent.map { Color(rgb: $0).opacity(accentOpacity ?? 1) } }

    var withReducedTransparency: TailSyncThemeComponentTokens {
        TailSyncThemeComponentTokens(
            background: background,
            backgroundOpacity: background == nil ? nil : 1,
            foreground: foreground,
            foregroundOpacity: foreground == nil ? nil : 1,
            secondaryText: secondaryText,
            secondaryTextOpacity: secondaryText == nil ? nil : 1,
            border: border,
            borderOpacity: border == nil ? nil : 1,
            focusRing: focusRing,
            focusRingOpacity: focusRing == nil ? nil : 1,
            icon: icon,
            iconOpacity: icon == nil ? nil : 1,
            accent: accent,
            accentOpacity: accent == nil ? nil : 1,
            radius: radius,
            padding: padding,
            spacing: spacing,
            fontSize: fontSize,
            fontWeight: fontWeight,
            shadowRadius: 0,
            shadowY: 0,
            shadowOpacity: 0
        )
    }
}

struct TailSyncThemeDefinition: Equatable {
    let id: String
    let packageDigest: String?
    let name: [String: String]
    let lightPalette: TailSyncThemePalette
    let darkPalette: TailSyncThemePalette
    let metrics: TailSyncThemeMetrics
    let darkMetrics: TailSyncThemeMetrics
    let typography: TailSyncThemeTypography
    let darkTypography: TailSyncThemeTypography
    let displayFontName: String?
    let darkDisplayFontName: String?
    let readingFontName: String?
    let darkReadingFontName: String?
    let components: [String: [String: TailSyncThemeComponentTokens]]
    let darkComponents: [String: [String: TailSyncThemeComponentTokens]]
    let assetSlots: [String: TailSyncThemeAssetDescriptor]
    func localizedName(preferred: String) -> String {
        name[preferred] ?? name["en"] ?? id
    }

    init(
        id: String,
        name: [String: String],
        packageDigest: String? = nil,
        lightPalette: TailSyncThemePalette,
        darkPalette: TailSyncThemePalette,
        metrics: TailSyncThemeMetrics,
        darkMetrics: TailSyncThemeMetrics? = nil,
        typography: TailSyncThemeTypography,
        darkTypography: TailSyncThemeTypography? = nil,
        displayFontName: String?,
        darkDisplayFontName: String? = nil,
        readingFontName: String?,
        darkReadingFontName: String? = nil,
        components: [String: [String: TailSyncThemeComponentTokens]] = [:],
        darkComponents: [String: [String: TailSyncThemeComponentTokens]] = [:],
        assetSlots: [String: TailSyncThemeAssetDescriptor] = [:]
    ) {
        self.id = id
        self.name = name
        self.packageDigest = packageDigest
        self.lightPalette = lightPalette
        self.darkPalette = darkPalette
        self.metrics = metrics
        self.darkMetrics = darkMetrics ?? metrics
        self.typography = typography
        self.darkTypography = darkTypography ?? typography
        self.displayFontName = displayFontName
        self.darkDisplayFontName = darkDisplayFontName ?? displayFontName
        self.readingFontName = readingFontName
        self.darkReadingFontName = darkReadingFontName ?? readingFontName
        self.components = components
        self.darkComponents = darkComponents
        self.assetSlots = assetSlots
    }

}

struct TailSyncThemeAssetDescriptor: Equatable {
    let slot: String
    let key: String
    let digest: String
    let mimeType: String
    let bytes: Int
    let width: Int
    let height: Int
}

extension TailSyncThemeDefinition {
    static func resolvedV2(id: String, packageDigest: String? = nil, light: [String: Any], dark: [String: Any], assetSlots: [String: TailSyncThemeAssetDescriptor] = [:]) -> TailSyncThemeDefinition {
        func palette(_ tokens: [String: Any], fallback: TailSyncThemePalette) -> TailSyncThemePalette {
            func parsed(_ value: Any?) -> (UInt32, Double)? {
                guard let text = value as? String else { return nil }
                if text == "system", let accent = NSColor.controlAccentColor.usingColorSpace(.deviceRGB) {
                    return ((UInt32((accent.redComponent * 255).rounded()) << 16) | (UInt32((accent.greenComponent * 255).rounded()) << 8) | UInt32((accent.blueComponent * 255).rounded()), Double(accent.alphaComponent))
                }
                if text.hasPrefix("#") {
                    let hex = String(text.dropFirst())
                    guard hex.count == 6 || hex.count == 8, let value = UInt32(hex, radix: 16) else { return nil }
                    return hex.count == 8 ? (value >> 8, Double(value & 0xFF) / 255) : (value, 1)
                }
                guard text.hasPrefix("rgba("), text.hasSuffix(")") else { return nil }
                let parts = text.dropFirst(5).dropLast().split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
                guard parts.count == 4, let r = Double(parts[0]), let g = Double(parts[1]), let b = Double(parts[2]), let alpha = Double(parts[3]), (0...255).contains(r), (0...255).contains(g), (0...255).contains(b), (0...1).contains(alpha) else { return nil }
                return ((UInt32(r.rounded()) << 16) | (UInt32(g.rounded()) << 8) | UInt32(b.rounded()), alpha)
            }
            func value(_ path: [String]) -> (UInt32, Double)? { var raw: Any = tokens; for key in path { guard let map = raw as? [String: Any], let next = map[key] else { return nil }; raw = next }; return parsed(raw) }
            func color(_ path: [String], _ fallback: UInt32) -> UInt32 { value(path)?.0 ?? fallback }
            func opacity(_ path: [String], _ fallback: Double) -> Double { value(path)?.1 ?? fallback }
            return TailSyncThemePalette(
                accent: color(["colors", "accent", "default"], fallback.accent),
                accentContrast: color(["colors", "accent", "onAccent"], fallback.accentContrast),
                window: color(["colors", "background", "canvas"], fallback.window),
                surface: color(["colors", "background", "surface"], fallback.surface),
                softSurface: color(["colors", "background", "input"], fallback.softSurface),
                raised: color(["colors", "background", "raised"], fallback.raised),
                textPrimary: color(["colors", "text", "primary"], fallback.textPrimary),
                textSecondary: color(["colors", "text", "secondary"], fallback.textSecondary),
                textTertiary: color(["colors", "text", "tertiary"], fallback.textTertiary),
                border: color(["colors", "border", "default"], fallback.border),
                divider: color(["colors", "border", "divider"], fallback.divider),
                positive: color(["colors", "status", "positive"], fallback.positive),
                warning: color(["colors", "status", "warning"], fallback.warning),
                toast: color(["colors", "background", "toast"], fallback.toast),
                toastText: color(["colors", "text", "toast"], fallback.toastText),
                accentOpacity: opacity(["colors", "accent", "default"], fallback.accentOpacity),
                accentContrastOpacity: opacity(["colors", "accent", "onAccent"], fallback.accentContrastOpacity),
                windowOpacity: opacity(["colors", "background", "canvas"], fallback.windowOpacity),
                surfaceOpacity: opacity(["colors", "background", "surface"], fallback.surfaceOpacity),
                softSurfaceOpacity: opacity(["colors", "background", "input"], fallback.softSurfaceOpacity),
                raisedOpacity: opacity(["colors", "background", "raised"], fallback.raisedOpacity),
                textPrimaryOpacity: opacity(["colors", "text", "primary"], fallback.textPrimaryOpacity),
                textSecondaryOpacity: opacity(["colors", "text", "secondary"], fallback.textSecondaryOpacity),
                textTertiaryOpacity: opacity(["colors", "text", "tertiary"], fallback.textTertiaryOpacity),
                borderOpacity: opacity(["colors", "border", "default"], fallback.borderOpacity),
                dividerOpacity: opacity(["colors", "border", "divider"], fallback.dividerOpacity),
                positiveOpacity: opacity(["colors", "status", "positive"], fallback.positiveOpacity),
                warningOpacity: opacity(["colors", "status", "warning"], fallback.warningOpacity),
                toastOpacity: opacity(["colors", "background", "toast"], fallback.toastOpacity),
                toastTextOpacity: opacity(["colors", "text", "toast"], fallback.toastTextOpacity),
                accentHover: color(["colors", "accent", "hover"], fallback.accentHover),
                accentSoft: color(["colors", "accent", "soft"], fallback.accentSoft),
                borderStrong: color(["colors", "border", "strong"], fallback.borderStrong),
                positiveSoft: color(["colors", "status", "positiveSoft"], fallback.positiveSoft),
                warningSoft: color(["colors", "status", "warningSoft"], fallback.warningSoft),
                info: color(["colors", "status", "info"], fallback.info),
                infoSoft: color(["colors", "status", "infoSoft"], fallback.infoSoft),
                hover: color(["colors", "background", "hover"], fallback.hover),
                active: color(["colors", "background", "active"], fallback.active),
                accentHoverOpacity: opacity(["colors", "accent", "hover"], fallback.accentHoverOpacity),
                accentSoftOpacity: opacity(["colors", "accent", "soft"], fallback.accentSoftOpacity),
                borderStrongOpacity: opacity(["colors", "border", "strong"], fallback.borderStrongOpacity),
                positiveSoftOpacity: opacity(["colors", "status", "positiveSoft"], fallback.positiveSoftOpacity),
                warningSoftOpacity: opacity(["colors", "status", "warningSoft"], fallback.warningSoftOpacity),
                infoOpacity: opacity(["colors", "status", "info"], fallback.infoOpacity),
                infoSoftOpacity: opacity(["colors", "status", "infoSoft"], fallback.infoSoftOpacity),
                hoverOpacity: opacity(["colors", "background", "hover"], fallback.hoverOpacity),
                activeOpacity: opacity(["colors", "background", "active"], fallback.activeOpacity)
            )
        }
        func number(_ tokens: [String: Any], _ path: [String], _ fallback: CGFloat) -> CGFloat { var value: Any = tokens; for key in path { guard let map = value as? [String: Any], let next = map[key] else { return fallback }; value = next }; return (value as? NSNumber).map { CGFloat($0.doubleValue) } ?? fallback }
        let base = TailSyncColorTheme.tailsync
        func families(_ tokens: [String: Any], _ path: [String]) -> String? { var value: Any = tokens; for key in path { guard let map=value as? [String: Any], let next=map[key] else { return nil }; value=next }; guard let list=value as? [String], !list.isEmpty else { return nil }; return list.joined(separator: ", ") }
        func bool(_ tokens: [String: Any], _ path: [String], _ fallback: Bool) -> Bool { var value: Any=tokens; for key in path { guard let map=value as? [String: Any], let next=map[key] else { return fallback }; value=next }; return value as? Bool ?? fallback }
        func components(_ tokens: [String: Any]) -> [String: [String: TailSyncThemeComponentTokens]] {
            guard let raw = tokens["components"] as? [String: Any] else { return [:] }
            func color(_ value: Any?) -> (UInt32, Double)? { guard let text = value as? String else { return nil }; if text == "system", let accent = NSColor.controlAccentColor.usingColorSpace(.deviceRGB) { return ((UInt32((accent.redComponent * 255).rounded()) << 16) | (UInt32((accent.greenComponent * 255).rounded()) << 8) | UInt32((accent.blueComponent * 255).rounded()), Double(accent.alphaComponent)) }; if text.hasPrefix("#") { let hex = String(text.dropFirst()); guard (hex.count == 6 || hex.count == 8), let value = UInt32(hex, radix: 16) else { return nil }; return hex.count == 8 ? (value >> 8, Double(value & 0xFF) / 255) : (value, 1) }; guard text.hasPrefix("rgba("), text.hasSuffix(")") else { return nil }; let parts = text.dropFirst(5).dropLast().split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }; guard parts.count == 4, let r = Double(parts[0]), let g = Double(parts[1]), let b = Double(parts[2]), let alpha = Double(parts[3]), (0...255).contains(r), (0...255).contains(g), (0...255).contains(b), (0...1).contains(alpha) else { return nil }; return ((UInt32(r.rounded()) << 16) | (UInt32(g.rounded()) << 8) | UInt32(b.rounded()), alpha) }
            var result: [String: [String: TailSyncThemeComponentTokens]] = [:]
            for (name, statesValue) in raw {
                guard let states = statesValue as? [String: Any] else { continue }
                var mapped: [String: TailSyncThemeComponentTokens] = [:]
                for (state, fieldsValue) in states {
                    guard let fields = fieldsValue as? [String: Any] else { continue }
                    let number: (String) -> CGFloat? = { key in (fields[key] as? NSNumber).map { CGFloat($0.doubleValue) } }
                    let typography = fields["typography"] as? [String: Any]
                    let shadow = fields["shadow"] as? [String: Any]
                    let nestedNumber: ([String: Any]?, String) -> CGFloat? = { values, key in
                        (values?[key] as? NSNumber).map { CGFloat($0.doubleValue) }
                    }
                    let background = color(fields["background"]), foreground = color(fields["foreground"]), secondaryText = color(fields["secondaryText"]), border = color(fields["border"]), focusRing = color(fields["focusRing"]), icon = color(fields["icon"]), accent = color(fields["accent"])
                    mapped[state] = TailSyncThemeComponentTokens(
                        background: background?.0, backgroundOpacity: background?.1,
                        foreground: foreground?.0, foregroundOpacity: foreground?.1,
                        secondaryText: secondaryText?.0, secondaryTextOpacity: secondaryText?.1,
                        border: border?.0, borderOpacity: border?.1,
                        focusRing: focusRing?.0, focusRingOpacity: focusRing?.1,
                        icon: icon?.0, iconOpacity: icon?.1,
                        accent: accent?.0, accentOpacity: accent?.1,
                        radius: number("radius"),
                        padding: number("padding"),
                        spacing: number("spacing"),
                        fontSize: nestedNumber(typography, "size"),
                        fontWeight: nestedNumber(typography, "weight"),
                        shadowRadius: nestedNumber(shadow, "radius"),
                        shadowY: nestedNumber(shadow, "y"),
                        shadowOpacity: nestedNumber(shadow, "opacity")
                    )
                }
                result[name] = mapped
            }
            return result
        }
        func metrics(_ tokens: [String: Any]) -> TailSyncThemeMetrics {
            TailSyncThemeMetrics(cardRadius: number(tokens, ["shape","surfaceRadius"], base.metrics.cardRadius), controlRadius: number(tokens, ["shape","controlRadius"], base.metrics.controlRadius), rowPadding: number(tokens, ["density","row"], base.metrics.rowPadding), shadowRadius: number(tokens, ["effects","shadow","radius"], base.metrics.shadowRadius))
        }
        func typography(_ tokens: [String: Any]) -> TailSyncThemeTypography {
            TailSyncThemeTypography(sectionTitleSize: number(tokens, ["typography","section","size"], base.typography.sectionTitleSize), uppercasesSectionTitles: bool(tokens, ["typography","section","uppercase"], base.typography.uppercasesSectionTitles), searchSize: number(tokens, ["typography","search","size"], base.typography.searchSize), searchUsesDisplayFont: bool(tokens, ["typography","search","useDisplayFont"], false), historyContentSize: number(tokens, ["typography","history","size"], base.typography.historyContentSize))
        }
        return TailSyncThemeDefinition(id: id, name: ["en": id], packageDigest: packageDigest, lightPalette: palette(light, fallback: base.palette(for: .light)), darkPalette: palette(dark, fallback: base.palette(for: .dark)), metrics: metrics(light), darkMetrics: metrics(dark), typography: typography(light), darkTypography: typography(dark), displayFontName: families(light, ["typography","display","families"]), darkDisplayFontName: families(dark, ["typography","display","families"]), readingFontName: families(light, ["typography","reading","families"]), darkReadingFontName: families(dark, ["typography","reading","families"]), components: components(light), darkComponents: components(dark), assetSlots: assetSlots)
    }
}

/// A resolved theme selection: a built-in theme, or a custom theme carrying
/// its definition from the daemon catalogue. Stored values use the
/// `custom:{id}` namespace; unknown stored values fall back to the default
/// theme at apply time (the stored value itself is never rewritten).
struct TailSyncThemeSelection: Equatable {
    let builtin: TailSyncColorTheme
    let definition: TailSyncThemeDefinition?
    let reduceTransparency: Bool
    let interfaceScale: CGFloat

    init(
        builtin: TailSyncColorTheme,
        definition: TailSyncThemeDefinition? = nil,
        reduceTransparency: Bool = false,
        interfaceScale: CGFloat = 1
    ) {
        self.builtin = builtin
        self.definition = definition
        self.reduceTransparency = reduceTransparency
        self.interfaceScale = interfaceScale
    }

    init(
        storedValue: String,
        catalogue: [TailSyncThemeDefinition],
        reduceTransparency: Bool = false,
        interfaceScale: CGFloat = 1
    ) {
        if let definition = catalogue.first(where: { "custom:\($0.id)" == storedValue || $0.id == storedValue }) {
            self.init(builtin: .tailsync, definition: definition, reduceTransparency: reduceTransparency, interfaceScale: interfaceScale)
        } else if let builtin = TailSyncColorTheme(rawValue: storedValue) {
            self.init(builtin: builtin, reduceTransparency: reduceTransparency, interfaceScale: interfaceScale)
        } else {
            self.init(builtin: .tailsync, reduceTransparency: reduceTransparency, interfaceScale: interfaceScale)
        }
    }

    /// Storage namespace value (`custom:{id}` for custom themes).
    var id: String {
        definition.map { $0.id.hasPrefix("custom:") ? $0.id : "custom:\($0.id)" } ?? builtin.rawValue
    }

    var localizationKey: String {
        definition.map { "settings.colorTheme.\($0.id)" } ?? builtin.localizationKey
    }

    var symbolName: String {
        builtin.symbolName
    }

    var metrics: TailSyncThemeMetrics {
        metrics(for: currentColorScheme)
    }

    var typography: TailSyncThemeTypography {
        typography(for: currentColorScheme)
    }

    var displayFontName: String? {
        displayFontName(for: currentColorScheme)
    }

    var readingFontName: String? {
        readingFontName(for: currentColorScheme)
    }

    func metrics(for scheme: ColorScheme) -> TailSyncThemeMetrics {
        let metrics: TailSyncThemeMetrics
        if let definition {
            metrics = scheme == .dark ? definition.darkMetrics : definition.metrics
        } else {
            metrics = builtin.metrics
        }
        return metrics.scaled(by: interfaceScale, removeShadow: reduceTransparency)
    }

    func typography(for scheme: ColorScheme) -> TailSyncThemeTypography {
        let typography: TailSyncThemeTypography
        if let definition {
            typography = scheme == .dark ? definition.darkTypography : definition.typography
        } else {
            typography = builtin.typography
        }
        return typography
    }

    func displayFontName(for scheme: ColorScheme) -> String? {
        guard let definition else { return builtin.displayFontName }
        return scheme == .dark ? definition.darkDisplayFontName : definition.displayFontName
    }

    func readingFontName(for scheme: ColorScheme) -> String? {
        guard let definition else { return builtin.readingFontName }
        return scheme == .dark ? definition.darkReadingFontName : definition.readingFontName
    }

    private var currentColorScheme: ColorScheme {
        NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? .dark : .light
    }

    func component(_ name: String, state: String = "default", scheme: ColorScheme = .light) -> TailSyncThemeComponentTokens? {
        let values = scheme == .light ? definition?.components : definition?.darkComponents
        let component = values?[name]?[state]
        return reduceTransparency ? component?.withReducedTransparency : component
    }

    func palette(for scheme: ColorScheme) -> TailSyncThemePalette {
        let palette = if let definition {
            scheme == .light ? definition.lightPalette : definition.darkPalette
        } else {
            builtin.palette(for: scheme)
        }
        return reduceTransparency ? palette.withReducedTransparency : palette
    }

    func displayFont(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        let size = size * interfaceScale
        if let resolved = FontCandidates.resolve(displayFontName) {
            return .custom(resolved, size: size).weight(weight)
        }
        if definition == nil && builtin == .rose {
            return .system(size: size, weight: weight, design: .rounded)
        }
        return .system(size: size, weight: weight)
    }

    func readingFont(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        let size = size * interfaceScale
        if let resolved = FontCandidates.resolve(readingFontName) {
            return .custom(resolved, size: size).weight(weight)
        }
        if definition == nil && builtin == .rose {
            return .system(size: size, weight: weight, design: .rounded)
        }
        return .system(size: size, weight: weight)
    }
}

struct TailSyncThemeMetrics: Equatable {
    let cardRadius: CGFloat
    let controlRadius: CGFloat
    let rowPadding: CGFloat
    let shadowRadius: CGFloat

    func scaled(by scale: CGFloat, removeShadow: Bool) -> TailSyncThemeMetrics {
        TailSyncThemeMetrics(
            cardRadius: cardRadius * scale,
            controlRadius: controlRadius * scale,
            rowPadding: rowPadding * scale,
            shadowRadius: removeShadow ? 0 : shadowRadius * scale
        )
    }
}

struct TailSyncThemeTypography: Equatable {
    let sectionTitleSize: CGFloat
    let uppercasesSectionTitles: Bool
    let searchSize: CGFloat
    let searchUsesDisplayFont: Bool
    let historyContentSize: CGFloat

    func scaled(by scale: CGFloat) -> TailSyncThemeTypography {
        TailSyncThemeTypography(
            sectionTitleSize: sectionTitleSize * scale,
            uppercasesSectionTitles: uppercasesSectionTitles,
            searchSize: searchSize * scale,
            searchUsesDisplayFont: searchUsesDisplayFont,
            historyContentSize: historyContentSize * scale
        )
    }
}

struct TailSyncThemePalette: Equatable {
    let accent: UInt32
    let accentHover: UInt32
    let accentSoft: UInt32
    let accentContrast: UInt32
    let window: UInt32
    let surface: UInt32
    let softSurface: UInt32
    let raised: UInt32
    let textPrimary: UInt32
    let textSecondary: UInt32
    let textTertiary: UInt32
    let border: UInt32
    let borderStrong: UInt32
    let divider: UInt32
    let positive: UInt32
    let positiveSoft: UInt32
    let warning: UInt32
    let warningSoft: UInt32
    let info: UInt32
    let infoSoft: UInt32
    let hover: UInt32
    let active: UInt32
    let toast: UInt32
    let toastText: UInt32
    let accentOpacity: Double
    let accentHoverOpacity: Double
    let accentSoftOpacity: Double
    let accentContrastOpacity: Double
    let windowOpacity: Double
    let surfaceOpacity: Double
    let softSurfaceOpacity: Double
    let raisedOpacity: Double
    let textPrimaryOpacity: Double
    let textSecondaryOpacity: Double
    let textTertiaryOpacity: Double
    let borderOpacity: Double
    let borderStrongOpacity: Double
    let dividerOpacity: Double
    let positiveOpacity: Double
    let positiveSoftOpacity: Double
    let warningOpacity: Double
    let warningSoftOpacity: Double
    let infoOpacity: Double
    let infoSoftOpacity: Double
    let hoverOpacity: Double
    let activeOpacity: Double
    let toastOpacity: Double
    let toastTextOpacity: Double

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
        accentOpacity: Double = 1,
        accentContrastOpacity: Double = 1,
        windowOpacity: Double = 1,
        surfaceOpacity: Double = 1,
        softSurfaceOpacity: Double = 1,
        raisedOpacity: Double = 1,
        textPrimaryOpacity: Double = 1,
        textSecondaryOpacity: Double = 1,
        textTertiaryOpacity: Double = 1,
        borderOpacity: Double = 1,
        dividerOpacity: Double = 1,
        positiveOpacity: Double = 1,
        warningOpacity: Double = 1,
        toastOpacity: Double = 1,
        toastTextOpacity: Double = 1,
        accentHover: UInt32? = nil,
        accentSoft: UInt32? = nil,
        borderStrong: UInt32? = nil,
        positiveSoft: UInt32? = nil,
        warningSoft: UInt32? = nil,
        info: UInt32? = nil,
        infoSoft: UInt32? = nil,
        hover: UInt32? = nil,
        active: UInt32? = nil,
        accentHoverOpacity: Double? = nil,
        accentSoftOpacity: Double? = nil,
        borderStrongOpacity: Double? = nil,
        positiveSoftOpacity: Double? = nil,
        warningSoftOpacity: Double? = nil,
        infoOpacity: Double? = nil,
        infoSoftOpacity: Double? = nil,
        hoverOpacity: Double? = nil,
        activeOpacity: Double? = nil
    ) {
        self.accent = accent
        self.accentHover = accentHover ?? accent
        self.accentSoft = accentSoft ?? accent
        self.accentContrast = accentContrast
        self.window = window
        self.surface = surface
        self.softSurface = softSurface
        self.raised = raised
        self.textPrimary = textPrimary
        self.textSecondary = textSecondary
        self.textTertiary = textTertiary
        self.border = border
        self.borderStrong = borderStrong ?? border
        self.divider = divider
        self.positive = positive
        self.positiveSoft = positiveSoft ?? positive
        self.warning = warning
        self.warningSoft = warningSoft ?? warning
        self.info = info ?? accent
        self.infoSoft = infoSoft ?? info ?? accent
        self.hover = hover ?? softSurface
        self.active = active ?? hover ?? softSurface
        self.toast = toast
        self.toastText = toastText
        self.accentOpacity = accentOpacity
        self.accentHoverOpacity = accentHoverOpacity ?? accentOpacity
        self.accentSoftOpacity = accentSoftOpacity ?? accentOpacity
        self.accentContrastOpacity = accentContrastOpacity
        self.windowOpacity = windowOpacity
        self.surfaceOpacity = surfaceOpacity
        self.softSurfaceOpacity = softSurfaceOpacity
        self.raisedOpacity = raisedOpacity
        self.textPrimaryOpacity = textPrimaryOpacity
        self.textSecondaryOpacity = textSecondaryOpacity
        self.textTertiaryOpacity = textTertiaryOpacity
        self.borderOpacity = borderOpacity
        self.borderStrongOpacity = borderStrongOpacity ?? borderOpacity
        self.dividerOpacity = dividerOpacity
        self.positiveOpacity = positiveOpacity
        self.positiveSoftOpacity = positiveSoftOpacity ?? positiveOpacity
        self.warningOpacity = warningOpacity
        self.warningSoftOpacity = warningSoftOpacity ?? warningOpacity
        self.infoOpacity = infoOpacity ?? accentOpacity
        self.infoSoftOpacity = infoSoftOpacity ?? infoOpacity ?? accentOpacity
        self.hoverOpacity = hoverOpacity ?? softSurfaceOpacity
        self.activeOpacity = activeOpacity ?? hoverOpacity ?? softSurfaceOpacity
        self.toastOpacity = toastOpacity
        self.toastTextOpacity = toastTextOpacity
    }

    var signature: String {
        [accent, accentHover, accentSoft, window, surface, softSurface, hover, active,
         textPrimary, textSecondary, textTertiary, border, borderStrong, divider,
         positive, positiveSoft, warning, warningSoft, info, infoSoft, toast, toastText]
            .map { String($0, radix: 16) }
            .joined(separator: ":") + ":\(accentOpacity):\(accentHoverOpacity):\(accentSoftOpacity):\(windowOpacity):\(surfaceOpacity):\(softSurfaceOpacity):\(hoverOpacity):\(activeOpacity)"
    }

    var accentColor: Color { Color(rgb: accent).opacity(accentOpacity) }
    var accentHoverColor: Color { Color(rgb: accentHover).opacity(accentHoverOpacity) }
    var accentSoftColor: Color { Color(rgb: accentSoft).opacity(accentSoftOpacity) }
    var accentContrastColor: Color { Color(rgb: accentContrast).opacity(accentContrastOpacity) }
    var windowColor: Color { Color(rgb: window).opacity(windowOpacity) }
    var surfaceColor: Color { Color(rgb: surface).opacity(surfaceOpacity) }
    var softSurfaceColor: Color { Color(rgb: softSurface).opacity(softSurfaceOpacity) }
    var inputColor: Color { softSurfaceColor }
    var hoverColor: Color { Color(rgb: hover).opacity(hoverOpacity) }
    var activeColor: Color { Color(rgb: active).opacity(activeOpacity) }
    var raisedColor: Color { Color(rgb: raised).opacity(raisedOpacity) }
    var primaryColor: Color { Color(rgb: textPrimary).opacity(textPrimaryOpacity) }
    var secondaryColor: Color { Color(rgb: textSecondary).opacity(textSecondaryOpacity) }
    var tertiaryColor: Color { Color(rgb: textTertiary).opacity(textTertiaryOpacity) }
    var borderColor: Color { Color(rgb: border).opacity(borderOpacity) }
    var borderStrongColor: Color { Color(rgb: borderStrong).opacity(borderStrongOpacity) }
    var dividerColor: Color { Color(rgb: divider).opacity(dividerOpacity) }
    var positiveColor: Color { Color(rgb: positive).opacity(positiveOpacity) }
    var positiveSoftColor: Color { Color(rgb: positiveSoft).opacity(positiveSoftOpacity) }
    var warningColor: Color { Color(rgb: warning).opacity(warningOpacity) }
    var warningSoftColor: Color { Color(rgb: warningSoft).opacity(warningSoftOpacity) }
    var infoColor: Color { Color(rgb: info).opacity(infoOpacity) }
    var infoSoftColor: Color { Color(rgb: infoSoft).opacity(infoSoftOpacity) }
    var toastColor: Color { Color(rgb: toast).opacity(toastOpacity) }
    var toastTextColor: Color { Color(rgb: toastText).opacity(toastTextOpacity) }

    var withReducedTransparency: TailSyncThemePalette {
        TailSyncThemePalette(
            accent: accent,
            accentContrast: accentContrast,
            window: window,
            surface: surface,
            softSurface: softSurface,
            raised: raised,
            textPrimary: textPrimary,
            textSecondary: textSecondary,
            textTertiary: textTertiary,
            border: border,
            divider: divider,
            positive: positive,
            warning: warning,
            toast: toast,
            toastText: toastText,
            accentHover: accentHover,
            accentSoft: accentSoft,
            borderStrong: borderStrong,
            positiveSoft: positiveSoft,
            warningSoft: warningSoft,
            info: info,
            infoSoft: infoSoft,
            hover: hover,
            active: active
        )
    }
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

enum TailSyncThemeAccessibilityPolicy {
    static func interfaceScale(for size: DynamicTypeSize) -> CGFloat {
        switch size {
        case .xSmall: return 0.82
        case .small: return 0.90
        case .medium: return 0.95
        case .large: return 1
        case .xLarge: return 1.12
        case .xxLarge: return 1.24
        case .xxxLarge: return 1.36
        case .accessibility1: return 1.5
        case .accessibility2: return 1.65
        case .accessibility3: return 1.8
        case .accessibility4: return 2
        case .accessibility5: return 2.2
        @unknown default: return 1
        }
    }
}

private struct TailSyncThemeModifier: ViewModifier {
    @ObservedObject private var loc = Loc.shared
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    func body(content: Content) -> some View {
        // Resolve the stored value against the custom-theme catalogue;
        // unknown custom ids fall back to the default theme at apply time.
        let selection = TailSyncThemeSelection(
            storedValue: loc.colorTheme,
            catalogue: loc.resolvedV2Themes,
            reduceTransparency: loc.reduceTransparency,
            interfaceScale: TailSyncThemeAccessibilityPolicy.interfaceScale(for: dynamicTypeSize)
        )
        let palette = selection.palette(for: colorScheme)
        return content
            .environment(\.tailSyncTheme, selection.builtin)
            .environment(\.tailSyncPalette, palette)
            .environment(\.tailSyncSelection, selection)
            .tint(palette.accentColor)
            .foregroundStyle(palette.primaryColor)
            .background(palette.windowColor.ignoresSafeArea())
            .task(id: selection.id) {
                await loc.loadResolvedV2Theme(loc.colorTheme)
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
