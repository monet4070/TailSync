import Network
import XCTest
@testable import TailSync

/// Verifies the fail-closed property of the trusted CSP end to end: a
/// loadable reference the disclosure gate does not list must still be
/// refused by the browser.  A local NWListener acts as the undisclosed host;
/// if the trusted CSP still allowed the fetch, the probe server would
/// record a hit.
@MainActor
final class TrustedCSPFailClosedTests: XCTestCase {
    func testUndisclosedHostIsNotFetchedInTrustedMode() async throws {
        guard ProcessInfo.processInfo.environment["TAILSYNC_WEB_SVG_RENDER_TESTS"] == "1" else {
            throw XCTSkip("set TAILSYNC_WEB_SVG_RENDER_TESTS=1 to exercise the real WKWebView path")
        }
        let probe = try await ProbeServer.start()
        defer { probe.stop() }

        // The image targets the loopback probe over HTTPS.
        // A scheme-wide trusted CSP would allow the TLS connection even
        // though the trust gate refuses loopback; exact-origin CSP must not.
        let source = ##"""
<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" xmlns:xlink="http://www.w3.org/1999/xlink">
  <image href="https://127.0.0.1:\##(probe.port)/probe.png" width="24" height="24"/>
  <rect width="24" height="24" fill="#4f8cff"/>
</svg>
"""##

        // Sanity: the summary classifies the probe as rejected and lists
        // no approved origin.  Host strings carry their explicit port.
        let summary = HistoryPreviewWebViewSVGRenderer.externalReferenceSummary(for: source)
        XCTAssertTrue(summary.allowedHosts.isEmpty)
        XCTAssertTrue(summary.rejectedHosts.contains { $0.hasPrefix("127.0.0.1:") })

        let renderer = HistoryPreviewWebViewSVGRenderer()
        _ = try await renderer.renderPNG(
            fromSVG: source,
            trustingExternalResources: true
        )
        // Any in-flight fetch the CSP wrongly allowed would land here.
        try await Task.sleep(nanoseconds: 600_000_000)
        XCTAssertEqual(
            probe.hitCount,
            0,
            "trusted mode must not fetch a host the disclosure gate refused"
        )
    }
}

/// Minimal TCP probe on the loopback interface; a TLS ClientHello is already
/// enough to prove that WebKit attempted the forbidden HTTPS connection.
final class ProbeServer: @unchecked Sendable {
    private let listener: NWListener
    private let queue = DispatchQueue(label: "tailsync.svg-csp-probe")
    private let lock = NSLock()
    private var hits = 0

    private init(listener: NWListener) {
        self.listener = listener
    }

    var port: UInt16 { listener.port?.rawValue ?? 0 }
    var hitCount: Int {
        lock.lock()
        defer { lock.unlock() }
        return hits
    }

    static func start() async throws -> ProbeServer {
        let parameters = NWParameters.tcp
        parameters.allowLocalEndpointReuse = true
        let listener = try NWListener(using: parameters, on: .any)
        let probe = ProbeServer(listener: listener)
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
            let gate = ProbeStartGate()
            listener.newConnectionHandler = { connection in
                connection.start(queue: probe.queue)
                probe.recordHit()
                connection.receiveMessage { _, _, _, _ in
                    connection.send(
                        content: Data("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".utf8),
                        completion: .contentProcessed { _ in
                            connection.cancel()
                        }
                    )
                }
            }
            listener.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    gate.complete(continuation)
                case .failed(let error):
                    gate.complete(continuation, throwing: error)
                default:
                    break
                }
            }
            listener.start(queue: probe.queue)
        }
        return probe
    }

    func stop() {
        listener.cancel()
    }

    private func recordHit() {
        lock.lock()
        hits += 1
        lock.unlock()
    }
}

private final class ProbeStartGate: @unchecked Sendable {
    private let lock = NSLock()
    private var completed = false

    func complete(
        _ continuation: CheckedContinuation<Void, any Error>,
        throwing error: (any Error)? = nil
    ) {
        lock.lock()
        guard !completed else {
            lock.unlock()
            return
        }
        completed = true
        lock.unlock()
        if let error {
            continuation.resume(throwing: error)
        } else {
            continuation.resume()
        }
    }
}
