import AppKit
import Darwin
import WebKit

/// The SVG preview renderer on macOS.  Every SVG snapshot runs through the
/// system browser engine so previews match what a browser would show.
///
/// The web view is locked down before any untrusted markup is loaded:
/// JavaScript is disabled at the configuration level, the document carries a
/// Content-Security-Policy that blocks every network subresource, the SVG is
/// loaded with a nil base URL (unique origin, no file access), and each render
/// is bounded by input size, pixel count, and wall-clock time.  The view is
/// never shown or made interactive and is released right after the snapshot;
/// SVG bytes never reach disk or Quick Look.  A render failure leaves the
/// in-memory source viewer as the visible fallback.
@MainActor
protocol HistoryPreviewWebSVGRendering: AnyObject {
    /// Rasterize `source` with the system browser engine and return PNG bytes.
    /// `trustingExternalResources` opts the render into loading network
    /// images and fonts referenced by the SVG; script execution and external
    /// stylesheets stay disabled either way.
    func renderPNG(
        fromSVG source: String,
        trustingExternalResources: Bool
    ) async throws -> Data
    /// Tear down the active render, if any.
    func cancel()
}

enum HistoryPreviewWebSVGLimits {
    static let maximumInputBytes = 8 * 1024 * 1024
    static let maximumOutputDimension: CGFloat = 4_096
    static let maximumOutputPixels: CGFloat = 16 * 1024 * 1024
    static let timeout: TimeInterval = 4
    /// Extra time for the compositor to paint after navigation completes.
    static let paintSettleInterval: UInt64 = 100_000_000
    /// Viewport used when the root element has no usable width/height, which
    /// matches how a browser would fill an ordinary window.
    static let defaultViewport = CGSize(width: 1_024, height: 768)
}

enum HistoryPreviewWebSVGRendererError: LocalizedError, Equatable {
    case inputTooLarge
    case loadFailed
    case timedOut
    case renderFailed

    var errorDescription: String? {
        switch self {
        case .inputTooLarge: return "SVG input exceeds the preview limit"
        case .loadFailed: return "SVG could not be loaded in the browser engine"
        case .timedOut: return "SVG preview exceeded the time limit"
        case .renderFailed: return "SVG could not be rasterized"
        }
    }
}

@MainActor
final class HistoryPreviewWebViewSVGRenderer: HistoryPreviewWebSVGRendering {
    private var active: ActiveRender?

    // Default arguments in the view model's initializer are evaluated in a
    // nonisolated context; constructing the renderer allocates nothing
    // main-actor-specific, so the initializer itself stays nonisolated.
    nonisolated init() {}

    func renderPNG(
        fromSVG source: String,
        trustingExternalResources: Bool
    ) async throws -> Data {
        guard source.utf8.count <= HistoryPreviewWebSVGLimits.maximumInputBytes else {
            throw HistoryPreviewWebSVGRendererError.inputTooLarge
        }
        cancel()

        let render = ActiveRender(
            frame: Self.viewportSize(for: source)
        )
        active = render
        let watchdog = Task { [weak render] in
            try? await Task.sleep(nanoseconds: UInt64(HistoryPreviewWebSVGLimits.timeout * 1_000_000_000))
            guard !Task.isCancelled else { return }
            render?.handleTimeout()
        }
        defer {
            watchdog.cancel()
            if active === render {
                active = nil
            }
            render.tearDown()
        }

        do {
            try await render.load(
                html: Self.documentHTML(
                    for: source,
                    trustingExternalResources: trustingExternalResources
                )
            )
            try await Task.sleep(nanoseconds: HistoryPreviewWebSVGLimits.paintSettleInterval)
            let image = try await render.snapshot()
            return try Self.pngData(from: image)
        } catch let error as HistoryPreviewWebSVGRendererError {
            throw error
        } catch is CancellationError {
            throw CancellationError()
        } catch {
            // WebKit's detailed failures are deliberately collapsed: the UI
            // only needs to know that this render attempt did not produce a
            // snapshot.
            throw HistoryPreviewWebSVGRendererError.loadFailed
        }
    }

    func cancel() {
        active?.tearDown()
        active = nil
    }

    // MARK: - Pure helpers (unit tested)

    /// Wraps the SVG in a minimal host document.  The default CSP forbids
    /// every network subresource: no external images, fonts, stylesheets,
    /// frames, media, or connections.  Inline styles and `data:` images/fonts
    /// stay available so design-tool output keeps its appearance without any
    /// network access.  A second CSP injected through the markup itself can
    /// only add restrictions, never remove these.  Top-level navigation is
    /// not governed by CSP at all; the navigation delegate blocks it.
    ///
    /// Trusted mode enumerates the exact HTTPS origins the trust gate
    /// disclosed — `img-src data: https://host1 https://host2:8443 …` — instead
    /// of a scheme-wide `https:` source, so a reference form the extractor
    /// misses cannot fetch anything: the browser refuses hosts that were
    /// never listed.  Trust may only relax passive image and font loading;
    /// scripts, forms, and connections stay disabled in every mode, and the
    /// render keeps its input, pixel, and wall-clock budgets.
    static func documentHTML(
        for source: String,
        trustingExternalResources: Bool
    ) -> String {
        let contentSecurityPolicy: String
        if trustingExternalResources {
            var seenSources = Set<String>()
            let hostSources = externalReferences(in: source)
                .filter(isTrustEligibleURL)
                .compactMap(trustedCSPSource)
                .filter { seenSources.insert($0).inserted }
                .joined(separator: " ")
            let hostTail = hostSources.isEmpty ? "" : " \(hostSources)"
            contentSecurityPolicy = "default-src 'none'; style-src 'unsafe-inline'; img-src data:\(hostTail); font-src data:\(hostTail); script-src 'none'; base-uri 'none'; form-action 'none'"
        } else {
            contentSecurityPolicy = "default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; media-src 'none'; script-src 'none'; base-uri 'none'; form-action 'none'"
        }
        return """
        <!DOCTYPE html><html><head><meta charset="utf-8">\
        <meta http-equiv="Content-Security-Policy" content="\(contentSecurityPolicy)">\
        <style>html,body{margin:0;padding:0;background:transparent;overflow:hidden}</style>\
        </head><body>\(source)</body></html>
        """
    }

    // MARK: - External reference extraction (trust gate)

    /// Summary of the external references an SVG carries, used both for the
    /// user-facing trust disclosure and for the eligibility check.
    struct ExternalReferenceSummary: Equatable {
        /// Distinct hosts that trust would allow (HTTPS, public literal IPs
        /// or hostnames).
        var allowedHosts: [String] = []
        /// Distinct hosts that trust must refuse: non-HTTPS schemes,
        /// literal private/loopback/link-local IP hosts, or unparsable
        /// targets.
        var rejectedHosts: [String] = []
    }

    /// Collects every external http(s) URL the document references through
    /// `href`/`xlink:href`/`src`/`srcset` attributes or CSS `url(...)`
    /// values, after HTML-entity decoding (WebKit resolves `&#58;`-style
    /// escapes in attribute values, so the extractor must too).  `srcset`
    /// candidates are split on commas and each URL token is inspected.  Data
    /// URLs, fragments, and relative references stay local and are ignored.
    /// This list drives both the disclosure dialog and the trusted CSP origin
    /// enumeration, so under-extraction cannot widen network access — only
    /// disclosure accuracy depends on it.
    static func externalReferences(in source: String) -> [URL] {
        var targets: [String] = []
        let attributePattern = #"(?:href|xlink:href|src|srcset)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#
        let cssPattern = #"url\(\s*(?:'([^']*)'|"([^"]*)"|([^)]*))"#
        for pattern in [attributePattern, cssPattern] {
            guard let regex = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive]) else {
                continue
            }
            let range = NSRange(source.startIndex..., in: source)
            for match in regex.matches(in: source, range: range) {
                for group in 1..<regex.numberOfCaptureGroups + 1 {
                    guard let groupRange = Range(match.range(at: group), in: source) else {
                        continue
                    }
                    let raw = String(source[groupRange])
                    if pattern == attributePattern {
                        // srcset values are comma-separated `URL descriptor`
                        // candidates; every URL token is a loadable target.
                        for candidate in raw.split(separator: ",") {
                            if let urlToken = candidate.split(whereSeparator: { $0.isWhitespace }).first {
                                targets.append(String(urlToken))
                            }
                        }
                    } else {
                        targets.append(raw)
                    }
                }
            }
        }
        var seen = Set<String>()
        var urls: [URL] = []
        for target in targets.map({ decodeHTMLEntities($0).trimmingCharacters(in: .whitespacesAndNewlines) }) {
            guard let url = URL(string: target),
                  let scheme = url.scheme?.lowercased(),
                  scheme == "http" || scheme == "https",
                  let host = url.host?.lowercased(),
                  !host.isEmpty,
                  seen.insert("\(scheme)://\(host):\(url.port.map(String.init) ?? "default")").inserted else {
                continue
            }
            urls.append(url)
        }
        return urls
    }

    /// Decodes the HTML escape forms WebKit resolves inside attribute
    /// values: numeric (`&#58;`, `&#x3a;`) and a small set of named entities.
    /// Unrecognized sequences are copied verbatim.
    private static func decodeHTMLEntities(_ value: String) -> String {
        guard value.contains("&") else { return value }
        var result = ""
        var index = value.startIndex
        while index < value.endIndex {
            guard value[index] == "&",
                  let semicolon = value[index...].firstIndex(of: ";"),
                  value.distance(from: index, to: semicolon) <= 12 else {
                result.append(value[index])
                index = value.index(after: index)
                continue
            }
            let body = value[value.index(after: index)..<semicolon]
            if body.hasPrefix("#") {
                let digits = body.dropFirst()
                let scalar: UInt32?
                if digits.hasPrefix("x") || digits.hasPrefix("X") {
                    scalar = UInt32(digits.dropFirst(), radix: 16)
                } else {
                    scalar = UInt32(digits)
                }
                if let scalar, let unicode = Unicode.Scalar(scalar) {
                    result.unicodeScalars.append(unicode)
                    index = value.index(after: semicolon)
                    continue
                }
            } else {
                let named: [String: String] = [
                    "amp": "&", "lt": "<", "gt": ">", "quot": "\"", "apos": "'",
                    "colon": ":", "semi": ";", "sol": "/", "bsol": "\\",
                    "period": ".", "commat": "@", "num": "#", "equals": "=",
                ]
                if let replacement = named[String(body)] {
                    result.append(contentsOf: replacement)
                    index = value.index(after: semicolon)
                    continue
                }
            }
            result.append(value[index])
            index = value.index(after: index)
        }
        return result
    }

    /// Trust eligibility for one external URL: HTTPS scheme and a
    /// non-private host.  Literal IP hosts are checked against private,
    /// loopback, and link-local ranges; the `localhost` and `.local`/
    /// `.localhost` hostnames are refused as well.  Other plain hostnames
    /// are accepted — resolving them ahead of the render would mean
    /// performing DNS for untrusted input, which is its own request leak.
    static func isTrustEligibleURL(_ url: URL) -> Bool {
        guard url.scheme?.lowercased() == "https",
              let host = url.host?.lowercased() else {
            return false
        }
        return !isPrivateLiteralHost(host)
    }

    static func externalReferenceSummary(for source: String) -> ExternalReferenceSummary {
        var summary = ExternalReferenceSummary()
        for url in externalReferences(in: source) {
            let host = (url.host?.lowercased()) ?? "(invalid)"
            if isTrustEligibleURL(url) {
                if !summary.allowedHosts.contains(host) {
                    summary.allowedHosts.append(host)
                }
            } else if !summary.rejectedHosts.contains(host) {
                summary.rejectedHosts.append(host)
            }
        }
        return summary
    }

    /// A CSP host-source is an origin-shaped value.  Preserve an explicit
    /// non-default port and bracket IPv6 literals so the trusted policy
    /// grants exactly the endpoint the SVG disclosed.
    private static func trustedCSPSource(for url: URL) -> String? {
        guard url.scheme?.lowercased() == "https",
              let host = url.host?.lowercased(),
              !host.isEmpty else {
            return nil
        }
        let cspHost = host.contains(":") ? "[\(host)]" : host
        if let port = url.port {
            return "https://\(cspHost):\(port)"
        }
        return "https://\(cspHost)"
    }

    /// Literal-IP check for private, loopback, link-local, and other
    /// non-public ranges, applied per host, plus the `localhost` and
    /// `.local`/`.localhost` hostname forms.  IPv6 checks the loopback,
    /// unique-local (fc00::/7), and link-local (fe80::/10) prefixes plus
    /// IPv4-mapped addresses.
    private static func isPrivateLiteralHost(_ host: String) -> Bool {
        let normalizedHost = host.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: "."))
        if normalizedHost == "localhost"
            || normalizedHost.hasSuffix(".localhost")
            || normalizedHost.hasSuffix(".local") {
            return true
        }
        if let ipv4 = parseBrowserIPv4(normalizedHost) {
            return isNonPublicIPv4(ipv4)
        }
        if let bytes = parseIPv6(normalizedHost) {
            if bytes.allSatisfy({ $0 == 0 }) { return true }
            if bytes.dropLast().allSatisfy({ $0 == 0 }) && bytes.last == 1 { return true }
            if bytes[0] & 0xFE == 0xFC { return true } // fc00::/7 unique-local
            if bytes[0] == 0xFE && bytes[1] & 0xC0 == 0x80 { return true } // fe80::/10 link-local
            if bytes[0] == 0xFF { return true } // multicast
            if bytes[0...3] == [0x20, 0x01, 0x0D, 0xB8] { return true } // documentation
            if bytes[0..<10].allSatisfy({ $0 == 0 }), bytes[10] == 0xFF, bytes[11] == 0xFF {
                let mapped = UInt32(bytes[12]) << 24
                    | UInt32(bytes[13]) << 16
                    | UInt32(bytes[14]) << 8
                    | UInt32(bytes[15])
                return isNonPublicIPv4(mapped)
            }
        }
        return false
    }

    /// Parses the legacy IPv4 number forms WebKit accepts (one to four
    /// decimal/octal/hex components).  For example, `2130706433`,
    /// `0x7f000001`, `0177.0.0.1`, and `127.1` all mean 127.0.0.1 in the
    /// browser and therefore must receive the same trust classification.
    private static func parseBrowserIPv4(_ host: String) -> UInt32? {
        let parts = host.split(separator: ".", omittingEmptySubsequences: false)
        guard (1...4).contains(parts.count), parts.allSatisfy({ !$0.isEmpty }) else {
            return nil
        }
        var numbers: [UInt64] = []
        for part in parts {
            let text = String(part)
            let radix: Int
            let digits: Substring
            if text.hasPrefix("0x") || text.hasPrefix("0X") {
                radix = 16
                digits = text.dropFirst(2)
            } else if text.count > 1 && text.hasPrefix("0") {
                radix = 8
                digits = text.dropFirst()
            } else {
                radix = 10
                digits = Substring(text)
            }
            let value: UInt64
            if digits.isEmpty && radix == 8 {
                value = 0
            } else if let parsed = UInt64(digits, radix: radix) {
                value = parsed
            } else {
                return nil
            }
            numbers.append(value)
        }
        for number in numbers.dropLast() where number > 255 {
            return nil
        }
        let lastBitCount = 8 * (5 - numbers.count)
        let lastLimit = UInt64(1) << lastBitCount
        guard let last = numbers.last, last < lastLimit else { return nil }

        var address = last
        for (index, number) in numbers.dropLast().enumerated() {
            address += number << (8 * (3 - index))
        }
        guard address <= UInt64(UInt32.max) else { return nil }
        return UInt32(address)
    }

    private static func isNonPublicIPv4(_ address: UInt32) -> Bool {
        let a = UInt8((address >> 24) & 0xFF)
        let b = UInt8((address >> 16) & 0xFF)
        let c = UInt8((address >> 8) & 0xFF)
        if a == 0 || a == 10 || a == 127 { return true }
        if a == 100 && (64...127).contains(b) { return true }
        if a == 169 && b == 254 { return true }
        if a == 172 && (16...31).contains(b) { return true }
        if a == 192 && b == 168 { return true }
        if a == 192 && b == 0 && c == 0 { return true }
        if a == 192 && b == 0 && c == 2 { return true }
        if a == 198 && (b == 18 || b == 19) { return true }
        if a == 198 && b == 51 && c == 100 { return true }
        if a == 203 && b == 0 && c == 113 { return true }
        return a >= 224
    }

    private static func parseIPv6(_ host: String) -> [UInt8]? {
        var address = in6_addr()
        let result = host.withCString { pointer in
            inet_pton(AF_INET6, pointer, &address)
        }
        guard result == 1 else { return nil }
        return withUnsafeBytes(of: &address) { Array($0) }
    }

    /// Pixel size for the offscreen web view.  Reads the root element's
    /// `width`/`height` presentation attributes (CSS-unit aware) so the
    /// snapshot matches the intrinsic document size, and clamps the result
    /// to the preview dimension and pixel budgets.  SVGs without usable
    /// dimensions get the default viewport.
    static func viewportSize(for source: String) -> CGSize {
        guard let tag = firstSVGTag(in: source) else {
            return HistoryPreviewWebSVGLimits.defaultViewport
        }
        let width = dimensionValue(in: tag, attribute: "width")
        let height = dimensionValue(in: tag, attribute: "height")
        guard let width, let height else {
            return HistoryPreviewWebSVGLimits.defaultViewport
        }
        return clampedViewport(width: width, height: height)
    }

    static func clampedViewport(width: CGFloat, height: CGFloat) -> CGSize {
        guard width > 0, height > 0, width.isFinite, height.isFinite else {
            return HistoryPreviewWebSVGLimits.defaultViewport
        }
        var scale: CGFloat = 1
        scale = min(scale, HistoryPreviewWebSVGLimits.maximumOutputDimension / width)
        scale = min(scale, HistoryPreviewWebSVGLimits.maximumOutputDimension / height)
        let pixels = width * height
        if pixels > HistoryPreviewWebSVGLimits.maximumOutputPixels {
            scale = min(
                scale,
                (HistoryPreviewWebSVGLimits.maximumOutputPixels / pixels).squareRoot()
            )
        }
        guard scale > 0, scale.isFinite else {
            return HistoryPreviewWebSVGLimits.defaultViewport
        }
        return CGSize(width: max(1, width * scale), height: max(1, height * scale))
    }

    private static func firstSVGTag(in source: String) -> Substring? {
        guard let range = source.range(of: "<svg", options: .caseInsensitive) else {
            return nil
        }
        let rest = source[range.lowerBound...]
        guard let end = rest.range(of: ">") else {
            return nil
        }
        return rest[..<end.lowerBound]
    }

    /// Parses a leading number plus CSS length unit into pixels.  Returns nil
    /// for percentages and relative units (em/ex) — they depend on context the
    /// snapshot does not have.
    private static func dimensionValue(in tag: Substring, attribute: String) -> CGFloat? {
        guard let match = attributeValue(in: tag, attribute: attribute) else {
            return nil
        }
        return cssLengthInPixels(match.trimmingCharacters(in: .whitespaces))
    }

    /// Reads one attribute from an opening tag.  Tokenizes instead of
    /// substring-matching so `stroke-width` or `data-width` can never satisfy
    /// a lookup for `width`.
    private static func attributeValue(in tag: Substring, attribute: String) -> String? {
        var text = tag
        // Skip past the element name to the first attribute.
        guard let nameEnd = text.firstIndex(where: { $0.isWhitespace }) else {
            return nil
        }
        text = text[nameEnd...]
        var index = text.startIndex
        while index < text.endIndex {
            index = skipWhitespace(in: text, from: index)
            guard index < text.endIndex else { break }
            let attributeNameStart = index
            while index < text.endIndex, text[index] != "=", !text[index].isWhitespace {
                index = text.index(after: index)
            }
            let attributeName = String(text[attributeNameStart..<index])
            index = skipWhitespace(in: text, from: index)
            guard index < text.endIndex, text[index] == "=" else { continue }
            index = text.index(after: index)
            index = skipWhitespace(in: text, from: index)
            guard index < text.endIndex else { break }
            if let value = scannedValue(in: text, from: &index),
               attributeName.caseInsensitiveCompare(attribute) == .orderedSame {
                return value
            }
        }
        return nil
    }

    private static func skipWhitespace(in text: Substring, from index: Substring.Index) -> Substring.Index {
        var index = index
        while index < text.endIndex, text[index].isWhitespace {
            index = text.index(after: index)
        }
        return index
    }

    private static func scannedValue(
        in text: Substring,
        from index: inout Substring.Index
    ) -> String? {
        let quote = text[index]
        guard quote == "\"" || quote == "'" else {
            let start = index
            while index < text.endIndex, !text[index].isWhitespace {
                index = text.index(after: index)
            }
            return String(text[start..<index])
        }
        index = text.index(after: index)
        let start = index
        while index < text.endIndex, text[index] != quote {
            index = text.index(after: index)
        }
        let value = String(text[start..<index])
        if index < text.endIndex {
            index = text.index(after: index)
        }
        return value
    }

    private static func cssLengthInPixels(_ value: String) -> CGFloat? {
        let normalized = value.trimmingCharacters(in: .whitespaces)
        let characters = normalized[...]
        guard let unitStart = characters.firstIndex(where: { character in
            !character.isNumber
                && character != "."
                && character != "-"
                && character != "+"
        }) else {
            return pixels(numberPart: normalized, unit: "")
        }
        let numberPart = String(characters[..<unitStart])
        let unit = String(characters[unitStart...])
        return pixels(numberPart: numberPart, unit: unit)
    }

    private static func pixels(numberPart: String, unit rawUnit: String) -> CGFloat? {
        guard let number = Double(numberPart), number.isFinite, number > 0 else {
            return nil
        }
        let pointsPerInch = 96.0
        let unit = rawUnit.trimmingCharacters(in: .whitespaces).lowercased()
        switch unit {
        case "px", "": return number
        case "pt": return number * pointsPerInch / 72
        case "pc": return number * pointsPerInch / 6
        case "in": return number * pointsPerInch
        case "cm": return number * pointsPerInch / 2.54
        case "mm": return number * pointsPerInch / 25.4
        case "q": return number * pointsPerInch / 101.6
        default: return nil
        }
    }

    private static func pngData(from image: NSImage) throws -> Data {
        guard let tiff = image.tiffRepresentation,
              let bitmap = NSBitmapImageRep(data: tiff),
              let png = bitmap.representation(using: .png, properties: [:]),
              !png.isEmpty else {
            throw HistoryPreviewWebSVGRendererError.renderFailed
        }
        return png
    }
}

/// One offscreen, fully locked-down web view.  Main-thread confined: created,
/// driven, and torn down from the main actor; WebKit delivers its delegate
/// and snapshot callbacks on the main thread as well.
private final class ActiveRender: NSObject, WKNavigationDelegate {
    let webView: WKWebView
    private(set) var timedOut = false
    private var navigationContinuation: CheckedContinuation<Void, Error>?
    private var snapshotContinuation: CheckedContinuation<NSImage, Error>?

    init(frame: CGSize) {
        let configuration = WKWebViewConfiguration()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = false
        configuration.websiteDataStore = .nonPersistent()
        webView = WKWebView(frame: NSRect(origin: .zero, size: frame), configuration: configuration)
        super.init()
        webView.navigationDelegate = self
    }

    func load(html: String) async throws {
        try await withCheckedThrowingContinuation { continuation in
            navigationContinuation = continuation
            // nil base URL gives the document a unique origin, so it cannot
            // reach the file system or any other loaded content.
            webView.loadHTMLString(html, baseURL: nil)
        }
    }

    func snapshot() async throws -> NSImage {
        try await withCheckedThrowingContinuation { continuation in
            snapshotContinuation = continuation
            let configuration = WKSnapshotConfiguration()
            configuration.rect = webView.bounds
            webView.takeSnapshot(with: configuration) { image, error in
                self.finishSnapshot(image: image, error: error)
            }
        }
    }

    /// Watchdog callback: abandon the render instead of waiting on a wedged
    /// web content process.
    func handleTimeout() {
        timedOut = true
        webView.stopLoading()
        navigationContinuation?.resume(throwing: HistoryPreviewWebSVGRendererError.timedOut)
        navigationContinuation = nil
        snapshotContinuation?.resume(throwing: HistoryPreviewWebSVGRendererError.timedOut)
        snapshotContinuation = nil
    }

    func tearDown() {
        webView.stopLoading()
        webView.navigationDelegate = nil
        navigationContinuation?.resume(throwing: CancellationError())
        navigationContinuation = nil
        snapshotContinuation?.resume(throwing: CancellationError())
        snapshotContinuation = nil
    }

    private func finishSnapshot(image: NSImage?, error: Error?) {
        guard let continuation = snapshotContinuation else { return }
        snapshotContinuation = nil
        if let image {
            continuation.resume(returning: image)
        } else {
            continuation.resume(throwing: error ?? HistoryPreviewWebSVGRendererError.renderFailed)
        }
    }

    // MARK: - WKNavigationDelegate (called on the main thread)

    /// The only navigation ever permitted is the initial `loadHTMLString`
    /// document, whose request URL is `about:blank`.  Everything else is
    /// cancelled: CSP `default-src 'none'` deliberately does not constrain
    /// top-level navigation, so an embedded `<meta http-equiv="refresh">`,
    /// link activation, or form submission could otherwise move the page to
    /// an external origin — leaking the user's IP and replacing the
    /// snapshot with third-party content.  This policy holds in both the
    /// default and the trusted-external-resources mode.
    func webView(
        _ webView: WKWebView,
        decidePolicyFor navigationAction: WKNavigationAction,
        decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
    ) {
        if navigationAction.targetFrame?.isMainFrame == true,
           navigationAction.request.url?.absoluteString == "about:blank" {
            decisionHandler(.allow)
        } else {
            decisionHandler(.cancel)
        }
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        navigationContinuation?.resume()
        navigationContinuation = nil
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        navigationContinuation?.resume(throwing: error)
        navigationContinuation = nil
    }

    func webView(
        _ webView: WKWebView,
        didFailProvisionalNavigation navigation: WKNavigation!,
        withError error: Error
    ) {
        navigationContinuation?.resume(throwing: error)
        navigationContinuation = nil
    }
}
