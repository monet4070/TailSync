import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import {
  bumpRepositoryVersions,
  verifyRepositoryVersions,
} from './bump-version.mjs';

function fixture(version = '2.1.0') {
  const root = mkdtempSync(join(tmpdir(), 'tailsync-bump-'));
  const files = {
    'README.md': [
      `# TailSync`,
      '',
      `[![Version](https://img.shields.io/badge/Version-v${version}-D5684B)](https://github.com/monet4070/TailSync/tree/v${version})`,
      '',
      `> TailSync ${version} 目前处于积极开发阶段。`,
      '',
      `当前产品版本为 ${version}，数据库 schema 为 v10。`,
      '',
    ].join('\n'),
    'CONTEXT.md': `线协议 v4；产品版本 ${version}；数据库 schema v10。详见 \`README.md\`。\n`,
    'docs/USER_GUIDE.zh-CN.md': `> 适用版本：TailSync ${version}，线协议 v4\n`,
    'docs/THEMING.md': [
      `> 适用版本：产品 ${version}，Theme V2。`,
      '',
      `| \`minCoreVersion\` | ✅ | SemVer，且 ≤ 当前 Core 版本（${version}），否则包被拒绝 |`,
      '',
    ].join('\n'),
    'windows/src-tauri/tauri.conf.json': JSON.stringify({ version }, null, 2) + '\n',
    'macos/src-tauri/tauri.conf.json': JSON.stringify({ version }, null, 2) + '\n',
    'windows/package.json': JSON.stringify({ name: 'tailsync-v2', version }, null, 2) + '\n',
    'site/package.json': JSON.stringify({ name: 'tailsync-site', version }, null, 2) + '\n',
    'windows/src-tauri/Cargo.toml':
      '[package]\nname = "tailsync"\nversion = "' + version + '"\nedition = "2021"\n\n[lib]\npath = "src/lib.rs"\n',
    'macos/src-tauri/Cargo.toml':
      '[package]\nname = "tailsync"\nversion = "' + version + '"\nedition = "2021"\n\n[lib]\npath = "src/lib.rs"\n',
    'shared/rust-core/Cargo.toml':
      '[package]\nname = "tailsync-core"\nversion = "' + version + '"\nedition = "2021"\n\n[lib]\npath = "src/lib.rs"\n',
    'shared/tailsync-protocol/Cargo.toml':
      '[package]\nname = "tailsync-protocol"\nversion = "' + version + '"\nedition = "2021"\n\n[lib]\npath = "src/lib.rs"\n',
    'shared/tailsync-themes/Cargo.toml':
      '[package]\nname = "tailsync-themes"\nversion = "' + version + '"\nedition = "2021"\n\n[lib]\npath = "src/lib.rs"\n',
    'shared/tailsync-history-classifier/Cargo.toml':
      '[package]\nname = "tailsync-history-classifier"\nversion = "' + version + '"\nedition = "2021"\n\n[lib]\npath = "src/lib.rs"\n',
    'windows/src-tauri/Cargo.lock':
      'version = 4\n\n[[package]]\nname = "tailsync"\nversion = "' + version + '"\ndependencies = ["tailsync-core"]\n\n[[package]]\nname = "tailsync-core"\nversion = "' + version + '"\n\n[[package]]\nname = "tailsync-protocol"\nversion = "' + version + '"\n\n[[package]]\nname = "tailsync-themes"\nversion = "' + version + '"\n\n[[package]]\nname = "tailsync-history-classifier"\nversion = "' + version + '"\n',
    'macos/src-tauri/Cargo.lock':
      'version = 4\n\n[[package]]\nname = "tailsync"\nversion = "' + version + '"\ndependencies = ["tailsync-core"]\n\n[[package]]\nname = "tailsync-core"\nversion = "' + version + '"\n\n[[package]]\nname = "tailsync-protocol"\nversion = "' + version + '"\n\n[[package]]\nname = "tailsync-themes"\nversion = "' + version + '"\n\n[[package]]\nname = "tailsync-history-classifier"\nversion = "' + version + '"\n',
    'Cargo.lock':
      'version = 4\n\n[[package]]\nname = "tailsync-core"\nversion = "' + version + '"\n\n[[package]]\nname = "tailsync-protocol"\nversion = "' + version + '"\n\n[[package]]\nname = "tailsync-themes"\nversion = "' + version + '"\n\n[[package]]\nname = "tailsync-history-classifier"\nversion = "' + version + '"\n',
    'windows/package-lock.json':
      JSON.stringify({
        name: 'tailsync-v2',
        version,
        lockfileVersion: 3,
        packages: { '': { name: 'tailsync-v2', version } },
      }, null, 2) + '\n',
    'site/package-lock.json':
      JSON.stringify({
        name: 'tailsync-site',
        version,
        lockfileVersion: 3,
        packages: { '': { name: 'tailsync-site', version } },
      }, null, 2) + '\n',
  };
  for (const [relative, content] of Object.entries(files)) {
    const path = join(root, relative);
    mkdirSync(join(root, relative.split('/').slice(0, -1).join('/')), { recursive: true });
    writeFileSync(path, content);
  }
  return root;
}

test('bump writes all nineteen version files and is idempotent', () => {
  const root = fixture('2.1.0');
  try {
    const written = bumpRepositoryVersions(root, '2.2.0');
    assert.equal(written.length, 19, `expected 19 files, got ${written.length}`);
    for (const relative of [
      'windows/src-tauri/tauri.conf.json',
      'macos/src-tauri/tauri.conf.json',
      'windows/package.json',
      'site/package.json',
      'shared/rust-core/Cargo.toml',
      'shared/tailsync-protocol/Cargo.toml',
      'shared/tailsync-themes/Cargo.toml',
      'shared/tailsync-history-classifier/Cargo.toml',
    ]) {
      assert.match(readFileSync(join(root, relative), 'utf8'), /2\.2\.0/);
    }
    for (const relative of [
      'windows/src-tauri/Cargo.lock',
      'macos/src-tauri/Cargo.lock',
      'Cargo.lock',
    ]) {
      const lock = readFileSync(join(root, relative), 'utf8');
      assert.match(lock, /version = "2\.2\.0"/);
      assert.doesNotMatch(lock, /version = "2\.1\.0"/);
    }
    for (const relative of ['windows/package-lock.json', 'site/package-lock.json']) {
      const lock = JSON.parse(readFileSync(join(root, relative), 'utf8'));
      assert.equal(lock.version, '2.2.0');
      assert.equal(lock.packages[''].version, '2.2.0');
    }
    for (const relative of ['README.md', 'CONTEXT.md', 'docs/USER_GUIDE.zh-CN.md', 'docs/THEMING.md']) {
      const doc = readFileSync(join(root, relative), 'utf8');
      assert.match(doc, /2\.2\.0/);
      assert.doesNotMatch(doc, /2\.1\.0/);
    }
    // A second run at the same version touches nothing.
    assert.equal(bumpRepositoryVersions(root, '2.2.0').length, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('dry-run records the would-be writes without touching the tree', () => {
  const root = fixture('2.1.0');
  try {
    const written = bumpRepositoryVersions(root, '2.2.0', true);
    assert.equal(written.length, 19);
    assert.match(readFileSync(join(root, 'windows/package.json'), 'utf8'), /2\.1\.0/);
    assert.match(readFileSync(join(root, 'README.md'), 'utf8'), /2\.1\.0/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('verify passes at the bumped version and fails at the old version', () => {
  const root = fixture('2.1.0');
  try {
    bumpRepositoryVersions(root, '2.2.0');
    assert.equal(verifyRepositoryVersions(root, '2.2.0'), 'stable');
    assert.throws(() => verifyRepositoryVersions(root, '2.1.0'));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('verify detects an out-of-sync Cargo.lock', () => {
  const root = fixture('2.1.0');
  try {
    bumpRepositoryVersions(root, '2.2.0');
    // Revert one lockfile entry behind the others.
    const lock = join(root, 'Cargo.lock');
    writeFileSync(lock, readFileSync(lock, 'utf8').replace('2.2.0', '2.1.0'));
    assert.throws(() => verifyRepositoryVersions(root, '2.2.0'));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('verify detects doc drift when a current-version marker lags behind', () => {
  const root = fixture('2.1.0');
  try {
    bumpRepositoryVersions(root, '2.2.0');
    // Revert only the README status note to the old version.
    const readme = join(root, 'README.md');
    writeFileSync(readme, readFileSync(readme, 'utf8').replace('> TailSync 2.2.0', '> TailSync 2.1.0'));
    assert.throws(() => verifyRepositoryVersions(root, '2.2.0'));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('verify rejects a missing doc version marker', () => {
  const root = fixture('2.1.0');
  try {
    bumpRepositoryVersions(root, '2.2.0');
    const guide = join(root, 'docs/USER_GUIDE.zh-CN.md');
    writeFileSync(guide, '> 适用版本：最新版本，线协议 v4\n');
    assert.throws(() => verifyRepositoryVersions(root, '2.2.0'));
    assert.throws(() => bumpRepositoryVersions(root, '2.3.0'));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('verify rejects a duplicated doc version marker', () => {
  const root = fixture('2.1.0');
  try {
    bumpRepositoryVersions(root, '2.2.0');
    const guide = join(root, 'docs/USER_GUIDE.zh-CN.md');
    const original = readFileSync(guide, 'utf8');
    writeFileSync(guide, `${original}\n${original}`);
    assert.throws(() => verifyRepositoryVersions(root, '2.2.0'));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
