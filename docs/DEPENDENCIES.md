# TailSync dependency policy

TailSync separates vulnerability response from ordinary dependency maintenance.

## 1. Security updates

The following controls must remain enabled:

- GitHub dependency graph
- Dependabot alerts
- Dependabot security updates
- Daily cargo-deny advisory audit

Ordinary version-update schedules and pull-request limits must never be treated
as the security notification mechanism.

The Dependabot configuration must not use `ignore` or
`versioning-strategy: lockfile-only` unless the security-update impact has been
explicitly reviewed.

## 2. Ordinary dependency updates

Ordinary Dependabot version updates are disabled for every configured package
ecosystem. Setting `open-pull-requests-limit: 0` suppresses version-update pull
requests without limiting Dependabot security-update pull requests.

Routine upgrades are collected into the quarterly review or an explicit
maintenance event instead of creating continuous automated pull requests.

### Cargo

Ordinary Dependabot Cargo version updates are disabled.

Cargo dependencies are updated only:

- to resolve an advisory or yanked release;
- as part of a planned dependency review;
- when required by a feature or platform change;
- during the quarterly lockfile refresh.

All applicable Cargo.lock files must be updated in one pull request:

- `/Cargo.lock`
- `/macos/src-tauri/Cargo.lock`
- `/windows/src-tauri/Cargo.lock`

### npm

Ordinary updates for `/windows` and `/site` are manual maintenance events. A
single reviewed change should update the applicable manifests and lockfiles and
must pass the frontend builds plus the packaged-application jobs.

Dependabot security updates remain enabled and may cross a major boundary when
required to reach the first patched release.

### GitHub Actions

All ordinary updates require manual review. Actions used only during tagged
releases require a release rehearsal or a documented comparison of inputs,
outputs and permissions. Dependabot security updates remain enabled.

## 3. Manual migration classes

### Protocol and cryptography

Examples:

- iroh
- snow
- ring
- rand
- base64
- hashing and encryption crates

Required validation:

- old/new client interoperability;
- pairing and identity pinning;
- encrypted frame round trips;
- LAN, Tailscale and iroh route tests;
- macOS and Windows packaged applications.

### Persistence and serialized formats

Examples:

- rusqlite
- schemars
- zip
- database, schema and archive dependencies

Required validation:

- existing database migration fixtures;
- Settings JSON Schema comparison;
- existing theme/archive fixtures;
- backward-compatible reads;
- rollback and corrupt-input behavior.

### Native runtime and packaging

Examples:

- Tauri
- windows
- windows-sys
- Tokio
- GitHub release and artifact actions

Required validation:

- real Windows compilation;
- NSIS executable smoke test;
- macOS bundle and daemon verification;
- updater manifest generation;
- tagged-release rehearsal when CI cannot exercise the action.

### Product renderers

Examples:

- pdfjs-dist
- docx-preview
- marked
- DOMPurify

Major updates require real renderer fixtures. A TypeScript build alone is not
sufficient evidence.

## 4. Yanked dependency runbook

A yanked release is detected by the daily cargo-deny workflow, but does not
necessarily create a Dependabot security pull request.

For a compatible replacement:

    git switch -c fix/<dependency>-yanked
    ./scripts/update-cargo-locks.sh <dependency>@<old-version> <new-version>

Review all three Cargo.lock files and run the full CI matrix.

If the replacement requires a parent or manifest update, stop using the
lockfile-only script and open a manual dependency migration pull request.

## 5. Security advisory runbook

1. Confirm the advisory and affected dependency paths.
2. Determine whether the dependency is built for a supported product target.
3. Prefer the minimum patched release.
4. Update every affected manifest and lockfile in one pull request.
5. Do not add a RustSec exception unless:
   - no patched release exists;
   - the dependency is required;
   - exploitability has been assessed;
   - an owner, expiry and tracking issue exist.
6. Run cargo-deny for all three manifests.
7. Require the complete packaged Windows and macOS CI jobs before merge.

## 6. Quarterly review

Once per quarter:

1. Inspect outdated direct Cargo dependencies.
2. Review exact pins and manual-major candidates.
3. Refresh compatible transitive dependencies across all three lockfiles.
4. Review npm and GitHub Actions major releases.
5. Remove expired freezes and RustSec exceptions.
6. Update this document when policy decisions change.

## 7. Current deliberate constraints

- `iroh` is exactly pinned because network behavior needs real cross-device
  verification.
- `pdfjs-dist` changes its TypeScript API inside minor releases (5.4 -> 5.7
  removed `getDocument`'s `isEvalSupported`), so every ordinary update is a
  manual migration requiring real PDF rendering regression tests.
- TypeScript and Node type-definition majors follow the selected Node/toolchain
  baseline.
- Release-only GitHub Actions majors require release-path validation.
- Rust 0.x minor releases are treated as potentially breaking.

Direct dependencies must also survive a source-use review. A package that is
available transitively is not evidence that each platform crate needs its own
manifest entry. During quarterly review, run Clippy with
`RUSTFLAGS='-W unused-crate-dependencies'` for the shared core and both platform
crates, verify feature-gated/native uses manually, and remove only proven
residue. Example-only dependencies belong in `[dev-dependencies]` unless a
shipped target imports them.
