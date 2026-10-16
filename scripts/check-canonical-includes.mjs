#!/usr/bin/env node
import { readFileSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const canonicalIncludes = [
  ['clipboard_composite.rs', 'platform-clipboard-composite.rs'],
  ['clipboard.rs', 'platform-clipboard-archive-sync.rs'],
  ['clipboard.rs', 'platform-clipboard-capture-terminal.rs'],
  ['clipboard.rs', 'platform-clipboard-capture-publish.rs'],
  ['clipboard.rs', 'platform-clipboard-capture-tests.rs'],
  ['automatic_images.rs', 'platform-automatic-images.rs'],
  ['clipboard/transfer.rs', 'platform-clipboard-transfer.rs'],
  ['clipboard/tests.rs', 'platform-clipboard-tests.rs'],
  ['network/server.rs', 'platform-network-server.rs'],
  ['network/pool.rs', 'platform-network-pool.rs'],
  ['network/iroh.rs', 'platform-network-iroh.rs'],
  ['network/peer_cache.rs', 'platform-peer-cache-tests.rs'],
  ['network/mod.rs', 'platform-pairing.rs'],
  ['network/tests.rs', 'platform-pairing-tests.rs'],
  ['api/tests.rs', 'platform-progress-measurement-tests.rs'],
  ['updates.rs', 'platform-updates.rs'],
];
// Mask Rust comments and literals while preserving offsets. Read an include's
// path from the original source only after its macro token is known to be code.
function codeOnly(text) {
  const masked = text.split('');
  function mask(start, end) {
    for (let j = start; j < end; j++) if (masked[j] !== '\n') masked[j] = ' ';
    return end;
  }
  let i = 0;
  while (i < text.length) {
    if (text.startsWith('//', i)) {
      const end = text.indexOf('\n', i);
      i = mask(i, end < 0 ? text.length : end);
    } else if (text.startsWith('/*', i)) {
      let end = i + 2, depth = 1;
      while (end < text.length && depth > 0) {
        if (text.startsWith('/*', end)) { depth++; end += 2; }
        else if (text.startsWith('*/', end)) { depth--; end += 2; }
        else end++;
      }
      i = mask(i, end);
    } else {
      const raw = (i === 0 || !/[\w]/.test(text[i - 1]))
        && /^(?:br|cr|r)(#{0,255})"/.exec(text.slice(i));
      const character = text[i] === "'"
        && /^'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\r\n])'/u.exec(text.slice(i));
      if (raw) {
        const closing = '"' + raw[1];
        const end = text.indexOf(closing, i + raw[0].length);
        i = mask(i, end < 0 ? text.length : end + closing.length);
      } else if (character) {
        i = mask(i, i + character[0].length);
      } else if (text[i] === '"') {
        let end = i + 1;
        while (end < text.length) {
          if (text[end] === '\\') end = Math.min(end + 2, text.length);
          else if (text[end++] === '"') break;
        }
        i = mask(i, end);
      } else i++;
    }
  }
  return masked.join('');
}
export function checkCanonicalIncludes(root) {
  const errors = [];
  for (const [wrapper, shared] of canonicalIncludes) {
    const target = resolve(root, 'shared', shared);
    if (!existsSync(target) || !readFileSync(target, 'utf8').trim()) errors.push(`missing canonical source: shared/${shared}`);
    for (const platform of ['macos', 'windows']) {
      const file = resolve(root, platform, 'src-tauri/src', wrapper);
      if (!existsSync(file)) { errors.push(`missing wrapper: ${platform}/${wrapper}`); continue; }
      const source = readFileSync(file, 'utf8');
      const code = codeOnly(source);
      const includes = [...source.matchAll(/\binclude!\s*\(\s*"([^"]+)"\s*\)\s*;/g)]
        .filter((m) => code.startsWith('include!', m.index));
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
