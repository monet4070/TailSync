import assert from 'node:assert/strict';
import test from 'node:test';

import { validatePublishedManifest } from './verify-published-update.mjs';

const signature = Buffer.from([
  'untrusted comment: signature from minisign secret key',
  Buffer.alloc(74).toString('base64'),
  'trusted comment: timestamp:0\tfile:test',
  Buffer.alloc(64).toString('base64'),
].join('\n')).toString('base64');

function manifest(overrides = {}) {
  return {
    version: '2.2.0',
    platforms: {
      'windows-x86_64': {
        url: 'https://example.com/windows.nsis.zip',
        signature,
      },
      'darwin-aarch64': {
        url: 'https://example.com/macos-arm64.app.tar.gz',
        signature,
      },
      'darwin-x86_64': {
        url: 'https://example.com/macos-x64.app.tar.gz',
        signature,
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
        signature,
      },
    },
  });
  assert.throws(
    () => validatePublishedManifest(broken, { version: '2.2.0' }),
    /missing platform darwin-aarch64/,
  );
});

test('rejects a malformed artifact signature', () => {
  const broken = manifest();
  broken.platforms['darwin-aarch64'].signature = '  ';
  assert.throws(
    () => validatePublishedManifest(broken, { version: '2.2.0' }),
    /signature is empty/,
  );
});
