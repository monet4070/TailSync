import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  mkdtempSync,
  mkdirSync,
  rmSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { checkProductionFeatures } from './check-production-features.mjs';

function manifest(body) {
  return `[package]
name = "tailsync"
version = "2.2.2"

${body}`;
}

function fixtureWith(body) {
  const root = mkdtempSync(join(tmpdir(), 'tailsync-features-'));
  mkdirSync(join(root, 'src-tauri'), { recursive: true });
  writeFileSync(join(root, 'src-tauri', 'Cargo.toml'), manifest(body));
  return root;
}

test('accepts test-support inside dev-dependencies only', () => {
  const root = fixtureWith(`[dependencies]
tailsync-core = { path = "../../shared/rust-core" }

[dev-dependencies]
tailsync-core = { path = "../../shared/rust-core", features = ["test-support"] }

[target.'cfg(target_os = "windows")'.dev-dependencies]
windows-sys = "0.59"`);
  try {
    assert.equal(checkProductionFeatures(root), 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('rejects test-support inside normal dependencies', () => {
  const root = fixtureWith(`[dependencies]
tailsync-core = { path = "../../shared/rust-core", features = ["test-support"] }`);
  try {
    assert.throws(() => checkProductionFeatures(root), /outside \[dev-dependencies\]/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('rejects test-support inside build-dependencies', () => {
  const root = fixtureWith(`[build-dependencies]
tailsync-core = { path = "../../shared/rust-core", features = ["test-support"] }`);
  try {
    assert.throws(() => checkProductionFeatures(root), /outside \[dev-dependencies\]/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('rejects test-support inside target-specific normal dependencies', () => {
  const root = fixtureWith(`[target.'cfg(target_os = "macos")'.dependencies]
tailsync-core = { path = "../../shared/rust-core", features = ["test-support"] }`);
  try {
    assert.throws(() => checkProductionFeatures(root), /outside \[dev-dependencies\]/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('rejects multiline test-support activation in normal dependencies', () => {
  const root = fixtureWith(`[dependencies]
tailsync-core = { path = "../../shared/rust-core", features = [
  "test-support",
] }`);
  try {
    assert.throws(() => checkProductionFeatures(root), /outside \[dev-dependencies\]/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('rejects production feature forwarding to test-support', () => {
  const root = fixtureWith(`[dependencies]
tailsync-core = { path = "../../shared/rust-core" }

[features]
default = ["tailsync-core/test-support"]`);
  try {
    assert.throws(() => checkProductionFeatures(root), /outside \[dev-dependencies\]/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('accepts the inert test-support feature declaration', () => {
  const root = fixtureWith(`[features]
test-support = []`);
  try {
    assert.equal(checkProductionFeatures(root), 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('rejects a top-level dotted production activation', () => {
  const root = fixtureWith(`[workspace]
members = []
dependencies.tailsync-core = { path = "../../shared/rust-core", features = ["test-support"] }`);
  try {
    assert.throws(() => checkProductionFeatures(root), /outside \[dev-dependencies\]/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('rejects a symlinked Cargo manifest instead of silently skipping it', { skip: process.platform === 'win32' }, () => {
  const root = fixtureWith(`[dependencies]
tailsync-core = { path = "../../shared/rust-core" }`);
  const manifestPath = join(root, 'src-tauri', 'Cargo.toml');
  const linkedManifest = join(root, 'linked-Cargo.toml');
  try {
    writeFileSync(linkedManifest, manifest(`[dependencies]
tailsync-core = { path = "../../shared/rust-core", features = ["test-support"] }`));
    unlinkSync(manifestPath);
    symlinkSync(linkedManifest, manifestPath);
    assert.throws(() => checkProductionFeatures(root), /symbolic link/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
