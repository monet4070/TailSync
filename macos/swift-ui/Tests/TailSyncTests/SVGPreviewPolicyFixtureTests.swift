import Foundation
import XCTest
@testable import TailSync

/// Shared SVG preview policy fixtures: the same JSON drives the Windows
/// (vitest) and macOS (XCTest) suites, so the two implementations of the
/// trust gate, reference extractor, visual eligibility gate, and CSP
/// construction cannot drift apart silently.  The fixture file lives in
/// `shared/` at the repository root; tests locate it from `#filePath`
/// because the test runner's current directory is not stable and SwiftPM
/// does not package resources outside the package root.
@MainActor
final class SVGPreviewPolicyFixtureTests: XCTestCase {
    private struct TrustEligibilityFixture: Decodable {
        let url: String
        let eligible: Bool
    }

    private struct ReferenceExtractionFixture: Decodable {
        let source: String
        let allowedHosts: [String]
        let allowedOrigins: [String]
        let rejectedHosts: [String]
    }

    private struct VisualEligibilityFixture: Decodable {
        let source: String
        let eligible: Bool
    }

    private struct CSPFixture: Decodable {
        let source: String
        let trusted: Bool
        let expectedCSP: String
    }

    private struct FixtureFile: Decodable {
        let trustEligibility: [TrustEligibilityFixture]
        let referenceExtraction: [ReferenceExtractionFixture]
        let visualEligibility: [VisualEligibilityFixture]
        let csp: [CSPFixture]
    }

    private func loadFixtures() throws -> FixtureFile {
        // .../macos/swift-ui/Tests/TailSyncTests/<file> → repository root.
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<5 {
            url.deleteLastPathComponent()
        }
        url.appendPathComponent("shared/svg-preview-policy-fixtures.json")
        let data = try Data(contentsOf: url)
        return try JSONDecoder().decode(FixtureFile.self, from: data)
    }

    private func cspFromDocument(_ html: String) throws -> String {
        let marker = "Content-Security-Policy\" content=\""
        guard let range = html.range(of: marker) else {
            throw XCTSkip("document has no CSP header")
        }
        let remainder = html[range.upperBound...]
        return String(remainder.prefix(while: { $0 != "\"" }))
    }

    func testTrustEligibilityMatchesSharedFixtures() throws {
        for fixture in try loadFixtures().trustEligibility {
            guard let url = URL(string: fixture.url) else {
                return XCTFail("fixture URL does not parse: \(fixture.url)")
            }
            XCTAssertEqual(
                HistoryPreviewWebViewSVGRenderer.isTrustEligibleURL(url),
                fixture.eligible,
                "\(fixture.url) should be eligible=\(fixture.eligible)"
            )
        }
    }

    func testReferenceExtractionMatchesSharedFixtures() throws {
        for fixture in try loadFixtures().referenceExtraction {
            let summary = HistoryPreviewWebViewSVGRenderer.externalReferenceSummary(
                for: fixture.source
            )
            XCTAssertEqual(summary.allowedHosts, fixture.allowedHosts, fixture.source)
            XCTAssertEqual(summary.allowedOrigins, fixture.allowedOrigins, fixture.source)
            XCTAssertEqual(summary.rejectedHosts, fixture.rejectedHosts, fixture.source)
        }
    }

    func testVisualEligibilityMatchesSharedFixtures() throws {
        for fixture in try loadFixtures().visualEligibility {
            XCTAssertEqual(
                HistoryPreviewWebViewSVGRenderer.isSVGVisualEligible(fixture.source),
                fixture.eligible,
                fixture.source
            )
        }
    }

    /// Byte-identical CSP: both platforms construct the policy from the same
    /// directive list and the same disclosed origins, so the emitted header
    /// must match exactly.  This is the drift tripwire — a directive rename
    /// or ordering change on one platform fails here.
    func testCSPMatchesSharedFixturesExactly() throws {
        for fixture in try loadFixtures().csp {
            let html = HistoryPreviewWebViewSVGRenderer.documentHTML(
                for: fixture.source,
                trustingExternalResources: fixture.trusted
            )
            let policy = try cspFromDocument(html)
            XCTAssertEqual(policy, fixture.expectedCSP, fixture.source)
        }
    }

    /// Directive-dictionary assertions for both modes: passive loading is
    /// the only thing trust relaxes, every active capability stays 'none',
    /// and an explicit port never widens into the whole host.
    func testCSPDirectiveDictionaryPolicy() {
        let blockedEverywhere: [(String, String)] = [
            ("default-src", "'none'"),
            ("media-src", "'none'"),
            ("object-src", "'none'"),
            ("frame-src", "'none'"),
            ("child-src", "'none'"),
            ("worker-src", "'none'"),
            ("connect-src", "'none'"),
            ("manifest-src", "'none'"),
            ("script-src", "'none'"),
            ("navigate-to", "'none'"),
            ("base-uri", "'none'"),
            ("form-action", "'none'"),
        ]
        for trusted in [false, true] {
            let html = HistoryPreviewWebViewSVGRenderer.documentHTML(
                for: #"<svg><image href="https://cdn.example.com:8443/a.png"/></svg>"#,
                trustingExternalResources: trusted
            )
            let policy = (try? cspFromDocument(html)) ?? ""
            var directives: [String: String] = [:]
            for statement in policy.split(separator: ";") {
                let parts = statement.split(
                    separator: " ",
                    maxSplits: 1,
                    omittingEmptySubsequences: true
                )
                .map { $0.trimmingCharacters(in: .whitespaces) }
                if parts.count == 2 {
                    directives[parts[0]] = parts[1]
                }
            }
            for (directive, value) in blockedEverywhere {
                XCTAssertEqual(directives[directive], value, "\(directive) in \(trusted ? "trusted" : "default") mode")
            }
            XCTAssertEqual(directives["style-src"], "'unsafe-inline'")
            if trusted {
                XCTAssertEqual(
                    directives["img-src"],
                    "data: https://cdn.example.com:8443"
                )
                XCTAssertEqual(
                    directives["font-src"],
                    "data: https://cdn.example.com:8443"
                )
            } else {
                XCTAssertEqual(directives["img-src"], "data:")
                XCTAssertEqual(directives["font-src"], "data:")
            }
        }
    }
}
