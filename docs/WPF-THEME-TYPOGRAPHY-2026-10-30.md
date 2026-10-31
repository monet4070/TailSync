# Theme typography refinement — 2026-10-30

Later Canvas refinement: its reading role now prefers Noto Serif SC / Source
Han Serif SC at 16px, with UI/search still separate. Other themes retain the
roles below. See [Canvas serif refinement](WPF-CANVAS-SERIF-2026-10-30.md).

Builtin typography now separates expressive display faces from compact reading
and control text. Fonts and sizes are resolved by Core, then applied by the
renderer; the WPF view does not substitute a fixed face for a selected theme.

| Role | Canvas / Ledger | Flux | Aura | Mono |
| --- | --- | --- | --- | --- |
| Display title | existing serif, 24px | existing geometric face, 22px | existing rounded face, 23px | existing monospace face, 21px |
| History | reading face, 15px | reading face, 15px | reading face, 15px | Cascadia Code / Mono fallback, 14px |
| Search | UI face, 16px | UI face, 16px | UI face, 16px | UI face, 15px |
| UI / controls | UI face, 14px | UI face, 14px | UI face, 14px | UI face, 14px |

The body weight is 400; builtin buttons use 500. Captions keep their smaller
existing scale. Light/dark modes share typography, while theme colors retain
their existing palettes. Existing pixel-alignment and text-rendering fixes are
preserved, along with thumbnails and row selection.

`typography.history.useDisplayFont` is an optional boolean, parallel to the
existing search setting. False selects `typography.reading.families`; true
selects `typography.display.families`. Builtins explicitly select false. Old
custom packages that omit the token keep their previous role selection; the
new builtin role is removed from the custom-package foundation before overlays
are applied. Authors can set it in foundation, mode or platform tokens.

WPF, the WebView CSS adapter and SwiftUI HistoryRow consume the explicit token.
The Windows deliverable is rebuilt and verified here. SwiftUI source support
is updated, but a macOS runtime build cannot be validated on this Windows host.

Validation includes the shared theme suite, WebView adapter tests and TypeScript
checks, five-theme light/dark WPF captures, legacy custom-theme migration,
native thumbnail checks and narrow-window/control layout checks. Current
acceptance artifacts live under `F:/TailSync/theme-typography-qa-20261030`.
