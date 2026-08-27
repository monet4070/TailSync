import AppKit
import XCTest
@testable import TailSync

@MainActor
final class HistoryPreviewWebViewSVGRendererTests: XCTestCase {
    func testDocumentHTMLCarriesRestrictiveCSPAndEmbedsSource() {
        let source = #"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"/>"#
        let html = HistoryPreviewWebViewSVGRenderer.documentHTML(
            for: source,
            trustingExternalResources: false
        )

        XCTAssertTrue(html.contains("Content-Security-Policy"))
        XCTAssertTrue(html.contains("default-src 'none'"))
        XCTAssertTrue(html.contains("script-src 'none'"))
        XCTAssertTrue(html.contains("base-uri 'none'"))
        XCTAssertFalse(html.contains("img-src *"))
        // The CSP header precedes the body so it applies to the SVG markup.
        XCTAssertTrue(
            html.range(of: "Content-Security-Policy")!.lowerBound
                < html.range(of: "<svg")!.lowerBound
        )
    }

    func testTrustedDocumentHTMLOpensNetworkContentButNeverScripts() {
        let source = ##"<image href="https://cdn.example.com/logo.png"/>"##
        let html = HistoryPreviewWebViewSVGRenderer.documentHTML(
            for: source,
            trustingExternalResources: true
        )

        // Trust enumerates exactly the disclosed hosts — no scheme-wide
        // https: source, no wildcard, no blob, no external CSS, no media.
        XCTAssertTrue(html.contains("img-src data: https://cdn.example.com"))
        XCTAssertTrue(html.contains("font-src data: https://cdn.example.com"))
        XCTAssertFalse(html.contains("img-src https:"))
        XCTAssertFalse(html.contains("*"))
        XCTAssertFalse(html.contains("blob:"))
        XCTAssertFalse(html.contains("media-src https"))
        // Active capabilities stay blocked regardless of the user's choice.
        XCTAssertTrue(html.contains("script-src 'none'"))
        XCTAssertTrue(html.contains("base-uri 'none'"))
        XCTAssertTrue(html.contains("form-action 'none'"))
        XCTAssertTrue(html.contains("default-src 'none'"))
    }

    func testTrustedCSPOnlyListsDisclosedEligibleHosts() {
        let source = ##"""
<image href="https://cdn.example.com/a.png"/><image href="http://plain.example.org/b.png"/><image href="https://10.0.0.9/c.png"/>
"""##
        let html = HistoryPreviewWebViewSVGRenderer.documentHTML(
            for: source,
            trustingExternalResources: true
        )

        // Only the eligible host is loadable.  The rejected http/private
        // targets still appear in the embedded markup itself, so assert on
        // the CSP directive rather than the whole document.
        let csp = html.range(of: "Content-Security-Policy\" content=\"")!
        let policy = html[html.index(csp.upperBound, offsetBy: 0)...]
            .prefix(while: { $0 != "\"" })
        XCTAssertTrue(policy.contains("https://cdn.example.com"))
        XCTAssertFalse(policy.contains("plain.example.org"))
        XCTAssertFalse(policy.contains("10.0.0.9"))

        // No external references at all: trusted CSP degrades to data:-only,
        // never to a scheme-wide source.
        let none = HistoryPreviewWebViewSVGRenderer.documentHTML(
            for: "<svg/>",
            trustingExternalResources: true
        )
        XCTAssertTrue(none.contains("img-src data:;"))
    }

    func testTrustedCSPPreservesAnExplicitHTTPSPort() {
        let source = ##"<image href="https://cdn.example.com:8443/a.png"/>"##
        let html = HistoryPreviewWebViewSVGRenderer.documentHTML(
            for: source,
            trustingExternalResources: true
        )

        XCTAssertTrue(html.contains("img-src data: https://cdn.example.com:8443"))
        XCTAssertTrue(html.contains("font-src data: https://cdn.example.com:8443"))
    }

    /// Regression for the reported disclosure bypass: `srcset` attributes
    /// and HTML-entity-encoded URLs (`&#58;`) are browser-loadable forms the
    /// extractor must cover, and loopback hostnames must be refused.
    func testExtractorCoversSrcsetAndHTMLEntityEncodedURLs() {
        let source = ##"""
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
  <image srcset="https://localhost:9443/undisclosed.png 1x, https://localhost:9443/undisclosed@2x.png 2x"/>
  <image href="https&#58;//localhost:9443/undisclosed.png"/>
  <image xlink:href="https&#x3a;//hex-entity.example.com/e.png"/>
  <image srcset="https://srcset.example.org/img.png 2x"/>
</svg>
"""##
        let summary = HistoryPreviewWebViewSVGRenderer.externalReferenceSummary(for: source)
        // All reference forms are disclosed: the loopback hostname (both via
        // srcset and the entity-encoded href) is refused, while the public
        // hosts — plain srcset and hex-entity-encoded — are allowed.
        XCTAssertEqual(summary.rejectedHosts, ["localhost"])
        XCTAssertEqual(Set(summary.allowedHosts), ["hex-entity.example.com", "srcset.example.org"])

        // The entity-encoded forms must be recognized as URLs at all.
        let hosts = Set(
            HistoryPreviewWebViewSVGRenderer.externalReferences(in: source)
                .compactMap { $0.host?.lowercased() }
        )
        XCTAssertEqual(hosts, ["localhost", "hex-entity.example.com", "srcset.example.org"])
    }

    func testTrustEligibilityRefusesLocalhostHostnames() {
        func eligible(_ string: String) -> Bool {
            HistoryPreviewWebViewSVGRenderer.isTrustEligibleURL(URL(string: string)!)
        }
        XCTAssertFalse(eligible("https://localhost:9443/x.png"))
        XCTAssertFalse(eligible("https://printer.local/x.png"))
        XCTAssertFalse(eligible("https://a.localhost/x.png"))
        XCTAssertTrue(eligible("https://cdn.example.com/x.png"))
    }

    func testExternalReferenceExtractionCoversAttributesAndCSS() {
        let source = ##"""
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
  <image href="https://cdn.example.com/logo.png" xlink:href="https://cdn.example.com/alt.png"/>
  <image src="http://plain.example.com/pic.jpg"/>
  <text style="fill:url(#local)">local</text>
  <style>.a { background: url('https://fonts.example.com/bg.png'); } .b { fill: url("https://other.example.net/x") }</style>
  <use href="#fragment"/>
  <image href="data:image/png;base64,AAAA"/>
</svg>
"""##
        let hosts = HistoryPreviewWebViewSVGRenderer.externalReferences(in: source)
            .compactMap { $0.host?.lowercased() }
        XCTAssertEqual(
            Set(hosts),
            ["cdn.example.com", "plain.example.com", "fonts.example.com", "other.example.net"]
        )
    }

    func testTrustEligibilityRequiresHTTPSAndPublicLiteralIPs() {
        func eligible(_ string: String) -> Bool {
            HistoryPreviewWebViewSVGRenderer.isTrustEligibleURL(URL(string: string)!)
        }
        XCTAssertTrue(eligible("https://cdn.example.com/logo.png"))
        XCTAssertTrue(eligible("https://8.8.8.8/logo.png"))
        XCTAssertFalse(eligible("http://cdn.example.com/logo.png"))
        XCTAssertFalse(eligible("https://192.168.1.10/logo.png"))
        XCTAssertFalse(eligible("https://10.0.0.5/logo.png"))
        XCTAssertFalse(eligible("https://172.16.4.2/logo.png"))
        XCTAssertFalse(eligible("https://172.31.9.9/logo.png"))
        XCTAssertFalse(eligible("https://127.0.0.1/x"))
        XCTAssertFalse(eligible("https://169.254.7.7/x"))
        XCTAssertFalse(eligible("https://100.64.1.2/x"))
        XCTAssertFalse(eligible("https://[fd00::1]/x"))
        XCTAssertFalse(eligible("https://[fe80::2]/x"))
        XCTAssertFalse(eligible("https://[::1]/x"))
        // WebKit canonicalizes these legacy IPv4 spellings to 127.0.0.1.
        // The disclosure gate must classify them the same way instead of
        // presenting an approval that the exact-origin CSP later refuses.
        XCTAssertFalse(eligible("https://2130706433/x"))
        XCTAssertFalse(eligible("https://0x7f000001/x"))
        XCTAssertFalse(eligible("https://0177.0.0.1/x"))
        XCTAssertFalse(eligible("https://127.1/x"))
        XCTAssertFalse(eligible("https://127.0.1/x"))

        let summary = HistoryPreviewWebViewSVGRenderer.externalReferenceSummary(
            for: ##"<image href="https://cdn.example.com/a.png"/><image href="http://insecure.example.com/b.png"/><image href="https://10.1.2.3/c.png"/>"##
        )
        XCTAssertEqual(summary.allowedHosts, ["cdn.example.com"])
        XCTAssertEqual(Set(summary.rejectedHosts), ["insecure.example.com", "10.1.2.3"])
    }

    func testViewportSizeReadsIntrinsicDimensionsWithUnits() {
        XCTAssertEqual(
            HistoryPreviewWebViewSVGRenderer.viewportSize(
                for: #"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24"/>"#
            ),
            CGSize(width: 32, height: 24)
        )
        XCTAssertEqual(
            HistoryPreviewWebViewSVGRenderer.viewportSize(
                for: #"<svg width="96pt" height="1in"/>"#
            ),
            CGSize(width: 128, height: 96)
        )
        // Unitless and px are the same at 96 dpi.
        XCTAssertEqual(
            HistoryPreviewWebViewSVGRenderer.viewportSize(
                for: #"<svg width="10px" height="20"/>"#
            ),
            CGSize(width: 10, height: 20)
        )
    }

    func testViewportSizeIgnoresPartialRelativeAndWrongAttributes() {
        let cases = [
            #"<svg width="100" height="50%"/>"#,
            #"<svg width="10em" height="20"/>"#,
            #"<svg stroke-width="100" height="50"/>"#,
            #"<svg data-width="100" height="50"/>"#,
            #"<svg width="auto" height="auto"/>"#,
            "<svg>",
        ]
        for source in cases {
            XCTAssertEqual(
                HistoryPreviewWebViewSVGRenderer.viewportSize(for: source),
                HistoryPreviewWebSVGLimits.defaultViewport,
                "\(source) should fall back to the default viewport"
            )
        }
    }

    func testViewportSizeClampsToDimensionAndPixelBudgets() {
        let clamped = HistoryPreviewWebViewSVGRenderer.viewportSize(
            for: #"<svg width="100000" height="50000"/>"#
        )
        XCTAssertLessThanOrEqual(clamped.width, HistoryPreviewWebSVGLimits.maximumOutputDimension)
        XCTAssertLessThanOrEqual(clamped.height, HistoryPreviewWebSVGLimits.maximumOutputDimension)
        XCTAssertLessThanOrEqual(
            clamped.width * clamped.height,
            HistoryPreviewWebSVGLimits.maximumOutputPixels
        )

        // Wide-aspect input: the pixel budget, not the per-side limit, binds.
        let wide = HistoryPreviewWebViewSVGRenderer.clampedViewport(width: 8_000, height: 1_000)
        XCTAssertLessThanOrEqual(wide.width, 8_000)
        XCTAssertLessThanOrEqual(wide.height, 1_000)
        XCTAssertLessThanOrEqual(wide.width * wide.height, HistoryPreviewWebSVGLimits.maximumOutputPixels)

        // Non-positive or non-finite input gets the default viewport.
        XCTAssertEqual(
            HistoryPreviewWebViewSVGRenderer.clampedViewport(width: 0, height: 100),
            HistoryPreviewWebSVGLimits.defaultViewport
        )
    }

    /// End-to-end check of the real offscreen WKWebView snapshot.  Opt-in
    /// because WebKit needs a window-server session; plain `swift test` in CI
    /// sandboxes may not provide one.
    func testRealWebViewSnapshotRasterizesStaticSVG() async throws {
        guard ProcessInfo.processInfo.environment["TAILSYNC_WEB_SVG_RENDER_TESTS"] == "1" else {
            throw XCTSkip("set TAILSYNC_WEB_SVG_RENDER_TESTS=1 to exercise the real WKWebView path")
        }
        let renderer = HistoryPreviewWebViewSVGRenderer()
        let source = ##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#4f8cff"/></svg>"##

        let png = try await renderer.renderPNG(
            fromSVG: source,
            trustingExternalResources: false
        )
        XCTAssertTrue(png.starts(with: [137, 80, 78, 71, 13, 10, 26, 10]))
        let bitmap = try XCTUnwrap(NSBitmapImageRep(data: png))
        var pixel = [Int](repeating: 0, count: 4)
        bitmap.getPixel(&pixel, atX: 8, y: 8)
        XCTAssertEqual(pixel, [79, 140, 255, 255])
    }

    /// Regression for the reported navigation bypass: an embedded
    /// `<meta http-equiv="refresh">` must not move the page to an external
    /// origin in the default (untrusted) mode.  If the navigation were
    /// allowed, the external page would replace the document and the
    /// snapshot pixel would no longer be the rect fill.
    func testRealWebViewBlocksMetaRefreshNavigation() async throws {
        guard ProcessInfo.processInfo.environment["TAILSYNC_WEB_SVG_RENDER_TESTS"] == "1" else {
            throw XCTSkip("set TAILSYNC_WEB_SVG_RENDER_TESTS=1 to exercise the real WKWebView path")
        }
        let renderer = HistoryPreviewWebViewSVGRenderer()
        let source = ##"""
<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#4f8cff"/></svg><meta http-equiv="refresh" content="0;url=https://example.invalid/tailsync-probe">
"""##

        let png = try await renderer.renderPNG(
            fromSVG: source,
            trustingExternalResources: false
        )
        let bitmap = try XCTUnwrap(NSBitmapImageRep(data: png))
        var pixel = [Int](repeating: 0, count: 4)
        bitmap.getPixel(&pixel, atX: 8, y: 8)
        XCTAssertEqual(pixel, [79, 140, 255, 255], "the SVG document must survive a blocked meta refresh")
    }
}
