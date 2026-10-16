import SwiftUI

struct PreviewThemePalette {
    let accentColor: Color
    let accentSoftColor: Color
    let surfaceColor: Color
    let windowBackgroundColor: Color
    let primaryColor: Color
    let secondaryColor: Color
    let tertiaryColor: Color
    let borderColor: Color
    let dividerColor: Color
    let greenColor: Color

    static func light() -> PreviewThemePalette {
        PreviewThemePalette(
            accentColor: Color(red: 0.369, green: 0.416, blue: 0.824),
            accentSoftColor: Color(red: 0.369, green: 0.416, blue: 0.824).opacity(0.12),
            surfaceColor: Color(white: 1.0),
            windowBackgroundColor: Color(red: 0.96, green: 0.96, blue: 0.97),
            primaryColor: Color(white: 0.12),
            secondaryColor: Color(white: 0.4),
            tertiaryColor: Color(white: 0.6),
            borderColor: Color(white: 0.88),
            dividerColor: Color(white: 0.93),
            greenColor: Color(red: 0.2, green: 0.78, blue: 0.35)
        )
    }

    static func dark() -> PreviewThemePalette {
        PreviewThemePalette(
            accentColor: Color(red: 0.482, green: 0.514, blue: 0.922),
            accentSoftColor: Color(red: 0.482, green: 0.514, blue: 0.922).opacity(0.18),
            surfaceColor: Color(red: 0.17, green: 0.17, blue: 0.18),
            windowBackgroundColor: Color(red: 0.11, green: 0.11, blue: 0.12),
            primaryColor: Color(white: 0.92),
            secondaryColor: Color(white: 0.6),
            tertiaryColor: Color(white: 0.4),
            borderColor: Color(white: 0.22),
            dividerColor: Color(white: 0.2),
            greenColor: Color(red: 0.19, green: 0.82, blue: 0.35)
        )
    }
}
