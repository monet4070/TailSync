#!/usr/bin/env node
// Single-entry version bump (T355 implementation of the T003 design).
//
// Writes the new version into all version-bearing files (#1-#12 in
// VERSION_MATRIX), keeps the Cargo.lock files in sync without
// re-resolving dependencies, and self-verifies through the existing
// validate-release-version.mjs. README badges (#13) are intentionally not
// touched unless --readme is passed.

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
