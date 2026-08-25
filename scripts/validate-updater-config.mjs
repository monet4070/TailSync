import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

function fail(message) {
  throw new Error(message);
}

function option(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8').replace(/^\uFEFF/, ''));
}

function normalizePublicKey(value) {
  return String(value ?? '').trim();
}

export function validateUpdaterConfiguration(expectedPublicKey, configurations) {
  const publicKey = normalizePublicKey(expectedPublicKey);
  if (!publicKey) fail('The checked-in updater public key is empty.');
  if (!/^[A-Za-z0-9+/=]+$/.test(publicKey)) {
    fail('The checked-in updater public key is not valid Base64.');
  }
  const decoded = Buffer.from(publicKey, 'base64').toString('utf8');
  if (!/^untrusted comment: minisign public key: [A-F0-9]{16}\nRW[A-Za-z0-9+/=]+\n?$/.test(decoded)) {
    fail('The checked-in updater public key is not a Tauri minisign public key.');
  }

  let expectedEndpoints;
  for (const [name, config] of Object.entries(configurations)) {
    const updater = config?.plugins?.updater;
    if (!updater) fail(`${name} has no plugins.updater configuration.`);
    if (normalizePublicKey(updater.pubkey) !== publicKey) {
      fail(`${name} updater public key does not match shared/updater.pub.`);
    }
    if (!Array.isArray(updater.endpoints) || updater.endpoints.length === 0) {
      fail(`${name} must configure at least one updater endpoint.`);
    }
    for (const endpoint of updater.endpoints) {
      if (typeof endpoint !== 'string' || !endpoint.startsWith('https://')) {
        fail(`${name} updater endpoints must use HTTPS.`);
      }
    }
    const endpoints = JSON.stringify(updater.endpoints);
    expectedEndpoints ??= endpoints;
    if (endpoints !== expectedEndpoints) {
      fail(`${name} updater endpoints do not match the other clients.`);
    }
  }
  return { publicKey, endpoints: JSON.parse(expectedEndpoints) };
}

export function validateRepositoryUpdaterConfiguration(root) {
  const publicKey = readFileSync(resolve(root, 'shared/updater.pub'), 'utf8');
  return validateUpdaterConfiguration(publicKey, {
    windows: readJson(resolve(root, 'windows/src-tauri/tauri.conf.json')),
    macos: readJson(resolve(root, 'macos/src-tauri/tauri.conf.json')),
  });
}

function main() {
  const root = resolve(option('--root') ?? '.');
  const result = validateRepositoryUpdaterConfiguration(root);
  process.stdout.write(
    `Updater trust anchor and ${result.endpoints.length} HTTPS endpoint(s) are consistent.\n`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
