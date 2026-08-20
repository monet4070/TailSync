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
- SVG as source text only;
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
an explicit click. SVG markup is never executed.

Windows keeps decrypted preview bytes in memory and revokes Blob URLs on item
replacement and window close. macOS creates a plaintext temporary file only
when a native Quick Look Office preview requires it. Its preview directory uses
mode 0700, files use mode 0600, and cleanup runs on replacement, close, and
startup.
