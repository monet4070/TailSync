/// Single source of truth for facts the marketing site quotes about the product.
///
/// `PRODUCT_VERSION` is injected at build time from `site/package.json`, which
/// `scripts/bump-version.mjs` already rewrites on every release (it is entry #4
/// of that script's `JSON_VERSION_FILES`). Nothing here may hardcode a version:
/// the site drifted to 2.0.1 while the product shipped 2.1.0 precisely because
/// six `.tsx` files each carried their own copy.
///
/// Everything else in this file is read from source, not from the README. When
/// a figure changes in `shared/rust-core`, change it here — never inline it in a
/// component.
export const PRODUCT_VERSION = __TAILSYNC_VERSION__;

export const GITHUB_URL = "https://github.com/monet4070/TailSync";
export const RELEASE_URL = `${GITHUB_URL}/releases`;
export const RELEASE_TAG_URL = `${GITHUB_URL}/releases/tag/v${PRODUCT_VERSION}`;

/// Illustrative installer filenames used inside the mocked product UI. These
/// are decoration, not download links — but they still carry the version so the
/// screenshots never look a release behind.
export const MAC_INSTALLER_NAME = `TailSync-v${PRODUCT_VERSION}-universal.dmg`;
export const WINDOWS_INSTALLER_NAME = `TailSync-${PRODUCT_VERSION}-setup.exe`;

/// Verified product limits. Each entry names the file it was read from so the
/// next person can re-check it in one step.
export const PRODUCT_FACTS = {
  /// `MAX_FILE_BATCH_COUNT` — shared/rust-core/src/sync.rs:32
  filesPerBatch: 20,
  /// `MAX_FILE_BATCH_BYTES` — shared/rust-core/src/sync.rs:33
  batchBytesLabel: "1 GiB",
  /// `FILE_CHUNK_SIZE` — shared/rust-core/src/protocol.rs
  chunkSizeLabel: "1 MiB",
  /// `CATEGORIES` — shared/rust-core/src/history_classifier.rs:4
  categoryCount: 8,
  /// `CLASSIFIER_VERSION` — shared/rust-core/src/history_classifier.rs:1
  classifierVersion: 4,
  /// Health TTL / two-round miss — CONTEXT.md, shared/rust-core/src/peer/health.rs
  offlineDetectionLabel: "8–12 秒",
  /// `builtin_ids()` — shared/rust-core/src/themes_v2.rs:365
  builtinThemeCount: 5,
  /// Importable packages shipped in themes/ — 5 `.tailsync-theme` files
  themePackageCount: 5,
} as const;
