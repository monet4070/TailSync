# Security Audit Adjudication — 2026-08

This document records the final adjudication of the 2026-08 external audit
reports against `main@eac7310`, so the same findings are not re-litigated.
Every item below was verified directly against the source tree before
acceptance; several widely-repeated claims were disproven and are listed at
the bottom.

## Accepted (fixed or scheduled)

| Finding | Resolution |
|---|---|
| Managed data directories and encrypted containers are created without explicit owner-only permissions (`db/paths.rs`, `db/file_encryption.rs`, `db/file_storage.rs`) | Accepted. The most substantive gap found: identity files were locked (`identity.rs`) but the rest of the managed storage tree was not. Fixed by the unified private-storage permission module. The final pass also routed migration reports, API imports, transfer-resume state, storage migration, and Windows key temporaries through it; managed symlinks and permission-repair failures now fail closed, clipboard materialization copies rather than hard-links user files, and startup repair detaches legacy hard links before changing permissions so upgrading cannot modify the original file. |
| `withGlobalTauri: true` in both `tauri.conf.json` files while the frontend never touches `window.__TAURI__` | Accepted. Disabled in both configs; CSP and capabilities unchanged. |
| Unrelated `teamspeak3.rar` download location in `deploy/nginx/luminousity.conf` | Accepted. Location removed from the config. The production server still needs `nginx -t` + reload and a 404 check on the URL. Keeping the config in-tree is fine: the domain and standard cert paths are not secrets. |
| README/CONTEXT/USER_GUIDE/THEMING pinned the product version to 2.2.0 while manifests were at 2.2.2 | Accepted. Doc version markers are now part of the `bump-version.mjs` matrix; `--check` (already a CI step) fails on drift, missing, or duplicated markers. |
| UDP discovery responder replies to every matching probe without per-source throttling | Resolved 2026-08-27: replies now pass `peer::discovery_admission` (shared in core, pinned by the drift check on both platforms) — non-LAN/Tailscale sources are never answered, per-source (burst 8 / 2 per second) and global (burst 128 / 32 per second) budgets bound the reply rate, and the 1024-entry source table is idle-expired so spoofed sources cannot grow it. Note the residual, accepted trade-off: a same-segment device can still enumerate hostname and Iroh endpoint ID at the throttled rate — that disclosure is inherent to the discovery protocol; a discoverability toggle would be a separate product feature. |
| `repeated_rtt_probes` iroh test ignored since 2026-08-14 | Resolved 2026-08-27: the `#[ignore]` was removed, both connects carry explicit 10 s outer timeouts, and the server drains each probe's QUIC close before accepting the next one so close-handshake timing cannot masquerade as an endpoint-isolation regression. Temporary 10x CI stress runs on both platforms verify stability. |
| No dependency-update automation or RustSec audit | Resolved 2026-08-27: monthly Dependabot covers all three Cargo manifests, both npm projects, and Actions (no auto-merge or grouping); cargo-deny advisories checks run on push/PR and daily with `--locked` over the supported macOS/Windows graphs. The three lockfiles use patched `h2` 0.4.19. Advisory exceptions require a full registry entry (reason, upstream link, owner, expiry, tracking issue) validated by `scripts/check-rustsec-exceptions.mjs`, which fails on invalid dates, duplicates, drift, or expiry. The sole current exception is the direct `bincode` dependency tracked by issue 28; transitive unmaintained notices remain owned by their direct-dependency upgrade path. Licenses/bans/sources checks are phase 2. The iroh `=1.1.0` pin carries its review rationale in `Cargo.toml`. |

## Downgraded (verified real, but not as reported)

| Finding | Adjudication |
|---|---|
| "Fixed test DEK compiled into production binaries" (`crypto.rs` `get_dek`) | The `test-support` feature is only activated from `[dev-dependencies]` in both platform crates; normal dependency edges carry no features, so shipped binaries never compile the fixed-DEK fallback. The runtime guard (`is_cargo_test_harness_executable`) additionally requires a Cargo libtest-harness path shape. No refactor needed — `scripts/check-production-features.mjs` now fails CI if the feature ever moves into a production edge, including multiline arrays, dependency-feature forwarding, and dotted TOML keys. |
| Trusted public keys stored without format validation (`Settings::trust_peer_without_save`) | The external trust entry points validate before writing (`identity.rs` `trust_peer` normalizes and checks the 32-byte key; pairing stores the Noise handshake key directly). The missing check in the settings method is an internal-invariant hardening item, not an exploitable gap — admission decodes and rejects malformed keys anyway. |
| `iroh` pinned exactly | Upgraded from 1.0.3 to 1.1.0 on 2026-08-27. The exact pin stays because Iroh network behavior needs explicit cross-version and real-device verification; future bot-driven upgrades remain deliberate review events. |
| TCP listener binds `0.0.0.0` | Pre-handshake source filtering (`source_matches_mode`, covering `auto` mode), the 64/8 connection limiter, and mandatory Noise authentication make this the standard design for a LAN-discovery P2P app. Binding specific interfaces instead would break multi-homed/DHCP setups. |

## Rejected (no action)

- **Constant-time comparison for `admission.rs` public-key equality.** The compared values are public identity keys, not secrets; timing protection buys nothing here.
- **Encrypting `config-v2.json`.** It holds public keys, addresses, and UI settings. Key-lifecycle and migration complexity is not justified by the threat model; the identity file already carries the actual secret under OS-keystore protection.
- **`entitlements: null` as a vulnerability.** Requesting no extra entitlement is not over-privilege. App Sandbox is a distribution-strategy decision (it constrains background clipboard access and listener sockets), not a security patch.
- **Immediate TCP connection-rate time-window limiting.** Existing per-source/global concurrency caps plus the 5s handshake timeout suffice for now; a naive sliding window would hurt wake-from-sleep reconnect storms. Revisit only if untrusted-shared-LAN enters the threat model.
- **8-digit pairing codes.** The code is HKDF-derived and bound to both handshake keys; with the 5-failure lockout and Noise's properties, the added entropy is not worth the UX cost.

## Disproven (do not reopen without new evidence)

- "Identity private key falls back to plaintext on non-keystore platforms" — `SystemKeyStore` returns `unsupported platform` and refuses to run; there is no plaintext fallback.
- "Non-test `unwrap()` calls in `crypto.rs`" — all of them live inside `#[cfg(test)] mod tests`.
- "`auto` connection mode exposes the TCP listener to the internet" — `auto` = `lan_only ∪ tailscale_only`; public sources are rejected before the Noise handshake.
- "Key management uses constant-time comparison" — no constant-time primitive exists in the tree (see Rejected above for why it is not needed).
