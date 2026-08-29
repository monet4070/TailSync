import SwiftUI
import AppKit
import UserNotifications
import UniformTypeIdentifiers

func routeInterfaceLabel(_ interface: String) -> String {
    switch interface {
    case "lan": return "LAN"
    case "iroh": return "Iroh"
    default: return "Tailscale"
    }
}

struct SettingsPollingPlan {
    let refreshPairingStatus: Bool
    let refreshPeers: Bool
}

enum SettingsPollingPolicy {
    static func next(
        applicationIsActive: Bool,
        peerRefreshTicks: inout Int
    ) -> SettingsPollingPlan {
        var refreshPeers = false
        if applicationIsActive {
            peerRefreshTicks += 1
            if peerRefreshTicks >= 5 {
                peerRefreshTicks = 0
                refreshPeers = true
            }
        }
        return SettingsPollingPlan(
            refreshPairingStatus: true,
            refreshPeers: refreshPeers
        )
    }
}

actor SettingsSaveCoordinator {
    private enum SaveResult: Sendable {
        case success(AppSettings)
        case failure(String, AppSettings)
    }

    private var tail: Task<SaveResult, Never>?

    func save(
        _ settings: AppSettings,
        fallback: AppSettings
    ) async -> (error: String?, persisted: AppSettings) {
        let predecessor = tail
        let job = Task<SaveResult, Never> {
            let lastPersisted: AppSettings
            if let result = await predecessor?.value {
                switch result {
                case .success(let settings): lastPersisted = settings
                case .failure(_, let settings): lastPersisted = settings
                }
            } else {
                lastPersisted = fallback
            }
            do {
                try await ApiClient.shared.updateSettings(settings)
                return .success(settings)
            } catch {
                return .failure(error.localizedDescription, lastPersisted)
            }
        }
        tail = job
        switch await job.value {
        case .success(let settings): return (nil, settings)
        case .failure(let message, let settings): return (message, settings)
        }
    }
}

enum ThemeV2CardLayout {
    static let previewHeight: CGFloat = 68
    static let minimumCardHeight: CGFloat = 108
    static let minimumActionHitSize: CGFloat = 28
}

enum ThemeV2CardPreviewPolicy {
    static func selection(
        themeId: String,
        catalogue: [TailSyncThemeDefinition],
        reduceTransparency: Bool,
        interfaceScale: CGFloat
    ) -> TailSyncThemeSelection {
        TailSyncThemeSelection(
            storedValue: themeId,
            catalogue: catalogue,
            reduceTransparency: reduceTransparency,
            interfaceScale: interfaceScale
        )
    }
}

struct ThemeV2CardView: View {
    let descriptor: ApiClient.ThemeV2Descriptor
    let name: String
    let selected: Bool
    let selection: TailSyncThemeSelection
    let colorScheme: ColorScheme
    let onSelect: () -> Void
    let onUpdate: () -> Void
    let onRollback: () -> Void
    let onDelete: () -> Void

    @State private var hovering = false

    private var palette: TailSyncThemePalette {
        selection.palette(for: colorScheme)
    }

    private var metrics: TailSyncThemeMetrics {
        selection.metrics(for: colorScheme)
    }

    private var typography: TailSyncThemeTypography {
        selection.typography(for: colorScheme)
    }

    private var cardRadius: CGFloat {
        min(14, max(3, metrics.cardRadius))
    }

    private var sourceLabel: String {
        if descriptor.status != "valid" {
            return descriptor.diagnostics.map(\.message).joined(separator: " ")
        }
        return descriptor.source == "builtin"
            ? Loc.t("settings.themePackageBuiltIn")
            : descriptor.version
    }

    private var cardBackground: Color {
        if selected { return palette.activeColor }
        if hovering { return palette.hoverColor }
        return palette.surfaceColor
    }

    private var previewTitle: String {
        typography.uppercasesSectionTitles ? "TAILSYNC" : "TailSync"
    }

    var body: some View {
        ZStack(alignment: .topTrailing) {
            Button(action: onSelect) {
                VStack(alignment: .leading, spacing: 0) {
                    previewBand
                        .frame(height: ThemeV2CardLayout.previewHeight)
                    HStack(spacing: 8) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(name)
                                .font(selection.displayFont(size: 11, weight: .semibold))
                                .foregroundColor(palette.primaryColor)
                                .lineLimit(1)
                                .minimumScaleFactor(0.75)
                            Text(sourceLabel)
                                .font(selection.readingFont(size: 9))
                                .foregroundColor(palette.tertiaryColor)
                                .lineLimit(1)
                                .truncationMode(.tail)
                        }
                        Spacer(minLength: 4)
                        if selected {
                            Image(systemName: "checkmark")
                                .font(.system(size: 9, weight: .bold))
                                .foregroundColor(palette.accentContrastColor)
                                .frame(width: 20, height: 20)
                                .background(palette.accentColor)
                                .clipShape(RoundedRectangle(cornerRadius: min(7, cardRadius), style: .continuous))
                        }
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 7)
                }
                .frame(maxWidth: .infinity, minHeight: ThemeV2CardLayout.minimumCardHeight, alignment: .topLeading)
                .background(cardBackground)
                .contentShape(Rectangle())
                .clipShape(RoundedRectangle(cornerRadius: cardRadius, style: .continuous))
                .overlay {
                    RoundedRectangle(cornerRadius: cardRadius, style: .continuous)
                        .stroke(
                            selected || hovering ? palette.accentColor : palette.borderColor,
                            lineWidth: selected ? 2 : 1
                        )
                }
            }
            .buttonStyle(.plain)
            .frame(maxWidth: .infinity)
            .disabled(descriptor.status != "valid")
            .accessibilityLabel("\(name), \(sourceLabel)")

            actionButtons
                .padding(6)
        }
        .frame(maxWidth: .infinity, minHeight: ThemeV2CardLayout.minimumCardHeight)
        .opacity(descriptor.status == "valid" ? 1 : 0.7)
        .onHover { hovering = $0 }
    }

    private var previewBand: some View {
        ZStack(alignment: .leading) {
            palette.windowColor
            Rectangle()
                .fill(palette.accentColor)
                .frame(width: 4)
            VStack(alignment: .leading, spacing: 7) {
                HStack(spacing: 6) {
                    Text(previewTitle)
                        .font(selection.displayFont(size: 10, weight: .semibold))
                        .foregroundColor(palette.primaryColor)
                        .lineLimit(1)
                        .minimumScaleFactor(0.7)
                    Spacer(minLength: 4)
                    previewSwatch(palette.accentColor)
                    previewSwatch(palette.secondaryColor)
                    previewSwatch(palette.borderStrongColor)
                }
                RoundedRectangle(cornerRadius: 1)
                    .fill(palette.secondaryColor.opacity(0.55))
                    .frame(maxWidth: .infinity, minHeight: 2, maxHeight: 2)
                HStack(spacing: 6) {
                    RoundedRectangle(cornerRadius: min(5, max(1, metrics.controlRadius)))
                        .fill(palette.inputColor)
                        .overlay(alignment: .leading) {
                            RoundedRectangle(cornerRadius: 1)
                                .fill(palette.tertiaryColor.opacity(0.55))
                                .frame(width: 44, height: 2)
                                .padding(.leading, 7)
                        }
                    RoundedRectangle(cornerRadius: min(5, max(1, metrics.controlRadius)))
                        .fill(palette.accentSoftColor)
                        .frame(width: 34)
                        .overlay {
                            RoundedRectangle(cornerRadius: 1)
                                .fill(palette.accentColor)
                                .frame(width: 13, height: 2)
                        }
                }
                .frame(height: 16)
            }
            .padding(.leading, 12)
            .padding(.trailing, descriptor.source == "builtin" ? 10 : 72)
            .padding(.vertical, 9)
        }
        .clipped()
    }

    private func previewSwatch(_ color: Color) -> some View {
        RoundedRectangle(cornerRadius: 2, style: .continuous)
            .fill(color)
            .frame(width: 9, height: 9)
    }

    @ViewBuilder
    private var actionButtons: some View {
        if descriptor.source != "builtin" {
            HStack(spacing: 4) {
                if descriptor.status == "valid" {
                    actionButton(
                        systemName: "arrow.triangle.2.circlepath",
                        label: Loc.t("settings.themePackageUpdate"),
                        action: onUpdate
                    )
                    actionButton(
                        systemName: "arrow.uturn.backward",
                        label: Loc.t("settings.themePackageRollback"),
                        action: onRollback
                    )
                }
                actionButton(
                    systemName: "trash",
                    label: Loc.t("settings.themePackageDelete"),
                    action: onDelete
                )
            }
        }
    }

    private func actionButton(
        systemName: String,
        label: String,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Image(systemName: systemName)
                .font(.system(size: 11, weight: .medium))
                .foregroundColor(palette.primaryColor)
                .frame(
                    width: ThemeV2CardLayout.minimumActionHitSize,
                    height: ThemeV2CardLayout.minimumActionHitSize
                )
                .background(palette.raisedColor.opacity(0.96))
                .clipShape(RoundedRectangle(cornerRadius: min(7, cardRadius), style: .continuous))
                .overlay {
                    RoundedRectangle(cornerRadius: min(7, cardRadius), style: .continuous)
                        .stroke(palette.borderColor, lineWidth: 1)
                }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
        .help(label)
    }
}

enum ThemePackageVersionRelation: Equatable {
    case upgrade
    case same
    case downgrade
}

struct ThemePackageUpdateOptions: Equatable {
    let allowSameVersion: Bool
    let allowDowngrade: Bool

    static func forRelation(_ relation: ThemePackageVersionRelation?) -> Self {
        Self(
            allowSameVersion: relation == .same,
            allowDowngrade: relation == .downgrade
        )
    }
}

enum AppUpdatePhase: Equatable {
    case ready
    case checking
    case available
    case current
    case installing
    case installed
    case failed
}

struct ThemePackageSemanticVersion: Comparable {
    let core: [UInt32]
    let prerelease: [String]?

    init?(_ value: String) {
        let buildParts = value.split(separator: "+", maxSplits: 1, omittingEmptySubsequences: false)
        guard buildParts.count <= 2,
              buildParts.count == 1 || Self.validIdentifiers(String(buildParts[1]), numericLeadingZeroesAllowed: true) else { return nil }
        let versionParts = buildParts[0].split(separator: "-", maxSplits: 1, omittingEmptySubsequences: false)
        let numbers = versionParts[0].split(separator: ".", omittingEmptySubsequences: false)
        guard numbers.count == 3 else { return nil }
        var parsed: [UInt32] = []
        for number in numbers {
            let text = String(number)
            guard !text.isEmpty, text == "0" || !text.hasPrefix("0"), let value = UInt32(text) else { return nil }
            parsed.append(value)
        }
        if versionParts.count == 2 {
            let value = String(versionParts[1])
            guard Self.validIdentifiers(value, numericLeadingZeroesAllowed: false) else { return nil }
            prerelease = value.split(separator: ".").map(String.init)
        } else {
            prerelease = nil
        }
        core = parsed
    }

    private static func validIdentifiers(_ value: String, numericLeadingZeroesAllowed: Bool) -> Bool {
        let identifiers = value.split(separator: ".", omittingEmptySubsequences: false)
        return !identifiers.isEmpty && identifiers.allSatisfy { identifier in
            let text = String(identifier)
            guard !text.isEmpty,
                  text.unicodeScalars.allSatisfy({ CharacterSet.alphanumerics.contains($0) || $0 == "-" }) else { return false }
            return numericLeadingZeroesAllowed || !text.allSatisfy(\.isNumber) || text == "0" || !text.hasPrefix("0")
        }
    }

    static func < (lhs: Self, rhs: Self) -> Bool {
        for index in lhs.core.indices where lhs.core[index] != rhs.core[index] {
            return lhs.core[index] < rhs.core[index]
        }
        switch (lhs.prerelease, rhs.prerelease) {
        case (nil, nil): return false
        case (nil, .some): return false
        case (.some, nil): return true
        case let (.some(left), .some(right)):
            for index in 0..<max(left.count, right.count) {
                guard index < left.count else { return true }
                guard index < right.count else { return false }
                let a = left[index], b = right[index]
                if a == b { continue }
                let aNumeric = a.allSatisfy(\.isNumber)
                let bNumeric = b.allSatisfy(\.isNumber)
                if aNumeric && bNumeric {
                    if a.count != b.count { return a.count < b.count }
                    return a < b
                }
                if aNumeric != bNumeric { return aNumeric }
                return a < b
            }
            return false
        }
    }

    static func relation(candidate: String, installed: String) -> ThemePackageVersionRelation? {
        guard let candidate = Self(candidate), let installed = Self(installed) else { return nil }
        if candidate == installed { return .same }
        return candidate < installed ? .downgrade : .upgrade
    }
}
