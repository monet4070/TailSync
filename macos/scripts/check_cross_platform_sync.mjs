import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

function fail(message) {
  throw new Error(message);
}

function option(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

const scriptRoot = dirname(fileURLToPath(import.meta.url));
const currentRoot = resolve(scriptRoot, '..');
const parentRoot = dirname(currentRoot);
const currentIsMac = existsSync(join(currentRoot, 'swift-ui')) &&
  existsSync(join(currentRoot, 'build-mac.sh'));
const winRoot = resolve(option('--win-root') ??
  (currentIsMac ? join(parentRoot, 'tailsync-v2-win') : currentRoot));
const macRoot = resolve(option('--mac-root') ??
  (currentIsMac ? currentRoot : join(parentRoot, 'tailsync-v2-mac-1')));

if (winRoot.toLowerCase() === macRoot.toLowerCase()) {
  fail('Windows and macOS roots must be different directories.');
}
for (const root of [winRoot, macRoot]) {
  for (const marker of ['package.json', 'src-tauri/Cargo.toml']) {
    if (!existsSync(join(root, marker))) fail(`Not a TailSync project root: ${root} (missing ${marker})`);
  }
}
for (const marker of ['swift-ui', 'build-mac.sh', 'src-tauri/Info.plist']) {
  if (!existsSync(join(macRoot, marker))) fail(`Not the TailSync macOS project root: ${macRoot} (missing ${marker})`);
}

function hash(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

function assertFileMatch(relativePath) {
  const winPath = join(winRoot, relativePath);
  const macPath = join(macRoot, relativePath);
  if (!existsSync(winPath) || !existsSync(macPath)) fail(`Shared file missing: ${relativePath}`);
  if (hash(winPath) !== hash(macPath)) fail(`Shared file drift detected: ${relativePath}`);
}

function treeFiles(root, relativeDirectory) {
  const base = join(root, relativeDirectory);
  if (!existsSync(base) || !statSync(base).isDirectory()) fail(`Shared directory missing: ${base}`);
  const files = new Map();
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (entry.name.startsWith('._')) continue;
      const path = join(directory, entry.name);
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile()) files.set(relative(base, path).replaceAll('\\', '/'), path);
    }
  };
  visit(base);
  return files;
}

function assertTreeMatch(relativeDirectory, allowedDrift = []) {
  const allowed = new Set(allowedDrift);
  const winFiles = treeFiles(winRoot, relativeDirectory);
  const macFiles = treeFiles(macRoot, relativeDirectory);
  const allFiles = [...new Set([...winFiles.keys(), ...macFiles.keys()])].sort();
  const drift = allFiles.filter((name) => !allowed.has(name) &&
    (!winFiles.has(name) || !macFiles.has(name) ||
      hash(winFiles.get(name)) !== hash(macFiles.get(name))));
  if (drift.length) fail(`Shared tree drift detected in ${relativeDirectory}: ${drift.join(', ')}`);
}

assertTreeMatch('src', [
  'App.tsx',
  'index.css',
  'landing.css',
  'main.tsx',
  'pages/History.tsx',
  'pages/Settings.tsx',
]);
assertTreeMatch('src-tauri/src', [
  'api.rs',
  'clipboard.rs',
  'clipboard_change.rs',
  'clipboard_file.rs',
  'commands.rs',
  'crypto.rs',
  'lib.rs',
  'network/lan.rs',
  'network/mdns.rs',
  'network/mod.rs',
  'network/tailscale.rs',
  'sync.rs',
  'tray.rs',
]);
for (const path of [
  '.oxlintrc.json',
  'package-lock.json',
  'tsconfig.json',
  'tsconfig.app.json',
  'tsconfig.node.json',
  'vite.config.ts',
  'src-tauri/build.rs',
  'src-tauri/examples/interop_probe.rs',
  'scripts/check_cross_platform_sync.mjs',
  'scripts/check_cross_platform_sync.ps1',
  'scripts/test_cross_project_interop.ps1',
  'docs/CROSS_PLATFORM_SYNC.md',
]) assertFileMatch(path);

function read(root, path) {
  return readFileSync(join(root, path), 'utf8');
}

function normalizedJson(value) {
  if (Array.isArray(value)) return value.map(normalizedJson);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.keys(value).sort()
      .map((key) => [key, normalizedJson(value[key])]));
  }
  return value;
}

function assertJsonMatch(description, expected, actual) {
  if (JSON.stringify(normalizedJson(expected)) !== JSON.stringify(normalizedJson(actual))) {
    fail(`${description} drift detected.`);
  }
}

const winPackage = JSON.parse(read(winRoot, 'package.json'));
const macPackage = JSON.parse(read(macRoot, 'package.json'));
const { scripts: winScripts = {}, ...winPackageMetadata } = winPackage;
const { scripts: macScripts = {}, ...macPackageMetadata } = macPackage;
assertJsonMatch('Package metadata/dependencies', winPackageMetadata, macPackageMetadata);
const sharedScripts = (scripts) => Object.fromEntries(Object.entries(scripts)
  .filter(([name]) => !name.startsWith('tauri:build:mac')));
assertJsonMatch('Shared package scripts', sharedScripts(winScripts), sharedScripts(macScripts));
if (macScripts['tauri:build:mac'] !== './build-mac.sh' ||
    macScripts['tauri:build:mac:dmg'] !== './build-dmg.sh') {
  fail('macOS package scripts must use build-mac.sh and build-dmg.sh.');
}

function constant(source, pattern, description) {
  const match = source.match(pattern);
  if (!match) fail(`Could not find ${description}.`);
  return Number(match[1]);
}

const winPeerPort = constant(read(winRoot, 'src-tauri/src/network/mod.rs'),
  /pub const TCP_PORT: u16 = (\d+);/, 'Windows peer TCP port');
const macPeerPort = constant(read(macRoot, 'src-tauri/src/network/mod.rs'),
  /pub const TCP_PORT: u16 = (\d+);/, 'macOS peer TCP port');
const winApiPort = constant(read(winRoot, 'src-tauri/src/api.rs'),
  /pub const API_PORT: u16 = (\d+);/, 'Windows daemon API port');
const macApiSource = read(macRoot, 'src-tauri/src/api.rs');
const macApiPort = constant(macApiSource,
  /pub const API_PORT: u16 = (\d+);/, 'macOS daemon API port');
const swiftSource = read(macRoot, 'swift-ui/Sources/TailSync/Services/ApiClient.swift');
const swiftApiPort = constant(swiftSource,
  /private let port: UInt16 = (\d+)/, 'SwiftUI API port');
if (winPeerPort !== 19890 || macPeerPort !== 19890) {
  fail(`Peer TCP port must be 19890 (Windows=${winPeerPort}, macOS=${macPeerPort}).`);
}
if (winApiPort !== 19889 || macApiPort !== 19889 || swiftApiPort !== 19889) {
  fail(`Local API port must be 19889 (Windows=${winApiPort}, macOS=${macApiPort}, SwiftUI=${swiftApiPort}).`);
}

const rustCommands = new Set();
for (const line of macApiSource.split(/\r?\n/)) {
  if (/^        "[a-z][a-z0-9_]*"( \| "[a-z][a-z0-9_]*")* =>/.test(line)) {
    for (const match of line.matchAll(/"([a-z][a-z0-9_]*)"/g)) rustCommands.add(match[1]);
  }
}
const swiftCommands = new Set([...swiftSource.matchAll(/"cmd"\s*:\s*"([a-z][a-z0-9_]*)"/g)]
  .map((match) => match[1]));
if (!rustCommands.size || !swiftCommands.size) fail('Could not extract the Rust or SwiftUI API command contract.');
const missingCommands = [...swiftCommands].filter((command) => !rustCommands.has(command));
if (missingCommands.length) fail(`SwiftUI calls commands missing from the Rust API: ${missingCommands.join(', ')}`);

function structBody(source, name, language) {
  const marker = language === 'rust' ? `pub struct ${name}` : `struct ${name}`;
  const start = source.indexOf(marker);
  if (start < 0) fail(`Could not find ${language} struct ${name}.`);
  const open = source.indexOf('{', start);
  let depth = 0;
  for (let index = open; index < source.length; index += 1) {
    if (source[index] === '{') depth += 1;
    if (source[index] === '}' && --depth === 0) return source.slice(open + 1, index);
  }
  fail(`Could not parse ${language} struct ${name}.`);
}

function rustFields(source, name) {
  const fields = new Set();
  let rename;
  for (const line of structBody(source, name, 'rust').split(/\r?\n/)) {
    const renameMatch = line.match(/#\[serde\(rename\s*=\s*"([a-z][a-z0-9_]*)"\)\]/);
    if (renameMatch) {
      rename = renameMatch[1];
      continue;
    }
    const fieldMatch = line.match(/pub\s+([a-z][a-z0-9_]*)\s*:/);
    if (fieldMatch) {
      fields.add(rename ?? fieldMatch[1]);
      rename = undefined;
    }
  }
  return fields;
}

function swiftFields(source, name) {
  const fields = new Set();
  let depth = 0;
  for (const line of structBody(source, name, 'swift').split(/\r?\n/)) {
    if (depth === 0) {
      const match = line.match(/^\s*(?:let|var)\s+([a-z][a-z0-9_]*)\s*:/);
      if (match && !line.slice(match.index + match[0].length).includes('{')) {
        fields.add(match[1]);
      }
    }
    for (const character of line) {
      if (character === '{') depth += 1;
      if (character === '}') depth -= 1;
    }
  }
  return fields;
}

function assertSameFields(description, expected, actual) {
  const missing = [...expected].filter((field) => !actual.has(field));
  const extra = [...actual].filter((field) => !expected.has(field));
  if (missing.length || extra.length) {
    fail(`${description} field drift (missing=${missing.join(',') || 'none'}, extra=${extra.join(',') || 'none'}).`);
  }
}

const rustSettingsSource = read(macRoot, 'src-tauri/src/crypto.rs');
const swiftSettingsSource = read(macRoot, 'swift-ui/Sources/TailSync/Models/Settings.swift');
assertSameFields('SwiftUI/Rust settings', rustFields(rustSettingsSource, 'Settings'),
  swiftFields(swiftSettingsSource, 'AppSettings'));

const pairingSource = read(macRoot, 'src-tauri/src/pairing.rs');
assertSameFields('SwiftUI/Rust pairing status', rustFields(pairingSource, 'PairingStatus'),
  swiftFields(swiftSource, 'PairingStatus'));
assertSameFields('SwiftUI/Rust pairing peer', rustFields(pairingSource, 'PairingPeerStatus'),
  swiftFields(swiftSource, 'PairingPeerStatus'));

const rustHistoryFields = rustFields(read(macRoot, 'src-tauri/src/db.rs'), 'HistoryEntry');
const swiftHistoryFields = swiftFields(
  read(macRoot, 'swift-ui/Sources/TailSync/Models/HistoryEntry.swift'), 'HistoryEntry');
assertSameFields('SwiftUI/Rust history entry', rustHistoryFields, swiftHistoryFields);

function assertJsonFields(description, source, fields) {
  const missing = fields.filter((field) => !new RegExp(`"${field}"\\s*:`).test(source));
  if (missing.length) fail(`${description} is missing fields used by SwiftUI: ${missing.join(', ')}`);
}
assertJsonFields('Local device snapshot', macApiSource,
  ['hostname', 'tailscale_ip', 'connection_mode', 'public_key', 'fingerprint']);
assertJsonFields('Daemon status', macApiSource,
  ['tcp_server_healthy', 'clipboard_monitor_healthy', 'active_routes']);
assertJsonFields('File progress', macApiSource, ['name', 'sent', 'total', 'active']);
assertJsonFields('Image thumbnail', macApiSource, ['width', 'height', 'rgba_b64']);

const peerInfoFields = rustFields(read(macRoot, 'src-tauri/src/network/tailscale.rs'), 'PeerInfo');
const swiftPeerFields = swiftFields(swiftSource, 'PeerSnapshot');
swiftPeerFields.delete('id');
assertSameFields('SwiftUI/Rust peer snapshot', peerInfoFields, swiftPeerFields);

const infoPlist = read(macRoot, 'src-tauri/Info.plist');
if (!/<key>NSLocalNetworkUsageDescription<\/key>\s*<string>[^<]+<\/string>/s.test(infoPlist) ||
    !/<key>NSBonjourServices<\/key>\s*<array>.*?<string>_tailsync\._tcp<\/string>/s.test(infoPlist)) {
  fail('src-tauri/Info.plist does not declare the local-network permission and _tailsync._tcp service.');
}
const macBuild = read(macRoot, 'build-mac.sh');
for (const pattern of [
  /<string>_tailsync\._tcp<\/string>/,
  /PlistBuddy.*NSLocalNetworkUsageDescription/,
  /PlistBuddy.*NSBonjourServices:0.*_tailsync\._tcp/,
]) if (!pattern.test(macBuild)) fail('build-mac.sh does not package and verify the local-network permission contract.');

const clipboardHelper = read(macRoot, 'src-tauri/clipboard-helper.swift');
if (!/--write-files/.test(clipboardHelper) || !/writeObjects\(urls\)/.test(clipboardHelper)) {
  fail('macOS clipboard helper does not support self-contained file URL restoration.');
}
const macApiSourceForClipboard = read(macRoot, 'src-tauri/src/api.rs');
if (!/clipboard_file::write_clipboard_files/.test(macApiSourceForClipboard) ||
    /Command::new\("swift"\)/.test(macApiSourceForClipboard)) {
  fail('macOS file restoration must use the packaged clipboard helper, not the Swift toolchain.');
}

const macVerifierPath = join(macRoot, 'scripts/verify_macos_release.sh');
if (!existsSync(macVerifierPath)) fail('Missing macOS release verification script: scripts/verify_macos_release.sh');
const macVerifier = readFileSync(macVerifierPath, 'utf8');
for (const pattern of [
  /npm ci/,
  /cargo test .*--lib/,
  /cargo clippy .*--lib.*-D warnings/,
  /swift build .*--package-path swift-ui/,
  /\.\/build-mac\.sh/,
  /codesign --verify --deep --strict/,
  /19889/,
  /19890/,
  /get_version/,
]) if (!pattern.test(macVerifier)) fail(`macOS release verifier is missing required check: ${pattern}`);

console.log(`Cross-platform contract passed: shared UI/backend/dependencies, ${swiftCommands.size} Swift API commands, Swift JSON models, TCP 19890, API 19889, and macOS release requirements.`);
