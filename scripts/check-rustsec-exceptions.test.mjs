import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import {
  checkRustsecExceptions,
  extractDenyIgnoreIds,
} from './check-rustsec-exceptions.mjs';

const DENY_TOML = `[graph]
targets = []

[advisories]
version = 2
yanked = "deny"
ignore = ["RUSTSEC-2024-0001"]
`;

function fixture({ exceptions, denyToml = DENY_TOML }) {
  const root = mkdtempSync(join(tmpdir(), 'tailsync-rustsec-'));
  mkdirSync(join(root, 'security'), { recursive: true });
  writeFileSync(
    join(root, 'security/rustsec-exceptions.json'),
    JSON.stringify({ exceptions }, null, 2) + '\n',
  );
  writeFileSync(join(root, 'deny.toml'), denyToml);
  return root;
}

function entry(overrides = {}) {
  const nextYear = new Date(Date.now() + 180 * 24 * 3600 * 1000)
    .toISOString()
    .slice(0, 10);
  return {
    id: 'RUSTSEC-2024-0001',
    reason: 'No fixed version exists upstream yet; impact limited to build-time tooling',
    upstream: 'https://github.com/example/upstream/issues/1',
    owner: 'monet',
    expires: nextYear,
    issue: 'https://github.com/monet4070/TailSync/issues/1',
    ...overrides,
  };
}

test('empty registry matches empty deny.toml ignore list', () => {
  const root = fixture({
    exceptions: [],
    denyToml: DENY_TOML.replace('ignore = ["RUSTSEC-2024-0001"]', 'ignore = []'),
  });
  try {
    assert.equal(checkRustsecExceptions(root), 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('registered exception passes and is counted', () => {
  const root = fixture({ exceptions: [entry()] });
  try {
    assert.equal(checkRustsecExceptions(root), 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('deny.toml entry without a registry entry fails', () => {
  const root = fixture({ exceptions: [] });
  try {
    assert.throws(() => checkRustsecExceptions(root), /without a registry entry/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('registry entry missing from deny.toml fails', () => {
  const root = fixture({
    exceptions: [entry()],
    denyToml: DENY_TOML.replace('ignore = ["RUSTSEC-2024-0001"]', 'ignore = []'),
  });
  try {
    assert.throws(() => checkRustsecExceptions(root), /not present in deny.toml/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('expired exception fails', () => {
  const yesterday = new Date(Date.now() - 24 * 3600 * 1000).toISOString().slice(0, 10);
  const root = fixture({ exceptions: [entry({ expires: yesterday })] });
  try {
    assert.throws(() => checkRustsecExceptions(root), /expired on/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('invalid calendar expiry date fails', () => {
  const root = fixture({ exceptions: [entry({ expires: '2099-99-99' })] });
  try {
    assert.throws(() => checkRustsecExceptions(root), /valid calendar date/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('entries missing required fields fail', () => {
  for (const field of ['reason', 'upstream', 'owner', 'expires', 'issue']) {
    const incomplete = entry();
    delete incomplete[field];
    const root = fixture({ exceptions: [incomplete] });
    try {
      assert.throws(() => checkRustsecExceptions(root), new RegExp(field), `missing ${field}`);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test('duplicate registry ids fail', () => {
  const root = fixture({ exceptions: [entry(), entry()] });
  try {
    assert.throws(() => checkRustsecExceptions(root), /Duplicate exception ids/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('duplicate deny.toml ignore ids fail', () => {
  const root = fixture({
    exceptions: [entry()],
    denyToml: DENY_TOML.replace(
      'ignore = ["RUSTSEC-2024-0001"]',
      'ignore = ["RUSTSEC-2024-0001", "RUSTSEC-2024-0001"]',
    ),
  });
  try {
    assert.throws(() => checkRustsecExceptions(root), /Duplicate deny.toml ignore ids/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('extractDenyIgnoreIds reads multiline ignore arrays', () => {
  const denyToml = `[advisories]
version = 2
ignore = [
    "RUSTSEC-2023-0001",
    "RUSTSEC-2024-0002",
]
`;
  assert.deepEqual(extractDenyIgnoreIds(denyToml), ['RUSTSEC-2023-0001', 'RUSTSEC-2024-0002']);
});
