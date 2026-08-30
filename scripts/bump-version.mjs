#!/usr/bin/env node
// Single-entry version bump (T355 implementation of the T003 design).
//
// Writes the new version into all version-bearing files (#1-#12 in
// VERSION_MATRIX), keeps the Cargo.lock files in sync without
// re-resolving dependencies, and self-verifies through the existing
// validate-release-version.mjs. The "current product version" markers in
// README/CONTEXT/USER_GUIDE/THEMING are part of the same matrix: each
// marker must appear exactly once and match the manifest version, so
// `--check` (run in CI) fails on doc drift.

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { validateRepositoryVersions, releaseChannel } from './validate-release-version.mjs';

const SEMVER = /^v?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/;

function fail(message) {
  throw new Error(message);
}

function option(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

function serializeJson(parsed) {
  return `${JSON.stringify(parsed, null, 2)}\n`;
}

function writeIfChanged(path, next, written, dryRun) {
  if (readFileSync(path, 'utf8') === next) return;
  if (!written.includes(path)) written.push(path);
  if (!dryRun) writeFileSync(path, next);
}

function bumpJson(path, mutate, written, dryRun) {
  const parsed = readJson(path);
  mutate(parsed);
  writeIfChanged(path, serializeJson(parsed), written, dryRun);
}

function bumpCargoToml(path, version, written, dryRun) {
  const lines = readFileSync(path, 'utf8').split('\n');
  let inPackage = false;
  let changed = false;
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (/^\[/.test(line)) {
      inPackage = line.startsWith('[package');
      continue;
    }
    if (inPackage && /^version\s*=/.test(line)) {
      lines[index] = line.replace(/^(version\s*=\s*")[^"]+(")/, `$1${version}$2`);
      changed = true;
      inPackage = false;
    }
  }
  if (!changed) fail(`Could not find a [package] version line in ${path}`);
  writeIfChanged(path, `${lines.join('\n')}`, written, dryRun);
}

function bumpCargoLock(path, packageName, version, written, dryRun) {
  const lines = readFileSync(path, 'utf8').split('\n');
  let inBlock = false;
  let changed = false;
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (/^\[\[package\]\]/.test(line)) {
      inBlock = true;
      continue;
    }
    if (inBlock && /^\[/.test(line)) {
      inBlock = false;
      continue;
    }
    if (inBlock && line.trim() === `name = "${packageName}"`) {
      for (let versionIndex = index + 1; versionIndex < lines.length; versionIndex += 1) {
        const versionLine = lines[versionIndex];
        if (/^\[/.test(versionLine)) break;
        if (/^version\s*=/.test(versionLine)) {
          lines[versionIndex] = versionLine.replace(
            /^(version\s*=\s*")[^"]+(")/,
            `$1${version}$2`,
          );
          changed = true;
          break;
        }
      }
      inBlock = false;
    }
  }
  if (!changed) fail(`Could not find package ${packageName} in ${path}`);
  writeIfChanged(path, `${lines.join('\n')}`, written, dryRun);
}

const JSON_VERSION_FILES = [
  'windows/src-tauri/tauri.conf.json',
  'macos/src-tauri/tauri.conf.json',
  'windows/package.json',
  'site/package.json',
];

const CARGO_TOML_FILES = [
  'windows/src-tauri/Cargo.toml',
  'macos/src-tauri/Cargo.toml',
  'shared/rust-core/Cargo.toml',
  'shared/tailsync-protocol/Cargo.toml',
  'shared/tailsync-themes/Cargo.toml',
  'shared/tailsync-history-classifier/Cargo.toml',
];

const LOCK_ROOTS = [
  // Application locks: tailsync + tailsync-core + the extracted shared crates.
  ['windows/src-tauri/Cargo.lock', 'tailsync', 'tailsync-core',
   'tailsync-protocol', 'tailsync-themes', 'tailsync-history-classifier'],
  ['macos/src-tauri/Cargo.lock', 'tailsync', 'tailsync-core',
   'tailsync-protocol', 'tailsync-themes', 'tailsync-history-classifier'],
  // Root workspace lock: the four shared crates.
  ['Cargo.lock', 'tailsync-core', 'tailsync-protocol', 'tailsync-themes',
   'tailsync-history-classifier'],
];

const PACKAGE_LOCK_FILES = ['windows/package-lock.json', 'site/package-lock.json'];

// "Current product version" markers in prose docs. Each entry's pattern must
// match exactly once in the file; bump rewrites the capture and --check pins
// it to the manifest version. Examples in RELEASE.md and test fixtures
// deliberately use other versions and stay out of this matrix.
const DOC_VERSION_MARKERS = [
  {
    relative: 'README.md',
    // Version badge label and link target: both must carry the same version.
    pattern: /\[!\[Version\]\(https:\/\/img\.shields\.io\/badge\/Version-v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)-D5684B\)\]\(https:\/\/github\.com\/monet4070\/TailSync\/tree\/v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)\)/,
    build: (version) =>
      `[![Version](https://img.shields.io/badge/Version-v${version}-D5684B)](https://github.com/monet4070/TailSync/tree/v${version})`,
  },
  {
    relative: 'README.md',
    pattern: /> TailSync (\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?) 目前处于积极开发阶段/,
    build: (version) => `> TailSync ${version} 目前处于积极开发阶段`,
  },
  {
    relative: 'README.md',
    pattern: /当前产品版本为 (\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)，数据库 schema 为/,
    build: (version) => `当前产品版本为 ${version}，数据库 schema 为`,
  },
  {
    relative: 'CONTEXT.md',
    pattern: /线协议 v4；产品版本 (\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)；数据库 schema v10/,
    build: (version) => `线协议 v4；产品版本 ${version}；数据库 schema v10`,
  },
  {
    relative: 'docs/USER_GUIDE.zh-CN.md',
    pattern: /> 适用版本：TailSync (\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)，线协议 v4/,
    build: (version) => `> 适用版本：TailSync ${version}，线协议 v4`,
  },
  {
    relative: 'docs/THEMING.md',
    pattern: /> 适用版本：产品 (\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)，Theme V2/,
    build: (version) => `> 适用版本：产品 ${version}，Theme V2`,
  },
  {
    relative: 'docs/THEMING.md',
    pattern: /SemVer，且 ≤ 当前 Core 版本（(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)），否则包被拒绝/,
    build: (version) => `SemVer，且 ≤ 当前 Core 版本（${version}），否则包被拒绝`,
  },
];

function globalPattern(pattern) {
  return pattern.flags.includes('g') ? pattern : new RegExp(pattern.source, `${pattern.flags}g`);
}

function bumpDocMarkers(root, version, written, dryRun) {
  for (const marker of DOC_VERSION_MARKERS) {
    const path = resolve(root, marker.relative);
    const content = readFileSync(path, 'utf8');
    const matches = [...content.matchAll(globalPattern(marker.pattern))];
    if (matches.length !== 1) {
      fail(`Expected exactly one version marker in ${marker.relative}, found ${matches.length}`);
    }
    const expected = marker.build(version);
    writeIfChanged(
      path,
      content.replace(marker.pattern, () => expected),
      written,
      dryRun,
    );
  }
}

function verifyDocMarkers(root, version) {
  for (const marker of DOC_VERSION_MARKERS) {
    const content = readFileSync(resolve(root, marker.relative), 'utf8');
    const matches = [...content.matchAll(globalPattern(marker.pattern))];
    if (matches.length !== 1) {
      fail(`Expected exactly one version marker in ${marker.relative}, found ${matches.length}`);
    }
    for (const [index, captured] of matches[0].slice(1).entries()) {
      if (captured !== version) {
        fail(
          `${marker.relative} marker ${index + 1} pins the product version to ${captured}, expected ${version}`,
        );
      }
    }
  }
}

function requireFiles(root, relatives) {
  for (const relative of relatives) {
    if (!existsSync(resolve(root, relative))) fail(`Missing version file: ${relative}`);
  }
}

export function bumpRepositoryVersions(root, version, dryRun = false) {
  const expected = version.replace(/^v/, '');
  requireFiles(root, [
    ...JSON_VERSION_FILES,
    ...CARGO_TOML_FILES,
    ...PACKAGE_LOCK_FILES,
    ...DOC_VERSION_MARKERS.map((marker) => marker.relative),
    ...LOCK_ROOTS.map(([relative]) => relative),
  ]);
  const written = [];
  for (const relative of JSON_VERSION_FILES) {
    bumpJson(resolve(root, relative), (parsed) => {
      parsed.version = expected;
    }, written, dryRun);
  }
  for (const relative of CARGO_TOML_FILES) {
    bumpCargoToml(resolve(root, relative), expected, written, dryRun);
  }
  for (const [relative, ...packageNames] of LOCK_ROOTS) {
    for (const packageName of packageNames) {
      bumpCargoLock(resolve(root, relative), packageName, expected, written, dryRun);
    }
  }
  for (const relative of PACKAGE_LOCK_FILES) {
    bumpJson(resolve(root, relative), (parsed) => {
      parsed.version = expected;
      if (parsed.packages?.['']) {
        parsed.packages[''].version = expected;
      }
    }, written, dryRun);
  }
  bumpDocMarkers(root, expected, written, dryRun);
  return written;
}

export function verifyRepositoryVersions(root, version) {
  const tag = version.startsWith('v') ? version : `v${version}`;
  validateRepositoryVersions(root, tag);
  const expected = version.replace(/^v/, '');
  for (const [relative, ...packageNames] of LOCK_ROOTS) {
    const lock = readFileSync(resolve(root, relative), 'utf8');
    let allPresent = lock.includes(`version = "${expected}"`);
    for (const packageName of packageNames) {
      // The version line must immediately follow the package name within
      // the same [[package]] block (only whitespace/newlines between them).
      // Lock files may use CRLF or LF line endings.
      const re = new RegExp(`name = "${packageName}"\\r?\\nversion = "${expected}"`);
      allPresent = allPresent && re.test(lock);
    }
    if (!allPresent) {
      fail(`${relative} does not contain version ${expected} for all expected packages`);
    }
  }
  for (const relative of PACKAGE_LOCK_FILES) {
    const lock = readJson(resolve(root, relative));
    if (lock.version !== expected || lock.packages?.['']?.version !== expected) {
      fail(`${relative} is not at version ${expected}`);
    }
  }
  verifyDocMarkers(root, expected);
  return releaseChannel(tag);
}

function main() {
  const root = resolve(option('--root') ?? '.');
  const target = option('--target');
  if (!target) {
    fail('Usage: bump-version.mjs <X.Y.Z|--target X.Y.Z> [--root PATH] [--check] [--dry-run]');
  }
  const match = SEMVER.exec(target);
  if (!match) fail(`Not a semantic version: ${target}`);
  const version = match[1];
  const dryRun = process.argv.includes('--dry-run');
  const checkOnly = process.argv.includes('--check');

  if (checkOnly) {
    const channel = verifyRepositoryVersions(root, version);
    process.stdout.write(
      `Release version ${version} is consistent across all version files (${channel} channel).\n`,
    );
    return;
  }

  const written = bumpRepositoryVersions(root, version, dryRun);
  if (dryRun) {
    // The writes were skipped in dry-run mode; report what would change.
    process.stdout.write(
      written.length === 0
        ? `Dry run: version files already at ${version}; nothing to write.\n`
        : `Dry run: would update ${written.length} file(s) to ${version}.\n`,
    );
    return;
  }

  const channel = verifyRepositoryVersions(root, version);
  process.stdout.write(
    `Bumped ${written.length} file(s) to ${version} (${channel} channel); self-verification passed.\n`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
