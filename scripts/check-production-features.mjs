#!/usr/bin/env node
// Guards the dev-only `tailsync-core/test-support` feature from leaking into
// production dependency edges (T402, docs/SECURITY-AUDIT-2026-08.md).
//
// `test-support` compiles a fixed all-0x54 test DEK into the library so
// platform integration tests can run without touching the real Keychain or
// DPAPI key store. It is only safe as a `[dev-dependencies]` feature: a
// normal `[dependencies]`/`[build-dependencies]` edge with that feature would
// compile the fixed DEK fallback into shipped binaries. This check fails CI
// if any Cargo.toml references `test-support` outside a dev-dependencies
// section.

import { lstatSync, readdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

function fail(message) {
  throw new Error(message);
}

const SKIP_DIRECTORIES = new Set(['target', 'target-macos', 'node_modules', '.git', '.build']);

function findCargoTomls(root) {
  const found = [];
  const walk = (directory) => {
    for (const entry of readdirSync(directory)) {
      if (SKIP_DIRECTORIES.has(entry)) continue;
      const path = join(directory, entry);
      const metadata = lstatSync(path);
      // Following repository symlinks could escape the checkout or form a
      // loop; silently skipping them could omit a production manifest.
      if (metadata.isSymbolicLink()) {
        fail(`Production feature scan refuses symbolic link: ${path}`);
      }
      if (metadata.isDirectory()) {
        walk(path);
      } else if (entry === 'Cargo.toml') {
        found.push(path);
      }
    }
  };
  walk(root);
  return found;
}

function isDevDependenciesSection(header) {
  return /(?:^|\.)dev-dependencies(?:\.|$)/.test(header);
}

function stripTomlComment(line) {
  let quote = null;
  let escaped = false;
  for (let index = 0; index < line.length; index += 1) {
    const character = line[index];
    if (quote === '"' && escaped) {
      escaped = false;
      continue;
    }
    if (quote === '"' && character === '\\') {
      escaped = true;
      continue;
    }
    if (quote) {
      if (character === quote) quote = null;
      continue;
    }
    if (character === '"' || character === "'") {
      quote = character;
    } else if (character === '#') {
      return line.slice(0, index);
    }
  }
  return line;
}

export function checkProductionFeatures(root) {
  const manifests = findCargoTomls(resolve(root));
  if (manifests.length === 0) fail('No Cargo.toml files found under the repository root');
  const violations = [];
  for (const manifest of manifests) {
    const lines = readFileSync(manifest, 'utf8').split(/\r?\n/);
    let section = '';
    for (const line of lines) {
      const trimmed = stripTomlComment(line).trim();
      if (!trimmed) continue;
      const header = /^\[(.+)\]$/.exec(trimmed);
      if (header) {
        section = header[1].trim();
        continue;
      }
      if (!trimmed.includes('test-support') || isDevDependenciesSection(section)) continue;

      const isFeatureDefinition = section === 'features'
        && /^(?:test-support|"test-support"|'test-support')\s*=/.test(trimmed)
        && !trimmed.slice(trimmed.indexOf('=') + 1).includes('test-support');
      if (isFeatureDefinition) continue;

      // Dependency feature arrays may span lines, Cargo features can forward
      // to dependency features (`tailsync-core/test-support`), and dotted
      // TOML keys can declare those edges from a parent table. Keep the rule
      // deliberately strict: only dev-dependencies and the inert local
      // feature declaration may mention this production-unsafe feature.
      violations.push(`${manifest}: "${trimmed}" activates test-support outside [dev-dependencies]`);
    }
  }
  if (violations.length > 0) {
    fail(`test-support must stay a dev-only feature:\n${violations.join('\n')}`);
  }
  return manifests.length;
}

function main() {
  const root = process.argv[2] ?? '.';
  const manifests = checkProductionFeatures(root);
  process.stdout.write(
    `OK: ${manifests} Cargo.toml manifest(s) keep test-support dev-only.\n`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
