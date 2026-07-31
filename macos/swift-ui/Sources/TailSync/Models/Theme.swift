import SwiftUI

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

    var metrics: TailSyncThemeMetrics {
        switch self {
        case .tailsync:
            return TailSyncThemeMetrics(cardRadius: 10, controlRadius: 9, rowPadding: 13, shadowRadius: 8)
        case .ocean:
            return TailSyncThemeMetrics(cardRadius: 5, controlRadius: 3, rowPadding: 11, shadowRadius: 7)
        case .forest:
            return TailSyncThemeMetrics(cardRadius: 4, controlRadius: 2, rowPadding: 12, shadowRadius: 5)
        case .rose:
            return TailSyncThemeMetrics(cardRadius: 12, controlRadius: 9, rowPadding: 11, shadowRadius: 10)
        case .highContrast:
            return TailSyncThemeMetrics(cardRadius: 0, controlRadius: 0, rowPadding: 12, shadowRadius: 0)
        }
    }

    var typography: TailSyncThemeTypography {
        switch self {
        case .tailsync:
            return TailSyncThemeTypography(
                sectionTitleSize: 25,
                uppercasesSectionTitles: false,
                searchSize: 18,
                searchUsesDisplayFont: true,
                historyContentSize: 15
            )
        case .forest:
            return TailSyncThemeTypography(
                sectionTitleSize: 12,
                uppercasesSectionTitles: true,
                searchSize: 13,
                searchUsesDisplayFont: false,
                historyContentSize: 14
            )
        default:
            return TailSyncThemeTypography(
                sectionTitleSize: 12,
                uppercasesSectionTitles: true,
                searchSize: 13,
                searchUsesDisplayFont: false,
                historyContentSize: 13
            )
        }
    }

    var displayFontName: String? {
        switch self {
        case .tailsync, .forest:
            return "Songti SC"
        case .ocean:
            return "Avenir Next"
        case .rose:
            return nil
        case .highContrast:
            return "Arial"
        }
    }

    var readingFontName: String? {
        switch self {
        case .ocean:
            return "Avenir Next"
        case .forest:
            return "Songti SC"
        case .highContrast:
            return "Arial"
        case .tailsync, .rose:
            return nil
        }
    }

    func displayFont(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        if let displayFontName {
            return .custom(displayFontName, size: size).weight(weight)
        }
        switch self {
        case .rose:
            return .system(size: size, weight: weight, design: .rounded)
        default:
            return .system(size: size, weight: weight)
        }
    }

    func readingFont(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        if let readingFontName {
            return .custom(readingFontName, size: size).weight(weight)
        }
        switch self {
        case .rose:
            return .system(size: size, weight: weight, design: .rounded)
        default:
            return .system(size: size, weight: weight)
        }
    }

    func palette(for scheme: ColorScheme) -> TailSyncThemePalette {
        switch (self, scheme) {
        case (.tailsync, .light):
            return .init(accent: 0xD5684B, accentContrast: 0xFFFFFF, window: 0xFAF9F5,
                         surface: 0xFFFEFA, softSurface: 0x1A1916, raised: 0xFFFEFA,
                         textPrimary: 0x191918, textSecondary: 0x68665F, textTertiary: 0x98958B,
                         border: 0xD3CEC2, divider: 0xECE8DF, positive: 0x44745A,
                         warning: 0xB96536, toast: 0x171716, toastText: 0xFFFFFF,
                         softSurfaceOpacity: 0.045)
        case (.tailsync, _):
            return .init(accent: 0xEC8668, accentContrast: 0x181412, window: 0x191918,
                         surface: 0x232321, softSurface: 0xFFFDF5, raised: 0x262522,
                         textPrimary: 0xF4F1E9, textSecondary: 0xAAA69C, textTertiary: 0x77746C,
                         border: 0x48463F, divider: 0x2C2B28, positive: 0x75AA86,
                         warning: 0xDC9163, toast: 0xF8F5ED, toastText: 0x171716,
                         softSurfaceOpacity: 0.055)
        case (.ocean, .light):
            return .init(accent: 0x087F8C, accentContrast: 0xFFFFFF, window: 0xF1F7F7,
                         surface: 0xFCFEFE, softSurface: 0x085B65, raised: 0xFFFFFF,
                         textPrimary: 0x051F21, textSecondary: 0x051F21, textTertiary: 0x051F21,
                         border: 0x08474E, divider: 0x08474E, positive: 0x34C759,
                         warning: 0xFF9500, toast: 0x000000, toastText: 0xFFFFFF,
                         softSurfaceOpacity: 0.06, textPrimaryOpacity: 0.90,
                         textSecondaryOpacity: 0.60, textTertiaryOpacity: 0.40,
                         borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.88)
        case (.ocean, _):
            return .init(accent: 0x4CC9C0, accentContrast: 0x071918, window: 0x111E20,
                         surface: 0x1A2A2D, softSurface: 0xD2FFFC, raised: 0x203235,
                         textPrimary: 0xF2FFFE, textSecondary: 0xE0F8F6, textTertiary: 0xE0F8F6,
                         border: 0xC0EFEB, divider: 0xC0EFEB, positive: 0x30D158,
                         warning: 0xFF9F0A, toast: 0xFFFFFF, toastText: 0x000000,
                         softSurfaceOpacity: 0.07, textPrimaryOpacity: 0.94,
                         textSecondaryOpacity: 0.62, textTertiaryOpacity: 0.40,
                         borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.92)
        case (.forest, .light):
            return .init(accent: 0x287A55, accentContrast: 0xFFFFFF, window: 0xF3F7F3,
                         surface: 0xFCFDFB, softSurface: 0x275C3E, raised: 0xFFFFFF,
                         textPrimary: 0x13261B, textSecondary: 0x13261B, textTertiary: 0x13261B,
                         border: 0x235235, divider: 0x235235, positive: 0x34C759,
                         warning: 0xFF9500, toast: 0x000000, toastText: 0xFFFFFF,
                         softSurfaceOpacity: 0.06, textPrimaryOpacity: 0.90,
                         textSecondaryOpacity: 0.60, textTertiaryOpacity: 0.40,
                         borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.88)
        case (.forest, _):
            return .init(accent: 0x69CA91, accentContrast: 0x0B1A11, window: 0x151F18,
                         surface: 0x202D24, softSurface: 0xE1FFEA, raised: 0x26362A,
                         textPrimary: 0xF6FFF8, textSecondary: 0xE5F8EA, textTertiary: 0xE5F8EA,
                         border: 0xCFEFD8, divider: 0xCFEFD8, positive: 0x30D158,
                         warning: 0xFF9F0A, toast: 0xFFFFFF, toastText: 0x000000,
                         softSurfaceOpacity: 0.07, textPrimaryOpacity: 0.94,
                         textSecondaryOpacity: 0.62, textTertiaryOpacity: 0.40,
                         borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.92)
        case (.rose, .light):
            return .init(accent: 0xB83E60, accentContrast: 0xFFFFFF, window: 0xFAF4F6,
                         surface: 0xFFFDFD, softSurface: 0x802A42, raised: 0xFFFFFF,
                         textPrimary: 0x30131B, textSecondary: 0x30131B, textTertiary: 0x30131B,
                         border: 0x71263B, divider: 0x71263B, positive: 0x34C759,
                         warning: 0xFF9500, toast: 0x000000, toastText: 0xFFFFFF,
                         softSurfaceOpacity: 0.055, textPrimaryOpacity: 0.90,
                         textSecondaryOpacity: 0.60, textTertiaryOpacity: 0.40,
                         borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.88)
        case (.rose, _):
            return .init(accent: 0xEF86A2, accentContrast: 0x251016, window: 0x24191D,
                         surface: 0x322329, softSurface: 0xFFE2EA, raised: 0x3A2930,
                         textPrimary: 0xFFF7F9, textSecondary: 0xFAE5EB, textTertiary: 0xFAE5EB,
                         border: 0xF5D3DC, divider: 0xF5D3DC, positive: 0x30D158,
                         warning: 0xFF9F0A, toast: 0xFFFFFF, toastText: 0x000000,
                         softSurfaceOpacity: 0.07, textPrimaryOpacity: 0.94,
                         textSecondaryOpacity: 0.62, textTertiaryOpacity: 0.40,
                         borderOpacity: 0.18, dividerOpacity: 0.07, toastOpacity: 0.92)
        case (.highContrast, .light):
            return .init(accent: 0x005FCC, accentContrast: 0xFFFFFF, window: 0xFFFFFF,
                         surface: 0xFFFFFF, softSurface: 0xF0F0F0, raised: 0xFFFFFF,
                         textPrimary: 0x000000, textSecondary: 0x333333, textTertiary: 0x5A5A5A,
                         border: 0x8A8A8A, divider: 0xB0B0B0, positive: 0x087A32,
                         warning: 0x9A4D00, toast: 0x000000, toastText: 0xFFFFFF)
        case (.highContrast, _):
            return .init(accent: 0xFFD400, accentContrast: 0x000000, window: 0x000000,
                         surface: 0x101010, softSurface: 0x1C1C1C, raised: 0x181818,
                         textPrimary: 0xFFFFFF, textSecondary: 0xE3E3E3, textTertiary: 0xB8B8B8,
                         border: 0x777777, divider: 0x595959, positive: 0x65FF8F,
                         warning: 0xFFD166, toast: 0xFFFFFF, toastText: 0x000000)
        }
    }
}

struct TailSyncThemeMetrics: Equatable {
    let cardRadius: CGFloat
    let controlRadius: CGFloat
    let rowPadding: CGFloat
    let shadowRadius: CGFloat
}

struct TailSyncThemeTypography: Equatable {
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

extension EnvironmentValues {
    var tailSyncTheme: TailSyncColorTheme {
        get { self[TailSyncThemeKey.self] }
        set { self[TailSyncThemeKey.self] = newValue }
    }

    var tailSyncPalette: TailSyncThemePalette {
        get { self[TailSyncPaletteKey.self] }
        set { self[TailSyncPaletteKey.self] = newValue }
    }
}

private struct TailSyncThemeModifier: ViewModifier {
    @ObservedObject private var loc = Loc.shared
    @Environment(\.colorScheme) private var colorScheme

    func body(content: Content) -> some View {
        let theme = TailSyncColorTheme(storedValue: loc.colorTheme)
        let palette = theme.palette(for: colorScheme)
        content
            .environment(\.tailSyncTheme, theme)
            .environment(\.tailSyncPalette, palette)
            .tint(palette.accentColor)
            .foregroundStyle(palette.primaryColor)
            .background(palette.windowColor.ignoresSafeArea())
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
