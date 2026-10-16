import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, dirname, relative } from 'node:path';
import { canonicalIncludes, checkCanonicalIncludes } from './check-canonical-includes.mjs';
function fixture() {
  const root = mkdtempSync(resolve(tmpdir(), 'tailsync-includes-'));
  const wrappers = new Map();
  for (const [wrapper, shared] of canonicalIncludes) {
    const target = resolve(root, 'shared', shared); mkdirSync(dirname(target), {recursive:true}); writeFileSync(target, 'pub fn canonical() {}');
    for (const platform of ['macos','windows']) {
      const file = resolve(root, platform, 'src-tauri/src', wrapper);
      if (!wrappers.has(file)) wrappers.set(file, []);
      wrappers.get(file).push(target);
    }
  }
  // A wrapper may carry several canonical includes (both platform clipboard
  // wrappers include the archive-sync, capture-terminal and capture-tests
  // sources), so accumulate them instead of overwriting per row.
  for (const [file, targets] of wrappers) {
    mkdirSync(dirname(file), {recursive:true});
    writeFileSync(file, targets.map(target => `include!("${relative(dirname(file), target)}");`).join('\n') + '\n');
  }
  for (const platform of ['macos','windows']) writeFileSync(resolve(root, platform, 'src-tauri/src/lib.rs'), 'mod clipboard_composite;\n');
  return root;
}
function withFixture(body) { const root = fixture(); try { body(root); } finally { rmSync(root, {recursive:true, force:true}); } }
test('valid canonical graph passes', () => withFixture(root => assert.deepEqual(checkCanonicalIncludes(root), [])));
for (const [wrapper, shared] of canonicalIncludes) for (const platform of ['macos','windows']) {
  test(`removing ${platform}/${wrapper} include of ${shared} fails`, () => withFixture(root => {
    const file = resolve(root, platform, 'src-tauri/src', wrapper);
    const target = resolve(root, 'shared', shared);
    const live = `include!("${relative(dirname(file), target)}");`;
    const source = readFileSync(file, 'utf8');
    assert(source.includes(live));
    writeFileSync(file, source.replace(live, `// ${live}`));
    assert(checkCanonicalIncludes(root).some(e => e.includes(`${platform}/${wrapper}`) && e.includes(`shared/${shared}`)));
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
test('duplicate canonical include fails', () => withFixture(root => {
  const file = resolve(root, 'macos/src-tauri/src/clipboard.rs');
  writeFileSync(file, `${readFileSync(file, 'utf8')}include!("../../../shared/platform-clipboard-capture-terminal.rs");\n`);
  assert(checkCanonicalIncludes(root).some(e => e.includes('macos/clipboard.rs')));
}));
for (const platform of ['macos', 'windows']) {
  test(`pairing include inside a raw string fails for ${platform}`, () => withFixture(root => {
    const file = resolve(root, platform, 'src-tauri/src/network/mod.rs');
    const live = readFileSync(file, 'utf8').trim();
    writeFileSync(file, `const EXAMPLE: &str = r##"${live}"##;\n`);
    assert(checkCanonicalIncludes(root).some(e => e.includes(`${platform}/network/mod.rs`)));
  }));
}
test('nested block comments cannot supply a live pairing include', () => withFixture(root => {
  const file = resolve(root, 'macos/src-tauri/src/network/mod.rs');
  const live = readFileSync(file, 'utf8');
  writeFileSync(file, `/* outer /* nested */ ${live} */\n`);
  assert(checkCanonicalIncludes(root).some(e => e.includes('macos/network/mod.rs')));
}));
test('Rust character literals and lifetimes do not hide a live include', () => withFixture(root => {
  const file = resolve(root, 'macos/src-tauri/src/network/mod.rs');
  const live = readFileSync(file, 'utf8');
  writeFileSync(file, `const QUOTE: char = '"'; const SLASH: char = '/';\nconst NAME: &'static str = "example";\n${live}`);
  assert.deepEqual(checkCanonicalIncludes(root), []);
}));
