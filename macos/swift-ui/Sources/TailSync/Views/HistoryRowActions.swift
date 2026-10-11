import SwiftUI

enum HistoryRowActionPolicy {
    static func explicitRestoreFormats(_ info: ContractCompositeInfo) -> [String] {
        guard !info.native_only else { return [] }
        let allowed: Set<String> = ["text", "png"]
        let withheld = Set(info.withheld_formats.map { $0.format })
        let formats = info.restorable_formats.filter { allowed.contains($0) && !withheld.contains($0) }
        guard hasRichContent(info) else { return formats.filter { $0 != "text" } }
        return formats
    }

    static func hasRichContent(_ info: ContractCompositeInfo) -> Bool {
        info.image_count > 0 || info.formats.contains { $0 != "text" }
    }

    static func showsMaintenanceActions(_ info: ContractCompositeInfo) -> Bool {
        !info.native_only && info.formats.contains("html") && info.completeness != "complete"
    }

    static func showsAdvancedActions(
        _ info: ContractCompositeInfo,
        hasAuthorizeHandler: Bool,
        hasImportHandler: Bool,
        hasRevokeHandler: Bool
    ) -> Bool {
        if hasRevokeHandler { return true }
        guard showsMaintenanceActions(info) else { return false }
        return hasAuthorizeHandler || hasImportHandler
    }

    static func showsDiagnostics(_ info: ContractCompositeInfo?) -> Bool {
        info != nil
    }

    static func showsActionsMenu(
        _ info: ContractCompositeInfo,
        canRestoreFormat: Bool,
        hasArchiveHandler: Bool,
        hasAdvancedHandlers: Bool
    ) -> Bool {
        if canRestoreFormat, !explicitRestoreFormats(info).isEmpty { return true }
        if hasAdvancedHandlers { return true }
        return hasArchiveHandler && showsMaintenanceActions(info)
    }
}

enum HistoryRowStatusLabel {
    static func text(for info: ContractCompositeInfo?) -> String? {
        guard let info else { return nil }
        if info.native_only { return Loc.t("history.composite.localOnly") }
        if info.completeness == "external_references" { return Loc.t("history.composite.resourcesMissing") }
        if info.completeness != "complete" { return Loc.t("history.composite.incomplete") }
        return nil
    }
}

enum HistoryRowDiagnostics {
    struct Row: Identifiable {
        let label: String
        let value: String
        var id: String { label }
    }

    static func rows(for info: ContractCompositeInfo) -> [Row] {
        let listSeparator = ", "
        var rows: [Row] = [
            Row(label: Loc.t("history.composite.diag.formats"), value: joined(info.formats, separator: listSeparator)),
            Row(label: Loc.t("history.composite.diag.representations"), value: "\(info.representation_count)"),
            Row(label: Loc.t("history.composite.diag.images"), value: "\(info.image_count)"),
            Row(label: Loc.t("history.composite.diag.nativeItems"), value: "\(info.native_item_count)"),
            Row(
                label: Loc.t("history.composite.diag.nativeOnly"),
                value: info.native_only ? Loc.t("history.composite.diag.yes") : Loc.t("history.composite.diag.no")
            ),
            Row(label: Loc.t("history.composite.diag.completeness"), value: completenessText(info.completeness)),
            Row(label: Loc.t("history.composite.diag.restorable"), value: joined(info.restorable_formats, separator: listSeparator)),
        ]
        if !info.withheld_formats.isEmpty {
            let withheld = info.withheld_formats
                .map { "\($0.format): \(withheldReason($0.code))" }
                .joined(separator: listSeparator)
            rows.append(Row(label: Loc.t("history.composite.diag.withheld"), value: withheld))
        }
        return rows
    }

    static func completenessText(_ completeness: String) -> String {
        let key = "history.composite.completeness.\(completeness)"
        let text = Loc.t(key)
        return text == key ? Loc.t("history.composite.diag.unknown") : text
    }

    static func withheldReason(_ code: String) -> String {
        let key = "history.restore.code.\(code)"
        let text = Loc.t(key)
        return text == key ? Loc.t("history.composite.diag.unavailable") : text
    }

    private static func joined(_ values: [String], separator: String) -> String {
        values.isEmpty ? Loc.t("history.composite.diag.none") : values.joined(separator: separator)
    }
}

struct HistoryRowTrailingActions: View {
    let info: ContractCompositeInfo
    let canRestoreFormat: Bool
    let onRestoreFormat: ((String) -> Void)?
    let onArchiveImages: (() -> Void)?
    let onAuthorizeSource: (() -> Void)?
    let onImportSourcePacket: (() -> Void)?
    let onRevokeSource: (() -> Void)?

    @State private var showsDiagnostics = false

    private var restoreFormats: [String] {
        canRestoreFormat ? HistoryRowActionPolicy.explicitRestoreFormats(info) : []
    }

    private var showsArchive: Bool {
        onArchiveImages != nil && HistoryRowActionPolicy.showsMaintenanceActions(info)
    }

    private var showsAdvanced: Bool {
        HistoryRowActionPolicy.showsAdvancedActions(
            info,
            hasAuthorizeHandler: onAuthorizeSource != nil,
            hasImportHandler: onImportSourcePacket != nil,
            hasRevokeHandler: onRevokeSource != nil
        )
    }

    private var showsActionsMenu: Bool {
        HistoryRowActionPolicy.showsActionsMenu(
            info,
            canRestoreFormat: canRestoreFormat,
            hasArchiveHandler: onArchiveImages != nil,
            hasAdvancedHandlers: showsAdvanced
        )
    }

    var body: some View {
        HStack(spacing: 2) {
            if HistoryRowActionPolicy.showsDiagnostics(info) {
                Button {
                    showsDiagnostics = true
                } label: {
                    Label(Loc.t("history.composite.details"), systemImage: "info.circle")
                        .labelStyle(.iconOnly)
                }
                .buttonStyle(.borderless)
                .fixedSize()
                .padding(2)
                .accessibilityLabel(Text(Loc.t("history.composite.details")))
                .popover(isPresented: $showsDiagnostics, arrowEdge: .bottom) {
                    HistoryRowDiagnosticsView(info: info)
                }
            }
            if showsActionsMenu {
                Menu {
                    ForEach(restoreFormats, id: \.self) { format in
                        Button(Loc.t("history.composite.copy.\(format)")) { onRestoreFormat?(format) }
                    }
                    if !restoreFormats.isEmpty, showsArchive || showsAdvanced {
                        Divider()
                    }
                    if showsArchive, let onArchiveImages {
                        Button(Loc.t("history.composite.archiveImages")) { onArchiveImages() }
                            .help(Loc.t("history.composite.archiveImagesHint"))
                            .accessibilityHint(Text(Loc.t("history.composite.archiveImagesHint")))
                    }
                    if showsAdvanced {
                        Menu(Loc.t("history.composite.advanced")) {
                            if HistoryRowActionPolicy.showsMaintenanceActions(info), let onAuthorizeSource {
                                Button(Loc.t("history.composite.authorizeSource")) { onAuthorizeSource() }
                            }
                            if HistoryRowActionPolicy.showsMaintenanceActions(info), let onImportSourcePacket {
                                Button(Loc.t("history.composite.importSourcePacket")) { onImportSourcePacket() }
                            }
                            if let onRevokeSource {
                                Button(Loc.t("history.composite.revokeSource")) { onRevokeSource() }
                            }
                        }
                    }
                } label: {
                    Label(Loc.t("history.composite.moreActions"), systemImage: "ellipsis.circle")
                        .labelStyle(.iconOnly)
                }
                .accessibilityLabel(Text(Loc.t("history.composite.moreActions")))
                .menuStyle(.borderlessButton).fixedSize().padding(2)
            }
        }
    }
}

struct HistoryRowDiagnosticsView: View {
    let info: ContractCompositeInfo

    @Environment(\.tailSyncPalette) private var palette

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(Loc.t("history.composite.details"))
                .font(.headline)
            ForEach(HistoryRowDiagnostics.rows(for: info)) { row in
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Text(row.label)
                        .font(.caption)
                        .foregroundColor(palette.secondaryColor)
                    Spacer(minLength: 12)
                    Text(row.value)
                        .font(.caption)
                        .foregroundColor(palette.primaryColor)
                        .multilineTextAlignment(.trailing)
                }
            }
        }
        .padding(12)
        .frame(minWidth: 240, alignment: .leading)
        .accessibilityElement(children: .contain)
    }
}
