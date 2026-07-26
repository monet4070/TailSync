# Windows/macOS synchronization contract

This document defines the shared contract between the Windows and macOS projects. The source drift check treats this file as shared content, so changes must be copied to both repositories.

## Shared implementation

The following must remain byte-for-byte aligned unless the drift checker is deliberately updated:

- shared React UI files under `src/`
- Rust backend under `src-tauri/src/`
- frontend and Rust dependency locks
- protocol interoperability probe and acceptance scripts
- this contract

The platform entry page and native-facing settings/history presentation may evolve independently in `App.tsx`, `index.css`, `pages/History.tsx`, and `pages/Settings.tsx`. The macOS SwiftUI shell and packaging scripts are validated against the same Rust API models and commands.

## Network contract

- Peer synchronization and pairing: TCP `19890`
- macOS SwiftUI-to-daemon JSON-lines API: TCP `127.0.0.1:19889`
- LAN discovery: `_tailsync._tcp.local.` plus UDP discovery
- Connection policy: `auto`, `lan_only`, or `tailscale_only`
- Authentication: Noise XX with a pinned X25519 device identity
- Pairing: explicit 120-second window, six-digit verification, bilateral confirmation, five-failure lockout
- Reliable text/image events: stable message ID, timestamp validation, ACK, retry, and replay suppression
- Files: 1 MiB checked blocks, offset ACKs, and reconnect resume while the process remains running

Protocol v1 plaintext peers are rejected. There is no insecure fallback.

The local JSON-lines API is an internal macOS shell bridge. It currently relies on loopback binding rather than a capability token; do not expose or proxy port `19889` outside the host.

## Drift gate

From either project root, provide both roots explicitly when the default sibling layout is not in use:

```bash
node scripts/check_cross_platform_sync.mjs \
  --win-root /path/to/tailsync-v2-win \
  --mac-root /path/to/tailsync-v2-mac-1
```

PowerShell wrapper:

```powershell
.\scripts\check_cross_platform_sync.ps1 `
  -WinRoot C:\path\to\tailsync-v2-win `
  -MacRoot C:\path\to\tailsync-v2-mac-1
```

The check fails on shared source drift, dependency drift, port changes, missing SwiftUI API commands/model fields, missing Bonjour declarations, or incomplete macOS release checks.

## Cross-project wire probe

Run in PowerShell with both projects available on the same machine:

```powershell
.\scripts\test_cross_project_interop.ps1 `
  -WinRoot C:\path\to\tailsync-v2-win `
  -MacRoot C:\path\to\tailsync-v2-mac-1
```

The probe builds each project's Rust example separately and tests both role assignments. It covers fixed-identity Noise XX, first-time bilateral pairing, reliable-event ACKs, and resumable file-block offset ACKs.

## Verification commands

Windows:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --lib
cargo fmt --manifest-path src-tauri\Cargo.toml --all -- --check
npm ci
npm run lint
npm run build
.\scripts\check_cross_platform_sync.ps1
.\scripts\test_cross_project_interop.ps1
npx tauri build --target x86_64-pc-windows-msvc --bundles nsis
```

macOS:

```bash
bash scripts/verify_macos_release.sh /path/to/tailsync-v2-win
```

The macOS verifier runs the frontend, Rust, SwiftUI, cross-project and bundle checks; launches `TailSync.app`; verifies listeners on `19889` and `19890`; calls the local API; and round-trips a file URL through the packaged clipboard helper. It refuses to run if either port is already occupied.

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
