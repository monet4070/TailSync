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
- SVG is treated as text/code and is never executed as active markup.
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
