#!/usr/bin/env node

import { pathToFileURL } from 'node:url';
import { validateTauriSignature } from './generate-update-manifest.mjs';

const DEFAULT_PLATFORMS = ['windows-x86_64', 'darwin-aarch64', 'darwin-x86_64'];

function fail(message) {
  throw new Error(message);
}

function option(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function normalizeVersion(value) {
  const match = /^v?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/.exec(String(value ?? ''));
  if (!match) fail(`Invalid release version: ${value}`);
  return match[1];
}

export function validatePublishedManifest(manifest, { version, platforms = DEFAULT_PLATFORMS }) {
  const expectedVersion = normalizeVersion(version);
  if (!manifest || typeof manifest !== 'object') fail('Published updater manifest is not an object.');
  if (manifest.version !== expectedVersion) {
    fail(`Published updater manifest has version ${manifest.version}; expected ${expectedVersion}.`);
  }
  if (!manifest.platforms || typeof manifest.platforms !== 'object') {
    fail('Published updater manifest has no platforms object.');
  }

  for (const platform of platforms) {
    const entry = manifest.platforms[platform];
    if (!entry || typeof entry !== 'object') {
      fail(`Published updater manifest is missing platform ${platform}.`);
    }
    if (typeof entry.url !== 'string' || !entry.url.startsWith('https://')) {
      fail(`Published updater manifest has an invalid URL for ${platform}.`);
    }
    try {
      validateTauriSignature(entry.signature, `published ${platform}`);
    } catch (error) {
      fail(error.message);
    }
  }

  return manifest;
}

async function wait(milliseconds) {
  await new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function fetchWithRetry(url, init = {}, { attempts = 6, delayMilliseconds = 2000 } = {}) {
  let lastError = null;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      const response = await fetch(url, init);
      if (response.ok) return response;
      lastError = new Error(`HTTP ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    if (attempt < attempts) await wait(delayMilliseconds);
  }
  fail(`Could not fetch ${url}: ${lastError?.message ?? 'unknown error'}`);
}

export async function verifyPublishedUpdate({
  endpoint,
  version,
  platforms = DEFAULT_PLATFORMS,
}) {
  if (typeof endpoint !== 'string' || !endpoint.startsWith('https://')) {
    fail(`Published updater endpoint must use HTTPS: ${endpoint}`);
  }
  const response = await fetchWithRetry(endpoint, {
    headers: { accept: 'application/json' },
  });
  let manifest;
  try {
    manifest = await response.json();
  } catch (error) {
    fail(`Published updater endpoint did not return JSON: ${error.message}`);
  }

  validatePublishedManifest(manifest, { version, platforms });
  for (const platform of platforms) {
    const url = manifest.platforms[platform].url;
    await fetchWithRetry(url, { method: 'HEAD', redirect: 'follow' }, { attempts: 3 });
  }
  return manifest;
}

async function main() {
  const endpoint = option('--endpoint');
  const version = option('--version');
  const platforms = (option('--platforms') ?? DEFAULT_PLATFORMS.join(','))
    .split(',')
    .map((value) => value.trim())
    .filter(Boolean);
  if (!endpoint || !version || platforms.length === 0) {
    fail('Usage: verify-published-update.mjs --endpoint URL --version X.Y.Z [--platforms PLATFORM,...]');
  }

  await verifyPublishedUpdate({ endpoint, version, platforms });
  process.stdout.write(
    `Published updater manifest ${normalizeVersion(version)} is reachable with ${platforms.length} platform artifact(s).\n`,
  );
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  main().catch((error) => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  });
}
