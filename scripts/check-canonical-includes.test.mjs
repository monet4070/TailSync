import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, dirname, relative } from 'node:path';
import { canonicalIncludes, checkCanonicalIncludes } from './check-canonical-includes.mjs';
function fixture() {
  const root = mkdtempSync(resolve(tmpdir(), 'tailsync-includes-'));
  for (const [wrapper, shared] of canonicalIncludes) {
    const target = resolve(root, 'shared', shared); mkdirSync(dirname(target), {recursive:true}); writeFileSync(target, 'pub fn canonical() {}');
    for (const platform of ['macos','windows']) {
      const file = resolve(root, platform, 'src-tauri/src', wrapper); mkdirSync(dirname(file), {recursive:true});
      writeFileSync(file, `include!("${relative(dirname(file), target)}");\n`);
    }
  }
  for (const platform of ['macos','windows']) writeFileSync(resolve(root, platform, 'src-tauri/src/lib.rs'), 'mod clipboard_composite;\n');
  return root;
}
function withFixture(body) { const root = fixture(); try { body(root); } finally { rmSync(root, {recursive:true, force:true}); } }
test('valid canonical graph passes', () => withFixture(root => assert.deepEqual(checkCanonicalIncludes(root), [])));
for (const [wrapper, shared] of canonicalIncludes) for (const platform of ['macos','windows']) {
  test(`removing ${platform}/${wrapper} fails`, () => withFixture(root => {
    const file = resolve(root, platform, 'src-tauri/src', wrapper);
    writeFileSync(file, `// ${readFileSync(file, 'utf8')}`);
    assert(checkCanonicalIncludes(root).some(e => e.includes(`${platform}/${wrapper}`)));
  }));
}
for (const platform of ['macos','windows']) test(`removing ${platform} composite registration fails`, () => withFixture(root => {
  writeFileSync(resolve(root, platform, 'src-tauri/src/lib.rs'), '// mod clipboard_composite;');
  assert(checkCanonicalIncludes(root).some(e => e.includes(`${platform}:`)));
}));
test('missing canonical source fails', () => withFixture(root => {
  rmSync(resolve(root,'shared/platform-clipboard-composite.rs'));
  assert(checkCanonicalIncludes(root).some(e => e.includes('missing canonical source')));
}));
test('wrong canonical target fails', () => withFixture(root => {
  writeFileSync(resolve(root,'windows/src-tauri/src/clipboard_composite.rs'), 'include!("../../../shared/platform-updates.rs");');
  assert(checkCanonicalIncludes(root).some(e => e.includes('windows/clipboard_composite')));
}));
