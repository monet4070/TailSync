import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createIsolatedTestEnvironment } from './test-environment.mjs';

const runner = fileURLToPath(new URL('./run-isolated-tests.mjs', import.meta.url));

for (const exitCode of [0, 7]) {
  test(`isolates real settings, storage and legacy paths and cleans up after exit ${exitCode}`, () => {
    const root = mkdtempSync(join(tmpdir(), 'tailsync-user-sentinel-'));
    try {
      const inherited = { ...process.env };
      const sentinels = [];
      for (const key of ['TAILSYNC_DATA_DIR', 'TAILSYNC_STORAGE_DIR', 'TAILSYNC_V1_DATA_DIR']) {
        const path = join(root, key);
        mkdirSync(path);
        const sentinel = join(path, 'config-v2.json');
        writeFileSync(sentinel, 'real-user-data-sentinel');
        inherited[key] = path;
        sentinels.push(sentinel);
      }
      const child = spawnSync(process.execPath, [runner, '--', process.execPath, '-e', `
        const fs = require('node:fs'), path = require('node:path');
        const keys = ['TAILSYNC_DATA_DIR', 'TAILSYNC_STORAGE_DIR', 'TAILSYNC_V1_DATA_DIR'];
        const paths = keys.map(k => process.env[k]);
        for (const p of paths) fs.writeFileSync(path.join(p, 'config-v2.json'), 'test-client');
        console.log(JSON.stringify({ paths, home: process.env.HOME, userprofile: process.env.USERPROFILE }));
        process.exit(${exitCode});
      `], { env: inherited, encoding: 'utf8' });
      assert.equal(child.status, exitCode, child.stderr);
      const result = JSON.parse(child.stdout);
      assert.equal(result.home, inherited.HOME);
      assert.equal(result.userprofile, inherited.USERPROFILE);
      for (const path of result.paths) {
        assert.equal(existsSync(path), false, 'temporary data survives child exit');
        assert.equal(existsSync(dirname(path)), false, 'temporary test root survives child exit');
      }
      for (const sentinel of sentinels) assert.equal(readFileSync(sentinel, 'utf8'), 'real-user-data-sentinel');
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

test('concurrent invocations have separate stores and do not mutate the caller environment', () => {
  const inherited = { HOME: '/preserved-home', USERPROFILE: '/preserved-profile', TAILSYNC_DATA_DIR: '/live-data' };
  const first = createIsolatedTestEnvironment(inherited);
  const second = createIsolatedTestEnvironment(inherited);
  try {
    assert.notEqual(first.env.TAILSYNC_DATA_DIR, second.env.TAILSYNC_DATA_DIR);
    first.cleanup();
    assert.equal(existsSync(second.env.TAILSYNC_DATA_DIR), true);
    assert.equal(inherited.TAILSYNC_DATA_DIR, '/live-data');
  } finally {
    first.cleanup();
    second.cleanup();
  }
});
