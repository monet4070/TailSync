# Collecting TailSync diagnostics

Report the product version shown in Settings/About, OS version/build and CPU
architecture on each peer. Include transfer direction, operation time/timezone,
text/image/file type (use synthetic examples), current peer online/pairing state,
selected LAN/Tailscale/Iroh route and whether reconnect/restart preceded the issue.
For a candidate build include its source fingerprint and package SHA-256.

- macOS SwiftUI launches its daemon with output in
  `~/Library/Logs/TailSync/tailsyncd.log` (truncated on daemon restart once over 5 MiB).
- Windows writes `tailsync.log` and `tailsync.log.1` in the application data root,
  normally `%APPDATA%\tailsync\TailSync\data`. Startup errors can also be present there.
- Custom `TAILSYNC_DATA_DIR` changes the identity/settings and Windows log root;
  custom storage changes history/payload placement, not the macOS UI log location.

Share a small excerpt around the failure time. Redact usernames, absolute local
paths, peer hostnames/IPs/endpoint IDs and clipboard text before sharing. Never
share identity private keys, encryption keys, local API tokens or a raw settings/
history database. Keep originals privately so repeated steps remain comparable.
Production Windows uses Tauri commands; TCP 19889 is not a supported diagnostics
API. macOS uses its authenticated local Unix socket.

Recovery logic exists for reconnect and wake. Single-Mac harness tests prove L4
process/transport behavior; they do not certify physical sleep/wake, NAT/relay,
Windows native clipboard/DPAPI/WebView2 or an older OS. macOS product minimum is
13.0; deployment metadata validation does not prove runtime on macOS 13 hardware.
