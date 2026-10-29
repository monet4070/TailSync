import AppKit
import WebKit

enum HistoryMarkdownResources {
    static let maximumSourceBytes = 256 * 1024
    static let engine = load("engine", extension: "js")
    static let styles = load("preview", extension: "css")

    private static func load(_ name: String, extension ext: String) -> String {
        guard let url = Bundle.module.url(forResource: name, withExtension: ext, subdirectory: "Markdown") else { return "" }
        return (try? String(contentsOf: url, encoding: .utf8)) ?? ""
    }

    static var shell: String {
        """
        <!doctype html><html><head><meta charset="utf-8">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; font-src data:; img-src 'none'; connect-src 'none'; base-uri 'none'; form-action 'none'">
        <style>html,body{margin:0;min-height:100%;}body{overflow-x:hidden;}\(styles)</style>
        </head><body><div id="notice" class="md-notice" role="status" hidden></div>
        <article id="article" class="md-article"></article></body></html>
        """
    }
}

/// A persistent view per preview. Source parsing is confined to WebKit's
/// content process; font/theme updates do not reload or parse the document.
@MainActor
final class HistoryMarkdownWebRenderer: NSObject, WKNavigationDelegate {
    let webView: WKWebView
    private var loaded = false
    private var disposed = false
    private var source: String?
    private var appearance: [String: Any] = [:]
    private var sourceChanged = false
    private var revision = 0
    private(set) var lastRender: [String: Any]?

    override init() {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = false
        configuration.userContentController.addUserScript(WKUserScript(
            source: HistoryMarkdownResources.engine,
            injectionTime: .atDocumentEnd, forMainFrameOnly: true,
            in: .defaultClient
        ))
        webView = WKWebView(frame: .zero, configuration: configuration)
        super.init()
        webView.navigationDelegate = self
        webView.setValue(false, forKey: "drawsBackground")
        webView.loadHTMLString(HistoryMarkdownResources.shell, baseURL: nil)
    }

    func update(source: String, appearance: [String: Any]) {
        guard !disposed else { return }
        sourceChanged = sourceChanged || self.source != source
        self.source = source
        self.appearance = appearance
        if loaded { apply() }
    }

    private func apply() {
        guard !disposed, let source else { return }
        if !sourceChanged {
            webView.callAsyncJavaScript("TailSyncMarkdown.setAppearance(appearance)",
                arguments: ["appearance": appearance], in: nil, in: .defaultClient)
            return
        }
        sourceChanged = false
        webView.isHidden = true
        revision += 1
        let expected = revision
        lastRender = nil
        let bounded = String(decoding: source.utf8.prefix(HistoryMarkdownResources.maximumSourceBytes), as: UTF8.self)
        webView.callAsyncJavaScript("return TailSyncMarkdown.render(source, appearance, truncated)",
            arguments: ["source": bounded, "appearance": appearance,
                        "truncated": source.utf8.count > HistoryMarkdownResources.maximumSourceBytes],
            in: nil, in: .defaultClient) { [weak self] result in
                guard let self, !self.disposed, self.revision == expected else { return }
                switch result {
                case .success(let result):
                    self.lastRender = result as? [String: Any]
                    self.webView.isHidden = false
                case .failure:
                    // Preserve readable content if WebKit cannot initialize.
                    self.webView.callAsyncJavaScript("document.getElementById('article').textContent = source",
                        arguments: ["source": bounded], in: nil, in: .defaultClient) { [weak self] _ in
                            guard let self, !self.disposed, self.revision == expected else { return }
                            self.webView.isHidden = false
                        }
                }
            }
    }

    func cancel() {
        disposed = true; revision += 1; source = nil
        webView.stopLoading(); webView.navigationDelegate = nil
        webView.configuration.userContentController.removeAllUserScripts()
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        loaded = true; apply()
    }

    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        if !loaded, action.request.url?.absoluteString == "about:blank", action.targetFrame?.isMainFrame == true {
            decisionHandler(.allow); return
        }
        if action.navigationType == .linkActivated, let url = action.request.url,
           Self.isAllowedLink(url) { NSWorkspace.shared.open(url) }
        decisionHandler(.cancel)
    }

    static func isAllowedLink(_ url: URL) -> Bool {
        ["http", "https"].contains(url.scheme?.lowercased() ?? "")
    }

    static func appearance(palette: TailSyncThemePalette, fontSize: Double) -> [String: Any] {
        func css(_ color: NSColor) -> String {
            let rgb = color.usingColorSpace(.deviceRGB) ?? .labelColor
            return "rgba(\(Int(rgb.redComponent * 255)),\(Int(rgb.greenComponent * 255)),\(Int(rgb.blueComponent * 255)),\(rgb.alphaComponent))"
        }
        return ["fontSize": HistoryPreviewPreferences.clampedTextFontSize(fontSize),
                "dark": ((palette.surface >> 16) & 255) < 128,
                "language": Loc.shared.lang,
                "truncatedMessage": Loc.t("history.preview.truncated"),
                "colors": ["--md-text": css(NSColor(palette.primaryColor)),
                           "--md-muted": css(NSColor(palette.secondaryColor)),
                           "--md-background": css(NSColor(palette.surfaceColor)),
                           "--md-soft": css(NSColor(palette.softSurfaceColor)),
                           "--md-stripe": css(NSColor(palette.softSurfaceColor)),
                           "--md-border": css(NSColor(palette.borderColor)),
                           "--md-accent": css(NSColor(palette.accentColor))]]
    }
}
