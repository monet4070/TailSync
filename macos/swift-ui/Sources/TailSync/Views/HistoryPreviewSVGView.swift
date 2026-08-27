import SwiftUI

/// Switches between the safe rasterized SVG and its original source. The mode
/// control lives in the shared preview header so SVG does not add a second,
/// mostly-empty toolbar above the normal image controls.
struct HistoryPreviewSVGView: View {
    enum Mode: String, CaseIterable {
        case visual
        case source
    }

    let source: String
    let material: HistoryPreviewImageMaterial
    @Binding var mode: Mode

    var body: some View {
        if mode == .visual {
            HistoryImagePreviewView(
                material: material,
                showsTransparencyInitially: false
            )
        } else {
            // The source remains plain text; it is never interpreted as HTML
            // or loaded by a web view.
            HistoryPreviewTextView(text: source, initiallyCode: true)
        }
    }
}
