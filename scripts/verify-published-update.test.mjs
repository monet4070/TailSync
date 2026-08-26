import assert from 'node:assert/strict';
import test from 'node:test';

import { validatePublishedManifest } from './verify-published-update.mjs';

function manifest(overrides = {}) {
  return {
    version: '2.2.0',
    platforms: {
      'windows-x86_64': {
        url: 'https://example.com/windows.nsis.zip',
        signature: 'windows-signature',
      },
      'darwin-aarch64': {
        url: 'https://example.com/macos-arm64.app.tar.gz',
        signature: 'macos-arm64-signature',
      },
      'darwin-x86_64': {
        url: 'https://example.com/macos-x64.app.tar.gz',
        signature: 'macos-x64-signature',
      },
    },
    ...overrides,
  };
}

test('accepts a complete published updater manifest', () => {
  assert.doesNotThrow(() => validatePublishedManifest(manifest(), { version: 'v2.2.0' }));
});

test('rejects a manifest that does not match the published version', () => {
  assert.throws(
    () => validatePublishedManifest(manifest(), { version: '2.2.1' }),
    /expected 2\.2\.1/,
  );
});

test('rejects a missing platform artifact', () => {
  const broken = manifest({
    platforms: {
      'windows-x86_64': {
        url: 'https://example.com/windows.nsis.zip',
        signature: 'windows-signature',
      },
    },
  });
  assert.throws(
    () => validatePublishedManifest(broken, { version: '2.2.0' }),
    /missing platform darwin-aarch64/,
  );
});

test('rejects an empty artifact signature', () => {
  const broken = manifest();
  broken.platforms['darwin-aarch64'].signature = '  ';
  assert.throws(
    () => validatePublishedManifest(broken, { version: '2.2.0' }),
    /empty signature for darwin-aarch64/,
  );
});
