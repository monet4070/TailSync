import AppKit
import XCTest
@testable import TailSync

private actor ThemeStartupAttemptCounter {
    private var attempts = 0

    func succeedOnThirdAttempt() -> Bool {
        attempts += 1
        return attempts == 3
    }

    func value() -> Int { attempts }
}

final class ThemeTests: XCTestCase {
    func testThemeAssetsAreDecodedAtTheirDisplayBound() {
        guard let representation = NSBitmapImageRep(
            bitmapDataPlanes: nil,
            pixelsWide: 1200,
            pixelsHigh: 800,
            bitsPerSample: 8,
            samplesPerPixel: 4,
            hasAlpha: true,
            isPlanar: false,
            colorSpaceName: .deviceRGB,
            bytesPerRow: 1200 * 4,
            bitsPerPixel: 32
        ),
        let data = representation.representation(using: .png, properties: [:]),
        let image = ThemeAssetImageDecoder.decode(data, slot: "logo") else {
            return XCTFail("expected a decodable PNG asset")
        }

        XCTAssertLessThanOrEqual(image.size.width, 96)
        XCTAssertLessThanOrEqual(image.size.height, 96)
    }

    func testStatusItemLogoDoesNotMutateTheCachedThemeImage() throws {
        let image = NSImage(size: NSSize(width: 64, height: 64))
        image.isTemplate = true
        let copy = try XCTUnwrap(StatusItemImagePolicy.image(from: image))

        XCTAssertEqual(image.size, NSSize(width: 64, height: 64))
        XCTAssertTrue(image.isTemplate)
        XCTAssertEqual(copy.size, NSSize(width: 18, height: 18))
        XCTAssertFalse(copy.isTemplate)
    }

    func testThemeCardPreviewResolvesTheThemeBeingPresented() {
        let canvas = TailSyncThemeDefinition.resolvedV2(
            id: "builtin:canvas@1",
            light: ["colors": ["accent": ["default": "#d5684b"]]],
            dark: ["colors": ["accent": ["default": "#ec8668"]]]
        )
        let flux = TailSyncThemeDefinition.resolvedV2(
            id: "builtin:flux@1",
            light: ["colors": ["accent": ["default": "#147970"]]],
            dark: ["colors": ["accent": ["default": "#65c8bd"]]]
        )

        let preview = ThemeV2CardPreviewPolicy.selection(
            themeId: flux.id,
            catalogue: [canvas, flux],
            reduceTransparency: false,
            interfaceScale: 1
        )

        XCTAssertEqual(preview.definition?.id, flux.id)
        XCTAssertNotEqual(preview.palette(for: .light), canvas.lightPalette)
        XCTAssertEqual(preview.palette(for: .light), flux.lightPalette)
    }

    func testThemeCardLayoutProvidesAFullCardTargetAndUsableActions() {
        XCTAssertGreaterThanOrEqual(ThemeV2CardLayout.minimumCardHeight, 96)
        XCTAssertGreaterThanOrEqual(ThemeV2CardLayout.minimumActionHitSize, 28)
    }

    func testThemeStartupRetryRecoversWhenDaemonBecomesReady() async {
        let counter = ThemeStartupAttemptCounter()

        let loaded = await ThemeStartupRetryPolicy.run(
            maximumAttempts: 5,
            delayNanoseconds: 0
        ) {
            await counter.succeedOnThirdAttempt()
        }

        let attempts = await counter.value()
        XCTAssertTrue(loaded)
        XCTAssertEqual(attempts, 3)
    }

    func testThemeStartupRetryStopsAfterItsBoundedAttemptCount() async {
        let counter = ThemeStartupAttemptCounter()

        let loaded = await ThemeStartupRetryPolicy.run(
            maximumAttempts: 4,
            delayNanoseconds: 0
        ) {
            _ = await counter.succeedOnThirdAttempt()
            return false
        }

        let attempts = await counter.value()
        XCTAssertFalse(loaded)
        XCTAssertEqual(attempts, 4)
    }

    func testThemeFallbackWarningRequiresALoadedCatalogueAndMissingSelection() {
        let canvas = "builtin:canvas@1"

        XCTAssertFalse(ThemeCatalogueDisplayPolicy.shouldShowFallback(
            catalogueLoaded: false,
            activeThemeId: canvas,
            validThemeIds: []
        ))
        XCTAssertFalse(ThemeCatalogueDisplayPolicy.shouldShowFallback(
            catalogueLoaded: true,
            activeThemeId: canvas,
            validThemeIds: [canvas]
        ))
        XCTAssertTrue(ThemeCatalogueDisplayPolicy.shouldShowFallback(
            catalogueLoaded: true,
            activeThemeId: "custom:studio.night",
            validThemeIds: [canvas]
        ))
    }

    func testResolutionCommitPolicyRejectsOutOfOrderAndStaleResponses() {
        let standard = ThemeResolutionRequest(
            identity: ThemeResolutionCacheIdentity(
                themeId: "custom:studio.night",
                packageDigest: "digest-v1",
                highContrast: false
            ),
            generation: 1
        )
        let highContrast = ThemeResolutionRequest(
            identity: ThemeResolutionCacheIdentity(
                themeId: "custom:studio.night",
                packageDigest: "digest-v1",
                highContrast: true
            ),
            generation: 2
        )

        XCTAssertFalse(ThemeLoadCommitPolicy.canCommitResolution(
            standard,
            latestGeneration: 2,
            currentPackageDigest: "digest-v1",
            effectiveHighContrast: true
        ))
        XCTAssertTrue(ThemeLoadCommitPolicy.canCommitResolution(
            highContrast,
            latestGeneration: 2,
            currentPackageDigest: "digest-v1",
            effectiveHighContrast: true
        ))
        XCTAssertFalse(ThemeLoadCommitPolicy.canCommitResolution(
            highContrast,
            latestGeneration: 2,
            currentPackageDigest: "digest-v2",
            effectiveHighContrast: true
        ))
    }

    func testResolutionFailureFallsBackOnlyForTheCurrentRequestAndActiveTheme() {
        let request = ThemeResolutionRequest(
            identity: ThemeResolutionCacheIdentity(
                themeId: "custom:studio.night",
                packageDigest: "digest-v1",
                highContrast: false
            ),
            generation: 3
        )

        XCTAssertFalse(ThemeLoadCommitPolicy.shouldFallbackAfterResolutionFailure(
            request,
            latestGeneration: 4,
            activeThemeId: "custom:studio.night"
        ))
        XCTAssertFalse(ThemeLoadCommitPolicy.shouldFallbackAfterResolutionFailure(
            request,
            latestGeneration: 3,
            activeThemeId: "custom:other.theme"
        ))
        XCTAssertTrue(ThemeLoadCommitPolicy.shouldFallbackAfterResolutionFailure(
            request,
            latestGeneration: 3,
            activeThemeId: "custom:studio.night"
        ))
    }

    func testAssetCommitPolicyRejectsOldThemeDigestAndGeneration() {
        let request = ThemeAssetLoadRequest(
            themeId: "custom:studio.night",
            packageDigest: "digest-v1",
            generation: 5
        )

        XCTAssertFalse(ThemeLoadCommitPolicy.canCommitAssets(
            request,
            latestGeneration: 6,
            activeThemeId: "custom:studio.night",
            currentDefinitionDigest: "digest-v1",
            currentDescriptorDigest: "digest-v1"
        ))
        XCTAssertFalse(ThemeLoadCommitPolicy.canCommitAssets(
            request,
            latestGeneration: 5,
            activeThemeId: "custom:other.theme",
            currentDefinitionDigest: "digest-v1",
            currentDescriptorDigest: "digest-v1"
        ))
        XCTAssertFalse(ThemeLoadCommitPolicy.canCommitAssets(
            request,
            latestGeneration: 5,
            activeThemeId: "custom:studio.night",
            currentDefinitionDigest: "digest-v2",
            currentDescriptorDigest: "digest-v2"
        ))
        XCTAssertTrue(ThemeLoadCommitPolicy.canCommitAssets(
            request,
            latestGeneration: 5,
            activeThemeId: "custom:studio.night",
            currentDefinitionDigest: "digest-v1",
            currentDescriptorDigest: "digest-v1"
        ))
    }

    func testCanvasFallbackCommitPolicyRejectsASelectionThatChangedDuringResolution() {
        XCTAssertFalse(ThemeLoadCommitPolicy.canCommitCanvasFallback(
            selectionGeneration: 7,
            latestSelectionGeneration: 8,
            colorTheme: "custom:studio.night",
            intendedThemeId: "custom:studio.night"
        ))
        XCTAssertFalse(ThemeLoadCommitPolicy.canCommitCanvasFallback(
            selectionGeneration: 7,
            latestSelectionGeneration: 7,
            colorTheme: "custom:studio.night",
            intendedThemeId: "builtin:canvas@1"
        ))
        XCTAssertTrue(ThemeLoadCommitPolicy.canCommitCanvasFallback(
            selectionGeneration: 7,
            latestSelectionGeneration: 7,
            colorTheme: "builtin:canvas@1",
            intendedThemeId: "builtin:canvas@1"
        ))
    }

    func testThemeRefreshCommitPolicyRejectsAnOutOfOrderDescriptorSnapshot() {
        XCTAssertFalse(ThemeLoadCommitPolicy.canCommitThemeRefresh(
            requestGeneration: 4,
            latestGeneration: 5
        ))
        XCTAssertTrue(ThemeLoadCommitPolicy.canCommitThemeRefresh(
            requestGeneration: 5,
            latestGeneration: 5
        ))
    }

    func testResolvedPairDecoderRejectsMixedCoreSnapshots() {
        let light = resolvedResponse(mode: "light")
        let dark = resolvedResponse(mode: "dark")
        XCTAssertNotNil(ThemeResolvedPairDecoder.decode(
            themeId: "custom:studio.night",
            highContrast: true,
            lightData: light,
            darkData: dark
        ))

        var mixedDigest = dark
        mixedDigest["digest"] = "digest-v2"
        XCTAssertNil(ThemeResolvedPairDecoder.decode(
            themeId: "custom:studio.night",
            highContrast: true,
            lightData: light,
            darkData: mixedDigest
        ))

        var mixedId = dark
        mixedId["id"] = "custom:other.theme"
        XCTAssertNil(ThemeResolvedPairDecoder.decode(
            themeId: "custom:studio.night",
            highContrast: true,
            lightData: light,
            darkData: mixedId
        ))

        var mixedContrast = dark
        mixedContrast["highContrast"] = false
        XCTAssertNil(ThemeResolvedPairDecoder.decode(
            themeId: "custom:studio.night",
            highContrast: true,
            lightData: light,
            darkData: mixedContrast
        ))
    }

    private func resolvedResponse(mode: String) -> [String: Any] {
        [
            "id": "custom:studio.night",
            "digest": "digest-v1",
            "mode": mode,
            "highContrast": true,
            "tokens": ["colors": ["background": ["canvas": "#101010"]]],
            "assetSlots": [String: Any]()
        ]
    }

    func testThemeResolutionCacheIdentityIncludesHighContrast() {
        let cached = ThemeResolutionCacheIdentity(
            themeId: "custom:studio.night",
            packageDigest: "digest-v1",
            highContrast: false
        )

        XCTAssertTrue(ThemeResolutionCacheIdentity.canReuse(
            cached,
            themeId: "custom:studio.night",
            packageDigest: "digest-v1",
            highContrast: false
        ))
        XCTAssertFalse(ThemeResolutionCacheIdentity.canReuse(
            cached,
            themeId: "custom:studio.night",
            packageDigest: "digest-v1",
            highContrast: true
        ))
        XCTAssertFalse(ThemeResolutionCacheIdentity.canReuse(
            cached,
            themeId: "custom:studio.night",
            packageDigest: "digest-v2",
            highContrast: false
        ))
    }

    func testThemePackageSemanticVersionRelations() {
        XCTAssertEqual(
            ThemePackageSemanticVersion.relation(candidate: "1.0.0", installed: "1.0.0"),
            .same
        )
        XCTAssertEqual(
            ThemePackageSemanticVersion.relation(candidate: "1.0.0-beta.2", installed: "1.0.0-beta.11"),
            .downgrade
        )
        XCTAssertEqual(
            ThemePackageSemanticVersion.relation(candidate: "1.0.0", installed: "1.0.0-rc.1"),
            .upgrade
        )
        XCTAssertEqual(
            ThemePackageSemanticVersion.relation(candidate: "2.0.0", installed: "3.0.0"),
            .downgrade
        )

        XCTAssertEqual(
            ThemePackageUpdateOptions.forRelation(.same),
            ThemePackageUpdateOptions(allowSameVersion: true, allowDowngrade: false)
        )
        XCTAssertEqual(
            ThemePackageUpdateOptions.forRelation(.downgrade),
            ThemePackageUpdateOptions(allowSameVersion: false, allowDowngrade: true)
        )
        XCTAssertEqual(
            ThemePackageUpdateOptions.forRelation(.upgrade),
            ThemePackageUpdateOptions(allowSameVersion: false, allowDowngrade: false)
        )
    }

    func testResolvedV2AdapterMapsSemanticTokens() {
        let light: [String: Any] = [
            "colors": ["accent": ["default": "#102030"], "background": ["canvas": "#405060", "surface": "#708090"], "text": ["primary": "#A0B0C0", "secondary": "#D0E0F0"]],
            "shape": ["surfaceRadius": 18], "density": ["row": 11], "typography": ["search": ["size": 15], "ui": ["size": 14]]
        ]
        let definition = TailSyncThemeDefinition.resolvedV2(id: "custom:night@1", light: light, dark: [:])
        XCTAssertEqual(definition.id, "custom:night@1")
        XCTAssertEqual(definition.lightPalette.accent, 0x102030)
        XCTAssertEqual(definition.lightPalette.window, 0x405060)
        XCTAssertEqual(definition.metrics.cardRadius, 18)
        XCTAssertEqual(definition.typography.searchSize, 15)
    }

    func testResolvedV2AdapterFallsBackForMissingOrMalformedTokens() {
        let definition = TailSyncThemeDefinition.resolvedV2(id: "builtin:canvas@1", light: ["colors": ["accent": ["default": "oops"]]], dark: [:])
        XCTAssertEqual(definition.lightPalette.accent, TailSyncColorTheme.tailsync.palette(for: .light).accent)
        XCTAssertEqual(definition.darkPalette.window, TailSyncColorTheme.tailsync.palette(for: .dark).window)
    }

    func testResolvedV2AdapterMapsComponentStates() {
        let definition = TailSyncThemeDefinition.resolvedV2(id: "custom:components", light: [
            "components": [
                "search": [
                    "focus": ["background": "#123456", "foreground": "#ffffff", "border": "#abcdef", "focusRing": "#fedcba", "radius": 5, "padding": 12]
                ]
            ]
        ], dark: [:])
        let tokens = definition.components["search"]?["focus"]
        XCTAssertEqual(tokens?.background, 0x123456)
        XCTAssertEqual(tokens?.foreground, 0xffffff)
        XCTAssertEqual(tokens?.radius, 5)
        XCTAssertEqual(tokens?.padding, 12)
    }

    func testResolvedV2AdapterRetainsEveryInteractiveButtonState() {
        let states = ["default", "hover", "active", "selected", "disabled", "focus"]
        var resolvedStates: [String: Any] = [:]
        for (index, state) in states.enumerated() {
            resolvedStates[state] = [
                "background": String(format: "#%06X", index + 1),
                "foreground": "#ffffff",
                "focusRing": "#ffff00"
            ]
        }
        let definition = TailSyncThemeDefinition.resolvedV2(
            id: "custom:button-states",
            light: ["components": ["button": resolvedStates]],
            dark: [:]
        )

        XCTAssertEqual(Set(definition.components["button"]?.keys.map { $0 } ?? []), Set(states))
        XCTAssertEqual(definition.components["button"]?["active"]?.background, 0x000003)
        XCTAssertEqual(definition.components["button"]?["focus"]?.focusRing, 0xffff00)
    }

    func testV2SelectionUsesExactDescriptorId() {
        let definition = TailSyncThemeDefinition.resolvedV2(id: "custom:night@1", light: [:], dark: [:])
        XCTAssertNotNil(TailSyncThemeSelection(storedValue: "custom:night@1", catalogue: [definition]).definition)
        XCTAssertNil(TailSyncThemeSelection(storedValue: "custom:missing@1", catalogue: [definition]).definition)
    }

    func testResolvedV2AdapterPreservesRgbAndAlphaColors() {
        let definition = TailSyncThemeDefinition.resolvedV2(id: "custom:alpha", light: [
            "colors": [
                "accent": ["default": "#0102031A", "hover": "rgba(2, 3, 4, 0.15)", "onAccent": "rgba(4, 5, 6, 0.2)", "soft": "rgba(40, 50, 60, 0.4)"],
                "background": ["canvas": "#11223380", "surface": "rgba(7, 8, 9, 0.3)", "input": "rgba(10, 20, 30, 0.25)", "hover": "#0B0C0D59", "active": "rgba(14, 15, 16, 0.4)", "raised": "#0A0B0C66", "toast": "rgba(13, 14, 15, 0.5)"],
                "text": ["primary": "#10111299", "secondary": "rgba(19, 20, 21, 0.7)", "tertiary": "#161718CC", "toast": "rgba(25, 26, 27, 0.9)"],
                "border": ["default": "rgba(27, 28, 29, 0.18)", "strong": "#1C1D1E33", "divider": "rgba(31, 32, 33, 0.22)"],
                "status": ["positive": "#22232471", "positiveSoft": "rgba(35, 36, 37, 0.26)", "warning": "rgba(37, 38, 39, 0.28)", "warningSoft": "#28292A4D", "info": "rgba(43, 44, 45, 0.32)", "infoSoft": "#2E2F3057"]
            ],
            "components": ["search": ["focus": [
                "background": "#10203080",
                "foreground": "rgba(255, 255, 255, 0.5)",
                "secondaryText": "#40506040",
                "border": "rgba(70, 80, 90, 0.3)",
                "focusRing": "#60708060",
                "icon": "rgba(100, 110, 120, 0.4)",
                "accent": "#8090A080",
                "shadow": ["radius": 8, "y": 3, "opacity": 0.2]
            ]]]
        ], dark: [:])
        let palette = definition.lightPalette
        XCTAssertEqual(palette.accentOpacity, 26.0 / 255.0, accuracy: 0.001)
        XCTAssertEqual(palette.accentHoverOpacity, 0.15, accuracy: 0.001)
        XCTAssertEqual(palette.accentSoftOpacity, 0.4, accuracy: 0.001)
        XCTAssertEqual(palette.accentContrastOpacity, 0.2, accuracy: 0.001)
        XCTAssertEqual(definition.lightPalette.window, 0x112233)
        XCTAssertEqual(palette.windowOpacity, 128.0 / 255.0, accuracy: 0.001)
        XCTAssertEqual(palette.surfaceOpacity, 0.3, accuracy: 0.001)
        XCTAssertEqual(definition.lightPalette.softSurface, 0x0A141E)
        XCTAssertEqual(palette.softSurfaceOpacity, 0.25, accuracy: 0.001)
        XCTAssertEqual(palette.hoverOpacity, 89.0 / 255.0, accuracy: 0.001)
        XCTAssertEqual(palette.activeOpacity, 0.4, accuracy: 0.001)
        XCTAssertEqual(palette.raisedOpacity, 102.0 / 255.0, accuracy: 0.001)
        XCTAssertEqual(palette.textPrimaryOpacity, 153.0 / 255.0, accuracy: 0.001)
        XCTAssertEqual(palette.textSecondaryOpacity, 0.7, accuracy: 0.001)
        XCTAssertEqual(palette.textTertiaryOpacity, 204.0 / 255.0, accuracy: 0.001)
        XCTAssertEqual(palette.borderOpacity, 0.18, accuracy: 0.001)
        XCTAssertEqual(palette.borderStrongOpacity, 51.0 / 255.0, accuracy: 0.001)
        XCTAssertEqual(palette.dividerOpacity, 0.22, accuracy: 0.001)
        XCTAssertEqual(palette.positiveOpacity, 113.0 / 255.0, accuracy: 0.001)
        XCTAssertEqual(palette.positiveSoftOpacity, 0.26, accuracy: 0.001)
        XCTAssertEqual(palette.warningOpacity, 0.28, accuracy: 0.001)
        XCTAssertEqual(palette.warningSoftOpacity, 77.0 / 255.0, accuracy: 0.001)
        XCTAssertEqual(palette.infoOpacity, 0.32, accuracy: 0.001)
        XCTAssertEqual(palette.infoSoftOpacity, 87.0 / 255.0, accuracy: 0.001)
        XCTAssertEqual(palette.toastOpacity, 0.5, accuracy: 0.001)
        XCTAssertEqual(palette.toastTextOpacity, 0.9, accuracy: 0.001)

        let component = definition.components["search"]?["focus"]
        XCTAssertEqual(component?.background, 0x102030)
        XCTAssertEqual(component?.backgroundOpacity ?? 0, 128.0 / 255.0, accuracy: 0.001)
        XCTAssertEqual(component?.foreground, 0xFFFFFF)
        XCTAssertEqual(component?.foregroundOpacity ?? 0, 0.5, accuracy: 0.001)
        XCTAssertEqual(component?.secondaryTextOpacity ?? 0, 64.0 / 255.0, accuracy: 0.001)
        XCTAssertEqual(component?.borderOpacity ?? 0, 0.3, accuracy: 0.001)
        XCTAssertEqual(component?.focusRingOpacity ?? 0, 96.0 / 255.0, accuracy: 0.001)
        XCTAssertEqual(component?.iconOpacity ?? 0, 0.4, accuracy: 0.001)
        XCTAssertEqual(component?.accentOpacity ?? 0, 128.0 / 255.0, accuracy: 0.001)
    }

    func testReducedTransparencyMakesColorsOpaqueAndRemovesShadows() {
        let definition = TailSyncThemeDefinition.resolvedV2(id: "custom:transparent", light: [
            "colors": ["background": ["canvas": "#11223380"]],
            "effects": ["shadow": ["radius": 9]],
            "components": ["panel": ["default": [
                "background": "rgba(10, 20, 30, 0.25)",
                "shadow": ["radius": 7, "y": 2, "opacity": 0.4]
            ]]]
        ], dark: [:])
        let selection = TailSyncThemeSelection(
            builtin: .tailsync,
            definition: definition,
            reduceTransparency: true
        )

        XCTAssertEqual(selection.palette(for: .light).windowOpacity, 1)
        XCTAssertEqual(selection.metrics(for: .light).shadowRadius, 0)
        let component = selection.component("panel", scheme: .light)
        XCTAssertEqual(component?.backgroundOpacity, 1)
        XCTAssertEqual(component?.shadowRadius, 0)
        XCTAssertEqual(component?.shadowY, 0)
        XCTAssertEqual(component?.shadowOpacity, 0)
    }

    func testInterfaceScaleOverridesThemeMetrics() {
        let selection = TailSyncThemeSelection(builtin: .tailsync, interfaceScale: 1.5)
        let base = TailSyncColorTheme.tailsync.metrics
        let scaled = selection.metrics(for: .light)
        XCTAssertEqual(scaled.cardRadius, base.cardRadius * 1.5)
        XCTAssertEqual(scaled.controlRadius, base.controlRadius * 1.5)
        XCTAssertEqual(scaled.rowPadding, base.rowPadding * 1.5)
        XCTAssertEqual(scaled.shadowRadius, base.shadowRadius * 1.5)
        XCTAssertEqual(TailSyncThemeAccessibilityPolicy.interfaceScale(for: .large), 1)
        XCTAssertEqual(TailSyncThemeAccessibilityPolicy.interfaceScale(for: .accessibility5), 2.2)
    }

    func testStructuredThemeErrorRetainsTheCompleteCoreContract() {
        let diagnostic = ApiClient.ThemeDiagnostic(
            code: "THEME_ID",
            message: "theme id does not match storage handle",
            jsonPointer: "/id",
            severity: "error",
            platforms: ["macos"],
            recoverable: true,
            fallbackApplied: false
        )
        let error = ApiError.themeError(diagnostic)

        guard case .themeError(let retained) = error else {
            return XCTFail("expected a structured theme error")
        }
        XCTAssertEqual(retained, diagnostic)
        XCTAssertEqual(error.localizedDescription, "THEME_ID: theme id does not match storage handle (/id)")
    }

    func testResolvedV2AdapterRetainsDarkStructuralTokens() {
        let definition = TailSyncThemeDefinition.resolvedV2(id: "custom:dark-metrics", light: [
            "shape": ["surfaceRadius": 4], "density": ["row": 8], "typography": ["search": ["size": 12]]
        ], dark: [
            "shape": ["surfaceRadius": 20], "density": ["row": 18], "typography": ["search": ["size": 22]]
        ])
        XCTAssertEqual(definition.metrics.cardRadius, 4)
        XCTAssertEqual(definition.darkMetrics.cardRadius, 20)
        XCTAssertEqual(definition.typography.searchSize, 12)
        XCTAssertEqual(definition.darkTypography.searchSize, 22)
    }
}
