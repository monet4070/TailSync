import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { pathToFileURL } from 'node:url';

function fail(message) {
  throw new Error(message);
}

function option(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, 'utf8').replace(/^\uFEFF/, ''));
  } catch (error) {
    fail(`Invalid JSON in ${path}: ${error.message}`);
  }
}

function safeReleaseName(value, description) {
  if (typeof value !== 'string' || !value || basename(value) !== value || /[\\/]/.test(value)) {
    fail(`Invalid ${description}: ${value}`);
  }
  return value;
}

export function generateManifest({ inputDirectory, repository, tag, pubDate, notes = '' }) {
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) {
    fail(`Invalid GitHub repository: ${repository}`);
  }
  const versionMatch = /^v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/.exec(tag);
  if (!versionMatch) fail(`Release tag must be a semantic version prefixed with v: ${tag}`);
  const version = versionMatch[1];
  if (Number.isNaN(Date.parse(pubDate))) fail(`Invalid publication date: ${pubDate}`);

  const fragmentNames = readdirSync(inputDirectory)
    .filter((name) => /^release-[a-z0-9_-]+\.json$/.test(name))
    .sort();
  if (!fragmentNames.length) fail(`No release fragments found in ${inputDirectory}`);

  const platforms = {};
  for (const fragmentName of fragmentNames) {
    const fragment = readJson(join(inputDirectory, fragmentName));
    if (fragment.schema !== 1 || fragment.product !== 'TailSync') {
      fail(`Unsupported release fragment: ${fragmentName}`);
    }
    if (fragment.version !== version) {
      fail(`${fragmentName} has version ${fragment.version}; tag requires ${version}`);
    }
    if (!/^(windows|darwin)-(x86_64|aarch64)$/.test(fragment.platform)) {
      fail(`Unsupported updater platform in ${fragmentName}: ${fragment.platform}`);
    }
    if (platforms[fragment.platform]) fail(`Duplicate updater platform: ${fragment.platform}`);

    const artifact = safeReleaseName(fragment.artifact, 'updater artifact name');
    const signatureFile = safeReleaseName(fragment.signatureFile, 'signature file name');
    const artifactPath = join(inputDirectory, artifact);
    const signaturePath = join(inputDirectory, signatureFile);
    if (!existsSync(artifactPath)) fail(`Missing updater artifact: ${artifact}`);
    if (!existsSync(signaturePath)) fail(`Missing updater signature: ${signatureFile}`);
    const signature = readFileSync(signaturePath, 'utf8').trim();
    if (!signature) fail(`Updater signature is empty: ${signatureFile}`);

    const url = `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(artifact)}`;
    platforms[fragment.platform] = { signature, url };
  }

  if (!platforms['windows-x86_64']) fail('A Windows x86_64 updater is required');
  if (!Object.keys(platforms).some((platform) => platform.startsWith('darwin-'))) {
    fail('At least one macOS updater is required');
  }

  return {
    version,
    notes,
    pub_date: new Date(pubDate).toISOString(),
    platforms,
  };
}

function main() {
  const inputDirectory = option('--input');
  const outputPath = option('--output');
  const repository = option('--repository');
  const tag = option('--tag');
  const pubDate = option('--pub-date') ?? new Date().toISOString();
  const notes = option('--notes') ?? '';
  if (!inputDirectory || !outputPath || !repository || !tag) {
    fail('Usage: generate-update-manifest.mjs --input DIR --output FILE --repository OWNER/REPO --tag vX.Y.Z [--pub-date ISO] [--notes TEXT]');
  }
  const manifest = generateManifest({ inputDirectory, repository, tag, pubDate, notes });
  writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
