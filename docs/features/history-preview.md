# History preview

TailSync provides a reusable, independent preview window for clipboard history.
The history list remains available while the preview is open, so search,
filters, pagination, restore, and delete are never covered by preview content.

## Interaction

- single click selects a history row;
- Space opens the selected row in the single preview window;
- selecting another row and pressing Space replaces the current preview;
- Space or Escape closes the preview when no interactive control owns the key;
- double click restores the entry to the clipboard;
- right click deletes the entry;
- Alt+Left and Alt+Right navigate files in the same batch.
- Control+wheel on Windows or Command/Control+wheel on macOS changes the text
  font size or zooms the image/PDF under the pointer without replacing ordinary
  scrolling.

The preview window is non-modal and not globally always-on-top. It closes with
the history window and follows it when minimized. Its frame is remembered by
renderer family, bounded to the current display's usable work area.

## Supported formats

- selectable plain text with search, wrapping, a remembered 18 px default
  font size, copy-all, and line/character counts;
- conservatively detected source code with line numbers and bundled syntax
  highlighting, plus a manual text/code mode switch;
- rendered Markdown article view with distinct headings, paragraphs, nested
  lists/tasks, block quotes, fenced and indented code, horizontal rules, and
  pipe tables (no source or split view);
- clipboard images and PNG, JPEG, GIF, and WebP files centered and fitted on
  the first frame, with fit-relative zoom, bounded pan hit-testing, view-only
  rotation, transparency, and dimensions;
- PDF with pagination, asynchronous search, modifier-wheel zoom, selectable
  text, and on-demand thumbnail navigation;
- DOCX with a local renderer on Windows and the native preview path on macOS;
- PPT and PPTX through the native macOS Quick Look preview path after signature
  validation;
- SVG on macOS rasterized by the system browser engine inside a locked-down
  offscreen web view, and on Windows rendered in a sandboxed WebView2 iframe;
  both provide a compact preview/source control and a per-entry "trust
  external links" switch in the shared header. A render in progress shows a
placeholder, and render failures or oversized SVGs fall back to source text;
- metadata and restore controls for unsupported formats such as XLSX.

## Batch navigation

The backend returns ordered metadata for a file batch but decrypts only the
current item. The title bar shows the current file name and position (for
example, `2 / 6`). Failure to load one item does not prevent navigation to the
rest of the batch.

## Failure states

Loading, oversized payloads, unsupported formats, corrupted data, decryption
failures, and temporary transport failures are presented separately. Retry is
offered only for retryable failures. Restore remains available whenever the
stored history entry can still be materialized safely, and the window confirms
when the entry has been restored to the clipboard.

## Security and resource limits

Preview payloads are capped at 64 MiB and checked before and after decryption.
Markdown is sanitized and cannot automatically load remote images, media,
frames, scripts, or styles. Links are handed to the system browser only after
an explicit click. On macOS, SVG markup is rendered by the system browser
engine inside a locked-down offscreen WKWebView: JavaScript is disabled at the
configuration level, the host document carries a `default-src 'none'`
Content-Security-Policy that blocks every network subresource (inline styles
and `data:` images/fonts remain available so design-tool exports keep their
appearance), the SVG is loaded with a nil base URL (unique origin, no file
access), and the data store is non-persistent. Because CSP does not constrain
top-level navigation, the navigation delegate permits only the initial
`about:blank` document load and cancels everything else — `<meta
http-equiv="refresh">`, links, and form submissions cannot leave the origin.
Each render is bounded by an 8 MiB input limit, 4,096-pixel dimension limit,
16-million-pixel output limit, and a four-second watchdog; a render in
progress shows a placeholder instead of intermediate source text. The
snapshot is taken in memory and the web view is destroyed immediately after
it; any render failure returns to the in-memory escaped source viewer. SVG
bytes never touch disk or Quick Look.
A per-entry "trust external links" switch, off by default and reset on
navigation, lets the user opt the current preview into loading external
images and fonts — always through a confirmation dialog listing the exact
HTTPS origins (with ports), with Allow as an explicit second button.
References to non-HTTPS targets, URLs with embedded credentials, or literal
private/loopback/link-local IP hosts are disclosed as refused and block trust
entirely. Browser-compatible decimal, octal, hexadecimal, and shortened IPv4
spellings receive the same non-public classification as their canonical
addresses, as do trailing-root-dot local names such as `localhost.` and
`printer.local.`. Trusted mode lists only the approved exact HTTPS origins in
`img-src` and `font-src` (preserving explicit ports), so an undisclosed host
remains blocked even if extraction misses its syntax; the renderer also
re-checks eligibility before rendering. Enabling trust is transactional: the
trusted state commits only after the trusted re-render completes (installing
the macOS snapshot or loading the Windows iframe), so a failed or timed-out
render never leaves the UI claiming trust it did not deliver. Pending trust is
bound to the current entry identity and payload, so replacing an entry cannot
inherit network access before navigation-reset effects run.

Both platforms share one SVG policy, pinned by
`shared/svg-preview-policy-fixtures.json`: the same trust eligibility rules,
reference extraction (srcset, HTML-entity-encoded URLs, CSS `url()`), visual
eligibility gate, and byte-identical CSP construction are asserted by the
XCTest and vitest suites against the same fixture file, and the
cross-platform sync script validates the fixtures themselves. Documents
containing active or navigation markup (scripts, meta refresh, frames, SMIL
or CSS animation) are classified to the source viewer on both platforms
before any web view or iframe is created, with a notice explaining why;
oversized documents classify the same way, and only transient render
failures offer a retry.

On Windows, the SVG is kept in memory and placed in a `srcdoc` document inside
an iframe with an empty sandbox (no scripts, forms, top navigation, or host
page access). Its CSP defaults to no network access and only permits inline
styles and `data:` images/fonts; trusted mode adds only the exact approved
public HTTPS origins to the image/font policy. The same 8 MiB input,
4,096-pixel dimension, 16-million-pixel output, and four-second watchdog
limits apply, and a timeout falls back to the in-memory source viewer.
The visual renderers differ by design: macOS snapshots into a bounded
raster while Windows displays a live sandboxed vector iframe, so very large
or deeply scaled documents can look different across platforms while
receiving identical security treatment.

Windows keeps decrypted preview bytes in memory and revokes Blob URLs on item
replacement and window close. macOS creates a plaintext temporary file only
when a native Quick Look Office preview requires it. Its preview directory uses
mode 0700, files use mode 0600, and cleanup runs on replacement, close, and
startup.
