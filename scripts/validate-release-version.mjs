import { execFileSync } from 'node:child_process';
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

function cargoPackageVersion(root, relativeManifest) {
  const manifest = resolve(root, relativeManifest);
  const metadata = JSON.parse(execFileSync(
    'cargo',
    ['metadata', '--locked', '--no-deps', '--format-version', '1', '--manifest-path', manifest],
    { cwd: root, encoding: 'utf8' },
  ));
  const packageInfo = metadata.packages.find(
    (candidate) => resolve(candidate.manifest_path) === manifest,
  );
  if (!packageInfo) fail(`Cargo metadata did not contain ${relativeManifest}`);
  return packageInfo.version;
}

export function validateVersions(tag, versions) {
  const match = /^v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/.exec(tag);
  if (!match) fail(`Release tag must be a semantic version prefixed with v: ${tag}`);
  const expected = match[1];
  const mismatches = Object.entries(versions)
    .filter(([, version]) => version !== expected)
    .map(([source, version]) => `${source}=${version}`);
  if (mismatches.length) {
    fail(`Release ${tag} does not match application versions: ${mismatches.join(', ')}`);
  }
  return expected;
}

export function releaseChannel(tag) {
  const match = /^v\d+\.\d+\.\d+(?:-([0-9A-Za-z.-]+))?$/.exec(tag);
  if (!match) fail(`Release tag must be a semantic version prefixed with v: ${tag}`);
  return match[1] ? 'prerelease' : 'stable';
}

export function validateRepositoryVersions(root, tag) {
  const versions = {
    'windows/tauri.conf.json': readJson(resolve(root, 'windows/src-tauri/tauri.conf.json')).version,
    'macos/tauri.conf.json': readJson(resolve(root, 'macos/src-tauri/tauri.conf.json')).version,
    'windows/Cargo.toml': cargoPackageVersion(root, 'windows/src-tauri/Cargo.toml'),
    'macos/Cargo.toml': cargoPackageVersion(root, 'macos/src-tauri/Cargo.toml'),
    'shared/Cargo.toml': cargoPackageVersion(root, 'shared/rust-core/Cargo.toml'),
  };
  return validateVersions(tag, versions);
}

function main() {
  const root = resolve(option('--root') ?? '.');
  const tag = option('--tag');
  if (!tag) fail('Usage: validate-release-version.mjs --tag vX.Y.Z [--root PATH]');
  const version = validateRepositoryVersions(root, tag);
  const channel = releaseChannel(tag);
  process.stdout.write(
    `Release version ${version} is consistent across all manifests (${channel} channel).\n`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
