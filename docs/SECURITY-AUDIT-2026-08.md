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
| UDP discovery responder replies to every matching probe without per-source throttling | Accepted, next iteration. The response discloses hostname, TCP port, and the Iroh endpoint ID to any same-segment device; throttling bounds the rate and blocks public sources but the disclosure itself is inherent to the discovery protocol (see the discovery-admission module notes). |
| `repeated_rtt_probes` iroh test ignored since 2026-08-14 | Resolved 2026-08-27: the environment regression recovered (11/11 consecutive local runs), the `#[ignore]` was removed, and both connects carry explicit 10 s outer timeouts so a future regression fails fast. Temporary 10× CI stress runs on both platforms verify stability; only a new environment-specific CI failure would trigger the fallback branch (deterministic state tests + scheduled real-QUIC test). |
| No dependency-update automation or RustSec audit | Accepted, next iteration. Dependabot monthly across the three Cargo workspaces, npm, and Actions; `cargo audit` with time-boxed, documented exceptions. |

## Downgraded (verified real, but not as reported)

| Finding | Adjudication |
|---|---|
| "Fixed test DEK compiled into production binaries" (`crypto.rs` `get_dek`) | The `test-support` feature is only activated from `[dev-dependencies]` in both platform crates; normal dependency edges carry no features, so shipped binaries never compile the fixed-DEK fallback. The runtime guard (`is_cargo_test_harness_executable`) additionally requires a Cargo libtest-harness path shape. No refactor needed — `scripts/check-production-features.mjs` now fails CI if the feature ever moves into a production edge, including multiline arrays, dependency-feature forwarding, and dotted TOML keys. |
| Trusted public keys stored without format validation (`Settings::trust_peer_without_save`) | The external trust entry points validate before writing (`identity.rs` `trust_peer` normalizes and checks the 32-byte key; pairing stores the Noise handshake key directly). The missing check in the settings method is an internal-invariant hardening item, not an exploitable gap — admission decodes and rejects malformed keys anyway. |
| `iroh = "=1.0.3"` pinned exactly | 1.0.3 is the current upstream release; there is nothing to upgrade to yet. The pin stays (Iroh's network behavior needs explicit cross-version verification), with a comment and bot-driven upgrade PRs when newer releases appear. |
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
