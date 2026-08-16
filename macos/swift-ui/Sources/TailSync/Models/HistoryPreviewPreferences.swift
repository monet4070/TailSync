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
}
