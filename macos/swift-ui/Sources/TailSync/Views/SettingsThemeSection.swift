import AppKit
import SwiftUI
import UniformTypeIdentifiers

extension SettingsView {
    var colorThemePicker: some View {
        VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(Loc.t("settings.colorTheme"))
                    .font(activeTheme.readingFont(size: 13, weight: .medium))
                Text(Loc.t("settings.colorThemeDescription"))
                    .font(activeTheme.readingFont(size: 10))
                    .foregroundColor(palette.tertiaryColor)
            }

            if !loc.themeDescriptors.isEmpty {
                LazyVGrid(
                    columns: [GridItem(.adaptive(minimum: 190), spacing: 8)],
                    spacing: 8
                ) {
                    ForEach(loc.themeDescriptors) { descriptor in
                        themeV2Card(descriptor)
                    }
                }
            }

            if !loc.themeCatalogueLoaded {
                HStack(spacing: 6) {
                    if loc.themeCatalogueLoadFailed {
                        Label(Loc.t("error.localServiceUnavailable"), systemImage: "exclamationmark.triangle")
                            .foregroundColor(palette.warningColor)
                        Spacer()
                        Button(Loc.t("settings.retry")) {
                            Task { @MainActor in await loc.retryThemeCatalogueLoading() }
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                    } else {
                        ProgressView().controlSize(.small)
                        Text(Loc.t("settings.loading"))
                            .foregroundColor(palette.tertiaryColor)
                    }
                }
                .font(activeTheme.readingFont(size: 10))
            } else if ThemeCatalogueDisplayPolicy.shouldShowFallback(
                catalogueLoaded: loc.themeCatalogueLoaded,
                activeThemeId: loc.colorTheme,
                validThemeIds: loc.themeDescriptors
                    .filter { $0.status == "valid" }
                    .map(\.id)
            ) {
                Label(Loc.t("settings.themePackageFallback"), systemImage: "exclamationmark.triangle")
                    .font(activeTheme.readingFont(size: 10))
                    .foregroundColor(palette.warningColor)
            }

            HStack(spacing: 8) {
                Button {
                    selectThemePackage(for: .install)
                } label: {
                    Label(Loc.t("settings.themePackageImport"), systemImage: "square.and.arrow.down")
                }
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    func themeV2Card(_ descriptor: ApiClient.ThemeV2Descriptor) -> some View {
        let selected = loc.colorTheme == descriptor.id
        let name: String = switch descriptor.id {
        case "builtin:canvas@1": Loc.t("settings.colorTheme.tailsync")
        case "builtin:flux@1": Loc.t("settings.colorTheme.ocean")
        case "builtin:ledger@1": Loc.t("settings.colorTheme.forest")
        case "builtin:aura@1": Loc.t("settings.colorTheme.rose")
        case "builtin:mono@1": Loc.t("settings.colorTheme.high-contrast")
        default: descriptor.name[loc.lang] ?? descriptor.name["en"] ?? descriptor.id
        }
        let previewSelection = ThemeV2CardPreviewPolicy.selection(
            themeId: descriptor.id,
            catalogue: loc.resolvedV2Themes,
            reduceTransparency: loc.reduceTransparency,
            interfaceScale: min(activeTheme.interfaceScale, 1.25)
        )
        return ThemeV2CardView(
            descriptor: descriptor,
            name: name,
            selected: selected,
            selection: previewSelection,
            colorScheme: colorScheme,
            onSelect: { Task { @MainActor in await loc.selectLocalTheme(id: descriptor.id) } },
            onUpdate: { selectThemePackage(for: .update(themeId: descriptor.id, installedVersion: descriptor.version)) },
            onRollback: { rollbackThemeV2(descriptor) },
            onDelete: { deleteThemeV2(descriptor) }
        )
    }

    func selectThemePackage(for operation: ThemePackageOperation) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "tailsync-theme")!]
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.title = Loc.t(operation.isInstall ? "settings.themePackageImportTitle" : "settings.themePackageUpdateTitle")
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { @MainActor in
            do {
                let light = try await ApiClient.shared.validateThemeV2(path: url.path, mode: "light")
                let dark = try await ApiClient.shared.validateThemeV2(path: url.path, mode: "dark")
                let highLight = try await ApiClient.shared.validateThemeV2(path: url.path, mode: "light", highContrast: true)
                let highDark = try await ApiClient.shared.validateThemeV2(path: url.path, mode: "dark", highContrast: true)
                let validations = [light, dark, highLight, highDark]
                let diagnostics = validations.flatMap(\.diagnostics)
                guard validations.allSatisfy(\.valid),
                      let digest = light.digest,
                      let candidateVersion = light.candidateVersion,
                      validations.allSatisfy({ $0.digest == digest }),
                      validations.allSatisfy({ $0.candidateVersion == candidateVersion }),
                      let lightTokens = light.previewTokens,
                      let darkTokens = dark.previewTokens,
                      let highLightTokens = highLight.previewTokens,
                      let highDarkTokens = highDark.previewTokens else {
                    throw ApiError.serverError(diagnostics.map(\.message).joined(separator: "\n"))
                }
                if case .update(let themeId, _) = operation,
                   validations.contains(where: { $0.previewId != themeId }) {
                    throw ApiError.serverError(Loc.t("settings.themePackageIdMismatch"))
                }
                var images: [String: NSImage] = [:]
                for slot in light.previewAssetSlots.keys where ["logo", "emptyState", "previewPlaceholder"].contains(slot) {
                    if let data = try? await ApiClient.shared.previewThemeAssetSlot(path: url.path, digest: digest, slot: slot),
                       let image = ThemeAssetImageDecoder.decode(data, slot: slot) {
                        images[slot] = image
                    }
                }
                pendingThemeImport = PendingThemeImport(
                    path: url.path,
                    digest: digest,
                    standard: TailSyncThemeDefinition.resolvedV2(id: "preview", light: lightTokens, dark: darkTokens),
                    highContrast: TailSyncThemeDefinition.resolvedV2(id: "preview-high", light: highLightTokens, dark: highDarkTokens),
                    diagnostics: diagnostics,
                    assetImages: images,
                    candidateVersion: candidateVersion,
                    versionRelation: {
                        if case .update(_, let installedVersion) = operation {
                            return ThemePackageSemanticVersion.relation(
                                candidate: candidateVersion,
                                installed: installedVersion
                            )
                        }
                        return nil
                    }(),
                    operation: operation
                )
            } catch {
                actionErrorMessage = error.localizedDescription
            }
        }
    }

    func themeImportPreview(_ preview: PendingThemeImport) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(Loc.t("settings.themePackagePreview")).font(.headline)
            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 10) {
                previewSwatch(Loc.t("settings.themePreviewLight"), theme: preview.standard, scheme: .light)
                previewSwatch(Loc.t("settings.themePreviewDark"), theme: preview.standard, scheme: .dark)
                previewSwatch(Loc.t("settings.themePreviewHighLight"), theme: preview.highContrast, scheme: .light)
                previewSwatch(Loc.t("settings.themePreviewHighDark"), theme: preview.highContrast, scheme: .dark)
            }
            if let logo = preview.assetImages["logo"] {
                Image(nsImage: logo).resizable().aspectRatio(contentMode: .fit).frame(width: 42, height: 42).frame(maxWidth: .infinity, alignment: .center)
            }
            HStack(spacing: 12) {
                if let empty = preview.assetImages["emptyState"] {
                    Image(nsImage: empty).resizable().aspectRatio(contentMode: .fit).frame(width: 46, height: 34)
                }
                if let placeholder = preview.assetImages["previewPlaceholder"] {
                    Image(nsImage: placeholder).resizable().aspectRatio(contentMode: .fit).frame(width: 58, height: 34)
                }
            }
            Text(themeVersionDescription(preview))
                .font(.caption.monospacedDigit())
                .foregroundColor(preview.versionRelation == .downgrade ? palette.warningColor : palette.secondaryColor)
            if !preview.diagnostics.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(preview.diagnostics.indices, id: \.self) { index in
                        Text(preview.diagnostics[index].message)
                            .font(.caption)
                            .foregroundColor(preview.diagnostics[index].severity == "error" ? .red : .orange)
                    }
                }
            }
            HStack {
                Spacer()
                Button(Loc.t("common.cancel")) { pendingThemeImport = nil }
                Button(Loc.t(preview.operation.isInstall ? "settings.themePackageInstall" : "settings.themePackageUpdate")) {
                    applyPreviewedTheme(preview)
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(20)
        .frame(width: 640)
    }

    func previewSwatch(
        _ title: String,
        theme: TailSyncThemeDefinition,
        scheme: ColorScheme
    ) -> some View {
        let palette = scheme == .light ? theme.lightPalette : theme.darkPalette
        let components = scheme == .light ? theme.components : theme.darkComponents
        let search = components["search"]?["focus"]
        let hover = components["history"]?["hover"]
        let selected = components["history"]?["selected"]
        let buttonStates = ["default", "hover", "active", "selected", "disabled", "focus"]
        return VStack(alignment: .leading, spacing: 8) {
            Text(title).font(.caption.weight(.semibold)).foregroundColor(palette.primaryColor)
            VStack(alignment: .leading, spacing: 7) {
                Text(Loc.t("settings.themePreviewSearch"))
                    .font(.caption)
                    .foregroundColor(search?.foregroundColor ?? palette.primaryColor)
                    .padding(search?.padding ?? 7)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(search?.backgroundColor ?? palette.softSurfaceColor)
                    .clipShape(RoundedRectangle(cornerRadius: search?.radius ?? 6))
                    .overlay(RoundedRectangle(cornerRadius: search?.radius ?? 6).stroke(search?.focusRingColor ?? palette.accentColor, lineWidth: 2))
                Text(Loc.t("settings.themePreviewHover"))
                    .font(.caption)
                    .foregroundColor(hover?.foregroundColor ?? palette.primaryColor)
                    .padding(hover?.padding ?? 7)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(hover?.backgroundColor ?? palette.hoverColor)
                Text(Loc.t("settings.themePreviewSelected"))
                    .font(.caption.weight(.medium))
                    .foregroundColor(selected?.foregroundColor ?? palette.accentContrastColor)
                    .padding(selected?.padding ?? 7)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(selected?.backgroundColor ?? palette.accentColor)
            }
            .padding(8)
            .background(palette.surfaceColor)
            .clipShape(RoundedRectangle(cornerRadius: 7))
            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible()), GridItem(.flexible())], spacing: 5) {
                ForEach(buttonStates, id: \.self) { state in
                    let token = components["button"]?[state]
                    VStack(alignment: .leading, spacing: 3) {
                        Text(previewStateLabel(state))
                            .font(.system(size: 9))
                            .foregroundColor(palette.tertiaryColor)
                        Text(previewStateLabel(state))
                            .font(.system(size: 10, weight: .medium))
                            .foregroundColor(token?.foregroundColor ?? palette.primaryColor)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 5)
                            .padding(.horizontal, token?.padding ?? 5)
                            .background(token?.backgroundColor ?? palette.raisedColor)
                            .clipShape(RoundedRectangle(cornerRadius: token?.radius ?? 5, style: .continuous))
                            .overlay {
                                RoundedRectangle(cornerRadius: token?.radius ?? 5, style: .continuous)
                                    .stroke(token?.borderColor ?? palette.borderColor, lineWidth: state == "focus" ? 2 : 1)
                            }
                            .opacity(state == "disabled" ? 0.55 : 1)
                    }
                }
            }
        }.padding(10).frame(maxWidth: .infinity, alignment: .leading).background(palette.windowColor).clipShape(RoundedRectangle(cornerRadius: 8))
    }

    func previewStateLabel(_ state: String) -> String {
        switch state {
        case "default": return Loc.t("settings.themePreviewStateDefault")
        case "hover": return Loc.t("settings.themePreviewStateHover")
        case "active": return Loc.t("settings.themePreviewStateActive")
        case "selected": return Loc.t("settings.themePreviewStateSelected")
        case "disabled": return Loc.t("settings.themePreviewStateDisabled")
        case "focus": return Loc.t("settings.themePreviewStateFocus")
        default: return state
        }
    }

    func themeVersionDescription(_ preview: PendingThemeImport) -> String {
        if case .update(_, let installedVersion) = preview.operation {
            return "\(installedVersion) → \(preview.candidateVersion)"
        }
        return "\(Loc.t("settings.themePackageCandidateVersion")): \(preview.candidateVersion)"
    }

    func applyPreviewedTheme(_ preview: PendingThemeImport) {
        if case .update(_, let installedVersion) = preview.operation,
           preview.versionRelation == .same || preview.versionRelation == .downgrade {
            let alert = NSAlert()
            alert.messageText = Loc.t(preview.versionRelation == .same
                ? "settings.themePackageReplaceTitle"
                : "settings.themePackageDowngradeTitle")
            alert.informativeText = "\(installedVersion) → \(preview.candidateVersion)"
            alert.addButton(withTitle: Loc.t("settings.themePackageUpdate"))
            alert.addButton(withTitle: Loc.t("common.cancel"))
            guard alert.runModal() == .alertFirstButtonReturn else { return }
        }
        let updateOptions = ThemePackageUpdateOptions.forRelation(preview.versionRelation)
        Task { @MainActor in
            do {
                switch preview.operation {
                case .install:
                    _ = try await ApiClient.shared.installThemeV2(path: preview.path, digest: preview.digest)
                case .update:
                    _ = try await ApiClient.shared.updateThemeV2(
                        path: preview.path,
                        digest: preview.digest,
                        allowSameVersion: updateOptions.allowSameVersion,
                        allowDowngrade: updateOptions.allowDowngrade
                    )
                }
                pendingThemeImport = nil
                await loc.refreshThemesV2()
                await loc.reloadActiveThemeAfterPackageChange()
                saved = true
                try? await Task.sleep(nanoseconds: 1_200_000_000)
                saved = false
            } catch { actionErrorMessage = error.localizedDescription }
        }
    }

    func rollbackThemeV2(_ descriptor: ApiClient.ThemeV2Descriptor) {
        let alert = NSAlert()
        alert.messageText = Loc.t("settings.themePackageRollbackTitle")
        alert.informativeText = descriptor.name[loc.lang] ?? descriptor.name["en"] ?? descriptor.id
        alert.addButton(withTitle: Loc.t("settings.themePackageRollback"))
        alert.addButton(withTitle: Loc.t("common.cancel"))
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        Task { @MainActor in
            do {
                _ = try await ApiClient.shared.rollbackThemeV2(id: descriptor.id)
                await loc.refreshThemesV2()
                await loc.reloadActiveThemeAfterPackageChange()
            } catch {
                actionErrorMessage = error.localizedDescription
            }
        }
    }

    func deleteThemeV2(_ descriptor: ApiClient.ThemeV2Descriptor) {
        let alert = NSAlert()
        alert.messageText = Loc.t("settings.themePackageDeleteTitle")
        alert.informativeText = descriptor.name[loc.lang] ?? descriptor.name["en"] ?? descriptor.id
        alert.addButton(withTitle: Loc.t("settings.themePackageDelete"))
        alert.addButton(withTitle: Loc.t("common.cancel"))
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        Task { @MainActor in
            do {
                try await ApiClient.shared.deleteThemeV2(id: descriptor.id, storageHandle: descriptor.storageHandle)
                await loc.refreshThemesV2()
                await loc.syncLocalThemeSettings()
            } catch {
                actionErrorMessage = error.localizedDescription
            }
        }
    }

}
