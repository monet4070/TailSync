#!/usr/bin/env node
import { readFileSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const canonicalIncludes = [
  ['clipboard_composite.rs', 'platform-clipboard-composite.rs'],
  ['clipboard.rs', 'platform-clipboard-archive-sync.rs'],
  ['automatic_images.rs', 'platform-automatic-images.rs'],
  ['clipboard/transfer.rs', 'platform-clipboard-transfer.rs'],
  ['clipboard/tests.rs', 'platform-clipboard-tests.rs'],
  ['network/server.rs', 'platform-network-server.rs'],
  ['network/pool.rs', 'platform-network-pool.rs'],
  ['network/iroh.rs', 'platform-network-iroh.rs'],
  ['network/peer_cache.rs', 'platform-peer-cache-tests.rs'],
  ['api/tests.rs', 'platform-progress-measurement-tests.rs'],
  ['updates.rs', 'platform-updates.rs'],
];
// Strip comments before looking for a live include/module declaration. Preserve
// strings, so a comment containing the old include cannot satisfy this gate.
function codeOnly(text) {
  return text.replace(/"(?:\\.|[^"\\])*"|\/\*[\s\S]*?\*\/|\/\/[^\n]*/g,
    (part) => part.startsWith('"') ? part : part.replace(/[^\n]/g, ' '));
}
export function checkCanonicalIncludes(root) {
  const errors = [];
  for (const [wrapper, shared] of canonicalIncludes) {
    const target = resolve(root, 'shared', shared);
    if (!existsSync(target) || !readFileSync(target, 'utf8').trim()) errors.push(`missing canonical source: shared/${shared}`);
    for (const platform of ['macos', 'windows']) {
      const file = resolve(root, platform, 'src-tauri/src', wrapper);
      if (!existsSync(file)) { errors.push(`missing wrapper: ${platform}/${wrapper}`); continue; }
      const includes = [...codeOnly(readFileSync(file, 'utf8')).matchAll(/\binclude!\s*\(\s*"([^"]+)"\s*\)\s*;/g)];
      const matching = includes.filter((m) => resolve(dirname(file), m[1]) === target);
      if (matching.length !== 1) errors.push(`${platform}/${wrapper}: expected exactly one live include of shared/${shared}, found ${matching.length}`);
    }
  }
  for (const platform of ['macos', 'windows']) {
    const lib = resolve(root, platform, 'src-tauri/src/lib.rs');
    if (!existsSync(lib) || !/^\s*(?:pub\s+)?mod\s+clipboard_composite\s*;/m.test(codeOnly(readFileSync(lib, 'utf8'))))
      errors.push(`${platform}: clipboard_composite module is not registered`);
  }
  return errors;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const root = resolve(process.argv[process.argv.indexOf('--root') + 1] && process.argv.includes('--root')
    ? process.argv[process.argv.indexOf('--root') + 1] : dirname(fileURLToPath(import.meta.url)) + '/..');
  const errors = checkCanonicalIncludes(root);
  if (errors.length) { console.error(errors.join('\n')); process.exitCode = 1; }
  else console.log(`PASS: ${canonicalIncludes.length} canonical sources, both platform wrappers and composite registrations`);
}
