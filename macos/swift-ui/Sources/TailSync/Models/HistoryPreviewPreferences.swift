import Foundation

enum HistoryPreviewPreferences {
    static let textFontSizeKey = "HistoryPreview.textFontSize"
    static let defaultTextFontSize = 18.0
    static let minimumTextFontSize = 12.0
    static let maximumTextFontSize = 32.0

    static func clampedTextFontSize(_ value: Double) -> Double {
        guard value.isFinite else { return defaultTextFontSize }
        return min(maximumTextFontSize, max(minimumTextFontSize, value.rounded()))
    }

    static func textFontSize(afterModifierScroll delta: CGFloat, current: Double) -> Double {
        guard delta != 0 else { return clampedTextFontSize(current) }
        return clampedTextFontSize(current + (delta > 0 ? 1 : -1))
    }
}

struct HistoryPreviewDocumentReadingOptions: Equatable {
    static let fontSizeKey = "HistoryPreview.documentFontSize"
    static let lineSpacingKey = "HistoryPreview.documentLineSpacing"
    static let comfortableWidthKey = "HistoryPreview.documentComfortableWidth"
    static let defaultFontSize = 18.0
    static let maximumReadingWidth: CGFloat = 740

    enum LineSpacing: String, CaseIterable, Identifiable {
        case compact, standard, relaxed

        var id: String { rawValue }
        var labelKey: String { "history.preview.document.spacing.\(rawValue)" }
        var multiple: CGFloat {
            switch self {
            case .compact: return 1
            case .standard: return 1.25
            case .relaxed: return 1.5
            }
        }
    }

    var fontSize: Double = defaultFontSize
    var lineSpacing: LineSpacing = .standard
    var comfortableWidth = false

    var normalizedFontSize: CGFloat {
        CGFloat(HistoryPreviewPreferences.clampedTextFontSize(fontSize))
    }
}
