import assert from 'node:assert/strict';
import test from 'node:test';

import { validateUpdaterConfiguration } from './validate-updater-config.mjs';

const publicKey = Buffer.from(
  'untrusted comment: minisign public key: 0123456789ABCDEF\nRWQexamplekeymaterial1234567890=\n',
).toString('base64');
const endpoint = 'https://example.com/releases/latest/download/latest.json';

function config(key = publicKey, endpoints = [endpoint]) {
  return { plugins: { updater: { pubkey: key, endpoints } } };
}

test('accepts matching public keys and HTTPS endpoints', () => {
  const result = validateUpdaterConfiguration(publicKey, {
    windows: config(),
    macos: config(),
  });
  assert.deepEqual(result.endpoints, [endpoint]);
});

test('rejects a client public key that drifts from the trust anchor', () => {
  assert.throws(
    () => validateUpdaterConfiguration(publicKey, {
      windows: config(),
      macos: config(Buffer.from('different').toString('base64')),
    }),
    /does not match shared\/updater\.pub/,
  );
});

test('rejects insecure or inconsistent updater endpoints', () => {
  assert.throws(
    () => validateUpdaterConfiguration(publicKey, {
      windows: config(publicKey, ['http://example.com/latest.json']),
      macos: config(),
    }),
    /must use HTTPS/,
  );
  assert.throws(
    () => validateUpdaterConfiguration(publicKey, {
      windows: config(),
      macos: config(publicKey, ['https://mirror.example.com/latest.json']),
    }),
    /do not match/,
  );
});
