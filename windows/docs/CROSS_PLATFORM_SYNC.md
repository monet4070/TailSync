# Windows/macOS synchronization contract

This document defines the contract between the Windows and macOS applications in the TailSync monorepo. The contract checker requires both copies of this document to remain identical.

## Shared implementation

Shared protocol, crypto, identity, pairing, history, and synchronization behavior has one canonical implementation in `shared/rust-core/`. The platform crates consume it through a path dependency and must not carry private copies of those modules.

The following platform files remain byte-for-byte aligned unless the contract checker is deliberately updated:

- protocol interoperability probe and acceptance scripts
- this contract

Windows uses React/Tauri; macOS uses SwiftUI and contains no parallel React product UI. OS-specific adapters for the API, clipboard, discovery, connection management, and tray may differ. Their ports, serialized models, Swift API commands, shared-core dependency, and on-wire behavior remain contract-checked.

## Network contract

- Peer synchronization and pairing: TCP `19890`
- macOS SwiftUI-to-daemon JSON-lines API: TCP `127.0.0.1:19889`
- LAN discovery: `_tailsync._tcp.local.` plus UDP discovery
- Connection policy: `auto`, `lan_only`, or `tailscale_only`
- Authentication: Noise XX with a pinned X25519 device identity
- Pairing: explicit 120-second window, six-digit verification, bilateral confirmation, five-failure lockout
- Reliable text/image events: stable message ID, timestamp validation, ACK, retry, and replay suppression
- Files: 1 MiB checked blocks, offset ACKs, and reconnect resume while the process remains running
- File echo suppression: files materialized under the app-managed `clipboard-files/` directory are never published as local clipboard copies; user-owned paths remain eligible for synchronization

Protocol v1 plaintext peers are rejected. There is no insecure fallback.

The local JSON-lines API is an internal macOS shell bridge. It binds to loopback and requires a per-process 256-bit capability token on every request. The SwiftUI parent writes the token once through the daemon's anonymous stdin pipe, then closes the pipe; the token is not placed in the daemon environment. Port `19889` must still not be exposed or proxied outside the host.

## Drift gate

From the repository root:

```bash
node windows/scripts/check_cross_platform_sync.mjs \
  --win-root windows \
  --mac-root macos \
  --core-root shared/rust-core
```

PowerShell wrapper:

```powershell
.\windows\scripts\check_cross_platform_sync.ps1 `
  -WinRoot windows `
  -MacRoot macos
```

The check fails on duplicated platform file drift, shared-core wiring errors, port changes, missing SwiftUI API commands/model fields, missing Bonjour declarations, or incomplete macOS release checks.

## Cross-project wire probe

Run in PowerShell with both projects available on the same machine:

```powershell
.\windows\scripts\test_cross_project_interop.ps1 `
  -WinRoot windows `
  -MacRoot macos
```

The probe builds each project's Rust example separately and tests both role assignments. It covers fixed-identity Noise XX, first-time bilateral pairing, reliable-event ACKs, and resumable file-block offset ACKs.

## Verification commands

Windows:

```powershell
cargo test --locked --manifest-path shared\rust-core\Cargo.toml
cargo test --locked --manifest-path windows\src-tauri\Cargo.toml --lib
cd windows
npm ci
npm test
npm run lint
npm run build
```

macOS:

```bash
bash scripts/verify_macos_release.sh /path/to/tailsync-v2-win
```

The macOS verifier runs the shared core, platform Rust, SwiftUI, cross-project and bundle checks; launches `TailSync.app`; verifies listeners on `19889` and `19890`; calls the authenticated local API; and round-trips a file URL through the packaged clipboard helper. It refuses to run if either port is already occupied.

## Two-device acceptance

1. Install current builds on both devices and select `auto`.
2. Open the pairing window on both devices, select the discovered peer, and confirm the same six-digit code.
3. Confirm the active route shows `LAN` while both devices share a LAN.
4. Copy text and an image in each direction; verify clipboard contents and history.
5. Transfer a file larger than 1 MiB in each direction and verify completion.
6. Interrupt a large transfer without quitting either application, restore connectivity, and verify resume from the confirmed offset.
7. Block LAN reachability while Tailscale remains connected and verify the route changes to `Tailscale`.
8. Revoke the peer and verify existing and new synchronization connections are rejected.

Packaged two-device acceptance remains a release gate even when the automated source and wire probes pass.
