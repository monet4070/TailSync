import AppKit
import SwiftUI

struct HistoryPreviewView: View {
    @ObservedObject var model: HistoryPreviewViewModel

    @ObservedObject private var loc = Loc.shared
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    private var theme: TailSyncThemeSelection {
        TailSyncThemeSelection(
            storedValue: loc.colorTheme,
            catalogue: loc.resolvedV2Themes,
            reduceTransparency: loc.reduceTransparency,
            interfaceScale: TailSyncThemeAccessibilityPolicy.interfaceScale(for: dynamicTypeSize)
        )
    }

    private var palette: TailSyncThemePalette {
        theme.palette(for: colorScheme)
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            content
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .tailSyncThemed()
    }

    private var header: some View {
        HStack(spacing: 12) {
            if model.batchPositionText != nil {
                HStack(spacing: 6) {
                    headerIconButton(
                        "chevron.left",
                        help: Loc.t("history.preview.previousItem"),
                        action: model.navigateBackward
                    )
                    .disabled(!model.canNavigateBackward)
                    headerIconButton(
                        "chevron.right",
                        help: Loc.t("history.preview.nextItem"),
                        action: model.navigateForward
                    )
                    .disabled(!model.canNavigateForward)
                }
            }

            Image(systemName: "doc.text")
                .font(.system(size: 15, weight: .semibold))
                .foregroundColor(palette.accentColor)
                .frame(
                    width: HistoryPreviewLayoutMetrics.regularControlSize,
                    height: HistoryPreviewLayoutMetrics.regularControlSize
                )
                .background(palette.accentColor.opacity(0.12))
                .clipShape(RoundedRectangle(cornerRadius: 9, style: .continuous))

            VStack(alignment: .leading, spacing: 2) {
                Text(model.currentName)
                    .font(theme.displayFont(size: 15, weight: .semibold))
                    .foregroundColor(palette.primaryColor)
                    .lineLimit(1)
                    .truncationMode(.middle)
                HStack(spacing: 8) {
                    if let batch = model.batchPositionText {
                        Text(batch).monospacedDigit()
                    }
                    if model.currentSize > 0 {
                        Text(ByteCountFormatter.string(
                            fromByteCount: model.currentSize,
                            countStyle: .file
                        ))
                    }
                }
                .font(.caption2)
                .foregroundColor(palette.tertiaryColor)
            }
            Spacer(minLength: 12)

            restoreFeedback
            Button(action: model.restoreCurrent) {
                Label(Loc.t("history.preview.restore"), systemImage: "doc.on.clipboard")
                    .font(.system(size: 12, weight: .semibold))
                    .padding(.horizontal, 2)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.regular)
            .disabled(model.restoreState == .restoring)
            .help(Loc.t("history.preview.restore"))
        }
        .padding(.horizontal, 16)
        .frame(height: HistoryPreviewLayoutMetrics.headerHeight)
        .background(palette.surfaceColor)
        .overlay(alignment: .bottom) {
            Rectangle().fill(palette.dividerColor).frame(height: 1)
        }
    }

    private func headerIconButton(
        _ systemName: String,
        help: String,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Image(systemName: systemName)
                .font(.system(size: 13, weight: .semibold))
                .frame(
                    width: HistoryPreviewLayoutMetrics.regularControlSize,
                    height: HistoryPreviewLayoutMetrics.regularControlSize
                )
        }
        .buttonStyle(.bordered)
        .controlSize(.regular)
        .help(help)
    }

    @ViewBuilder
    private var restoreFeedback: some View {
        switch model.restoreState {
        case .idle:
            EmptyView()
        case .restoring:
            ProgressView().controlSize(.small)
        case .restored:
            Label(Loc.t("history.preview.restored"), systemImage: "checkmark.circle.fill")
                .font(.caption.weight(.semibold))
                .foregroundColor(palette.positiveColor)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(palette.positiveSoftColor)
                .clipShape(RoundedRectangle(cornerRadius: 4))
        case .failed:
            Text(Loc.t("history.preview.restoreFailed"))
                .font(.caption)
                .foregroundColor(palette.warningColor)
        }
    }

    @ViewBuilder
    private var content: some View {
        switch model.state {
        case .idle, .loading:
            stateView(
                icon: nil,
                title: Loc.t("history.preview.loading"),
                message: nil,
                showsProgress: true,
                retry: false
            )
        case .failed(let failure):
            failureView(failure)
        case .ready(let payload, let material, let format):
            readyView(payload: payload, material: material, format: format)
                .id(model.currentEntryId)
        }
    }

    @ViewBuilder
    private func readyView(
        payload: HistoryPreviewData,
        material: HistoryPreviewMaterial,
        format: HistoryPreviewFormat
    ) -> some View {
        switch material {
        case .text(let text):
            if format == .markdown {
                HistoryMarkdownPreviewView(source: text)
            } else {
                HistoryPreviewTextView(text: text, initiallyCode: format == .code)
            }
        case .image(let image):
            HistoryImagePreviewView(material: image)
        case .pdf(let pdf):
            HistoryPDFPreviewView(material: pdf)
        case .quickLook(let url):
            HistoryQuickLookPreviewView(url: url)
        case .unsupported:
            stateView(
                icon: "doc.questionmark",
                title: Loc.t("history.preview.unsupportedTitle"),
                message: Loc.t("history.preview.unsupportedMessage")
                    .replacingOccurrences(of: "{type}", with: displayType(payload.name)),
                showsProgress: false,
                retry: false
            )
        }
    }

    @ViewBuilder
    private func failureView(_ failure: HistoryPreviewFailure) -> some View {
        let keys: (String, String, String) = switch failure.kind {
        case .tooLarge:
            ("externaldrive.badge.exclamationmark", "history.preview.tooLargeTitle", "history.preview.tooLargeMessage")
        case .unsupported:
            ("doc.questionmark", "history.preview.unsupportedTitle", "history.preview.unsupportedMessage")
        case .corrupt:
            ("doc.badge.ellipsis", "history.preview.corruptTitle", "history.preview.corruptMessage")
        case .decryption:
            ("lock.trianglebadge.exclamationmark", "history.preview.decryptTitle", "history.preview.decryptMessage")
        case .unavailable:
            ("exclamationmark.triangle", "history.preview.unavailableTitle", "history.preview.unavailableMessage")
        }
        let localizedMessage = Loc.t(keys.2)
        let message = failure.kind == .unsupported
            ? localizedMessage.replacingOccurrences(
                of: "{type}",
                with: displayType(model.currentName)
            )
            : localizedMessage
        stateView(
            icon: keys.0,
            title: Loc.t(keys.1),
            message: message,
            showsProgress: false,
            retry: failure.canRetry
        )
    }

    private func stateView(
        icon: String?,
        title: String,
        message: String?,
        showsProgress: Bool,
        retry: Bool
    ) -> some View {
        VStack(spacing: 12) {
            if showsProgress {
                ProgressView().controlSize(.regular)
            } else if let image = loc.themeAssetImages["previewPlaceholder"] {
                Image(nsImage: image).resizable().aspectRatio(contentMode: .fit).frame(width: 72, height: 72)
            } else if let icon {
                Image(systemName: icon)
                    .font(.system(size: 34))
                    .foregroundColor(palette.tertiaryColor)
            }
            Text(title)
                .font(theme.displayFont(size: 16, weight: .semibold))
                .foregroundColor(palette.primaryColor)
                .multilineTextAlignment(.center)
            if let message {
                Text(message)
                    .font(theme.readingFont(size: 12))
                    .foregroundColor(palette.secondaryColor)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 460)
            }
            HStack(spacing: 8) {
                if retry {
                    Button(Loc.t("history.preview.retry"), action: model.retry)
                        .buttonStyle(.bordered)
                }
                Button(Loc.t("history.preview.restore"), action: model.restoreCurrent)
                    .buttonStyle(.borderedProminent)
                    .disabled(model.restoreState == .restoring)
            }
        }
        .padding(30)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(palette.surfaceColor)
    }

    private func displayType(_ name: String) -> String {
        let fileExtension = HistoryPreviewFileTypes.fileExtension(for: name)
        return fileExtension.isEmpty ? Loc.t("history.preview.unknownType") : fileExtension.uppercased()
    }
}
