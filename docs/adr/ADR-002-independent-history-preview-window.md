# ADR-002: Independent history preview window

## Status

Accepted for implementation.

## Context

The first history-preview implementation rendered a modal overlay inside the
small history window. The overlay competed with search and filters for space,
mixed window lifecycle with history-list state, and encouraged both platform
frontends to grow large format-specific branches inside their history views.

Preview data may contain passwords, tokens, private documents, and large media.
The design therefore also needs an explicit ownership boundary for decrypted
bytes and any temporary plaintext files.

## Decision

TailSync uses one reusable, non-modal preview window per application process.
The history window sends only an entry identifier and optional batch context;
the preview window loads one bounded payload at a time from the local backend.

Responsibilities are separated as follows:

- the shared Rust core owns preview metadata, batch ordering, the 64 MiB limit,
  and stable error classification;
- each platform owns a small preview-window controller and delegates content to
  format-specific renderers;
- the history view owns selection and the established gestures: single click
  selects, Space previews, double click restores, and right click deletes;
- window-size persistence is keyed by renderer family rather than by file.

The window is not globally always-on-top. It is focused when opened, follows
the history window when that window is minimized, and closes when the history
window closes. Space and Escape close the preview unless an interactive control
currently owns the keystroke. A batch exposes previous/next navigation without
loading all batch payloads into memory.

## Security constraints

- Windows keeps preview bytes in memory and revokes Blob URLs on replacement
  and close. It does not use the clipboard materialisation path.
- macOS may materialise data only when a native Quick Look fallback requires a
  file. The directory is private (0700), files are private (0600), and cleanup
  runs on replacement, close, and application startup.
- Markdown is sanitised and cannot load remote images, media, frames, scripts,
  or styles. Links open only after an explicit user action.
- macOS SVG previews rasterize every SVG with the system browser engine inside
  a locked-down offscreen WKWebView, so a preview matches what a browser
  would render. JavaScript is disabled at the configuration level, the host
  document carries a `default-src 'none'` Content-Security-Policy that blocks
  every network subresource, the SVG is loaded with a nil base URL (unique
  origin, no file access), and the data store is non-persistent. Because CSP
  does not constrain top-level navigation, the navigation delegate only
  allows the initial `about:blank` document load and cancels every other
  navigation decision — embedded `<meta http-equiv="refresh">`, link
  activation, or form submission can never move the page to another origin.
  Each render is bounded by an 8 MiB input limit, a 4,096-pixel dimension
  limit, a 16-million-pixel output limit, and a four-second watchdog. The
  snapshot is taken in memory and the web view is destroyed immediately; SVG
  bytes are never written to a temporary file or passed to Quick Look. Any
  render failure falls back to the in-memory escaped source viewer, and a
  render in progress shows a placeholder instead of intermediate content.
  This replaces the earlier bundled Rust resvg helper, which has been removed.
- The preview may load external images and fonts only after the user
  explicitly accepts per-origin confirmation for the current entry; the choice
  resets on navigation and is off by default, and turning it on is
  transactional — the trusted flag only commits after a trusted re-render
  completes (installing the macOS snapshot or loading the Windows iframe), so
  a failed or timed-out render never leaves the UI claiming trust it did not
  deliver. A pending grant is bound to the current entry identity and payload,
  preventing a replacement entry from inheriting network access during the
  render before reset effects run. The confirmation lists the exact
  HTTPS origins (with ports) the preview would contact, requires Allow as
  the explicit second button, and refuses trust entirely when the document
  references non-HTTPS targets, embedded URL credentials, literal
  private/loopback/link-local IP hosts, or non-public ranges, including
  browser-compatible alternate IPv4 spellings and trailing-root-dot aliases
  such as `localhost.` or `printer.local.`. Trusted mode relaxes passive
  HTTPS image and font loading only, by listing the approved exact origins
  (including explicit ports) in `img-src` and `font-src` — external
  stylesheets, media, plain HTTP, blob URLs, scripts, forms, and connections
  stay blocked, and the renderer re-checks reference eligibility before
  rendering so a bypassed dialog cannot widen it.
- Both platforms share one SVG policy: a visual-eligibility gate routes
  documents with active or navigation markup (scripts, meta refresh, frames,
  SMIL or CSS animation) to the source viewer before any web view or iframe
  exists, oversized documents classify the same way, and a transient render
  failure offers a retry. The WKNavigationDelegate (macOS) and the sandboxed
  iframe (Windows) remain the actual security boundaries; the regex gate is
  a classification layer shared for cross-platform consistency. The trust
  gate, reference extractor, eligibility rules, and CSP construction are
  pinned by `shared/svg-preview-policy-fixtures.json`, which both the XCTest
  and vitest suites assert against — including byte-identical CSP output —
  so the two implementations cannot drift apart silently. The
  cross-platform sync script validates the fixture file itself.
- The two visual renderers differ by design: macOS snapshots into a bounded
  raster (4,096-pixel dimension, 16-million-pixel budget), while Windows
  displays a live sandboxed vector iframe that scales to the viewport. The
  security policy is identical on both platforms; large or deeply scaled
  documents can look different, which is an accepted trade-off rather than
  a defect.
- Oversized metadata is rejected before decryption and the decoded payload is
  checked again before it reaches a renderer.

## Format policy

- text and code use a source viewer with search, wrapping, font sizing, and
  copy-all; code adds line numbers and local syntax highlighting;
- Markdown renders only the article view;
- images use fit-to-window as the stable base for zoom and pan, with no
  separate actual-size mode that can unexpectedly reflow the viewport;
- PDF uses a controllable local reader (PDF.js on Windows and PDFKit on macOS)
  with selectable text;
- DOCX uses a local Windows renderer and the native macOS preview path;
- SVG uses the platform browser engine in a locked-down visual renderer with a
  visual/source toggle in the shared header: macOS uses an offscreen WKWebView
  snapshot, while Windows uses a sandboxed WebView2 `srcdoc` iframe. Both
  default to no external network access and fall back to the in-memory source
  viewer when rendering fails;
- unsupported formats show metadata and retain the restore action.

## Compatibility

Existing preview commands remain available while the batch-metadata and typed
error fields are added. Existing history gestures and restore/delete semantics
do not change. No database migration is required.

## Consequences

The application gains an additional window entry point and platform controller,
but history-list code becomes smaller and preview renderers can be tested in
isolation. PDF, DOCX, and syntax rendering add frontend maintenance cost, so all
resources must be bundled locally and their versions must remain pinned by the
existing package lock.

## Re-evaluation triggers

Revisit the decision if the preview size limit changes materially, if a mobile
frontend is introduced, or if native platform APIs can replace the bundled
Windows document renderers without writing plaintext files.
