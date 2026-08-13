import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import { generateManifest } from './generate-update-manifest.mjs';

function fixture() {
  const directory = mkdtempSync(join(tmpdir(), 'tailsync-manifest-'));
  const add = (platform, artifact) => {
    const signatureFile = `${artifact}.sig`;
    writeFileSync(join(directory, artifact), 'package');
    writeFileSync(join(directory, signatureFile), `signature-${platform}\n`);
    writeFileSync(join(directory, `release-${platform}.json`), JSON.stringify({
      schema: 1,
      product: 'TailSync',
      version: '2.2.0',
      platform,
      artifact,
      signatureFile,
    }));
  };
  add('windows-x86_64', 'TailSync-2.2.0-Windows-x64.nsis.zip');
  add('darwin-aarch64', 'TailSync-2.2.0-macOS-arm64.app.tar.gz');
  return directory;
}

test('generates a static Tauri manifest from signed platform fragments', () => {
  const directory = fixture();
  try {
    const manifest = generateManifest({
      inputDirectory: directory,
      repository: 'monet4070/TailSync',
      tag: 'v2.2.0',
      pubDate: '2026-08-12T10:00:00Z',
      notes: 'Security update',
    });
    assert.equal(manifest.version, '2.2.0');
    assert.equal(manifest.platforms['windows-x86_64'].signature, 'signature-windows-x86_64');
    assert.match(manifest.platforms['darwin-aarch64'].url, /v2\.2\.0\/TailSync-2\.2\.0/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test('refuses to publish artifacts whose signed fragment version differs from the tag', () => {
  const directory = fixture();
  try {
    assert.throws(() => generateManifest({
      inputDirectory: directory,
      repository: 'monet4070/TailSync',
      tag: 'v2.3.0',
      pubDate: '2026-08-12T10:00:00Z',
    }), /tag requires 2\.3\.0/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
