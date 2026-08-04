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
const sharedCoreRoot = resolve(option('--core-root') ?? join(dirname(winRoot), 'shared/rust-core'));

if (winRoot.toLowerCase() === macRoot.toLowerCase()) {
  fail('Windows and macOS roots must be different directories.');
}
for (const [root, markers] of [
  [winRoot, ['package.json', 'src-tauri/Cargo.toml']],
  [macRoot, ['src-tauri/Cargo.toml']],
]) {
  for (const marker of markers) {
    if (!existsSync(join(root, marker))) fail(`Not a TailSync project root: ${root} (missing ${marker})`);
  }
}
for (const marker of ['swift-ui', 'build-mac.sh', 'src-tauri/Info.plist']) {
  if (!existsSync(join(macRoot, marker))) fail(`Not the TailSync macOS project root: ${macRoot} (missing ${marker})`);
}
for (const marker of ['Cargo.toml', 'src/lib.rs']) {
  if (!existsSync(join(sharedCoreRoot, marker))) {
    fail(`Not the TailSync shared core root: ${sharedCoreRoot} (missing ${marker})`);
  }
}
const settingsSchemaPath = join(dirname(sharedCoreRoot), 'schema/settings.schema.json');
if (!existsSync(settingsSchemaPath)) fail(`Missing shared Settings schema: ${settingsSchemaPath}`);

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
  const staleAllowed = [...allowed].filter((name) => !winFiles.has(name) && !macFiles.has(name));
  if (staleAllowed.length) {
    fail(`Stale allowed-drift entries in ${relativeDirectory}: ${staleAllowed.join(', ')}`);
  }
  const allFiles = [...new Set([...winFiles.keys(), ...macFiles.keys()])].sort();
  const drift = allFiles.filter((name) => !allowed.has(name) &&
    (!winFiles.has(name) || !macFiles.has(name) ||
      hash(winFiles.get(name)) !== hash(macFiles.get(name))));
  if (drift.length) fail(`Shared tree drift detected in ${relativeDirectory}: ${drift.join(', ')}`);
}

// The production UIs intentionally differ: Windows uses React/Tauri while
// macOS ships SwiftUI. Cross-platform checks below cover their shared runtime
// and serialized contracts instead of requiring frontend source parity.
assertTreeMatch('src-tauri/src', [
  'api.rs',
  'api/routes.rs',
  'clipboard.rs',
  'clipboard_change.rs',
  'clipboard_file.rs',
  'commands.rs',
  'lib.rs',
  'network/lan.rs',
  'network/mdns.rs',
  'network/mod.rs',
  'network/health.rs',
  'network/peer_cache.rs',
  'network/pool.rs',
  'network/server.rs',
  'network/tailscale.rs',
  'network/types.rs',
  'tray.rs',
]);
for (const path of [
  'src-tauri/build.rs',
  'src-tauri/examples/interop_probe.rs',
  'scripts/check_cross_platform_sync.mjs',
  'scripts/check_cross_platform_sync.ps1',
  'scripts/test_cross_project_interop.ps1',
]) assertFileMatch(path);

function read(root, path) {
  return readFileSync(join(root, path), 'utf8');
}

function readCore(path) {
  return readFileSync(join(sharedCoreRoot, path), 'utf8');
}

const winPackage = JSON.parse(read(winRoot, 'package.json'));
const winScripts = winPackage.scripts ?? {};
for (const required of ['build', 'lint', 'test']) {
  if (!winScripts[required]) fail(`Windows package is missing the ${required} script.`);
}
for (const path of ['swift-ui/Package.swift', 'build-mac.sh', 'build-dmg.sh']) {
  if (!existsSync(join(macRoot, path))) fail(`macOS native build input is missing: ${path}`);
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
const macApiRoutesSource = read(macRoot, 'src-tauri/src/api/routes.rs');
const macApiContractSource = `${macApiSource}\n${macApiRoutesSource}`;
const winApiContractSource = `${read(winRoot, 'src-tauri/src/api.rs')}\n${read(winRoot, 'src-tauri/src/api/routes.rs')}`;
const macApiPort = constant(macApiSource,
  /pub const API_PORT: u16 = (\d+);/, 'macOS daemon API port');
const swiftSource = read(macRoot, 'swift-ui/Sources/TailSync/Services/ApiClient.swift');
const swiftAppSource = read(macRoot, 'swift-ui/Sources/TailSync/TailSyncApp.swift');
if (/environment\["TAILSYNC_API_TOKEN"\]\s*=/.test(swiftAppSource) ||
    !/TAILSYNC_API_TOKEN_STDIN/.test(swiftAppSource) ||
    !/standardInput\s*=\s*tokenPipe/.test(swiftAppSource) ||
    !/TAILSYNC_API_TOKEN_STDIN/.test(macApiSource)) {
  fail('macOS must pass the local API token through the daemon stdin pipe, not its environment.');
}
const swiftApiPort = constant(swiftSource,
  /private let port: UInt16 = (\d+)/, 'SwiftUI API port');
if (winPeerPort !== 19890 || macPeerPort !== 19890) {
  fail(`Peer TCP port must be 19890 (Windows=${winPeerPort}, macOS=${macPeerPort}).`);
}
if (winApiPort !== 19889 || macApiPort !== 19889 || swiftApiPort !== 19889) {
  fail(`Local API port must be 19889 (Windows=${winApiPort}, macOS=${macApiPort}, SwiftUI=${swiftApiPort}).`);
}

const rustCommands = new Set();
for (const line of macApiRoutesSource.split(/\r?\n/)) {
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
  const marker = language === 'rust'
    ? `pub struct ${name}`
    : language === 'typescript' ? `interface ${name}` : `struct ${name}`;
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

function rustFieldTypes(source, name) {
  const body = structBody(source, name, 'rust');
  const fields = new Map();
  const pattern = /\bpub\s+([a-z][a-z0-9_]*)\s*:/g;
  for (let match; (match = pattern.exec(body));) {
    let angleDepth = 0;
    let end = pattern.lastIndex;
    for (; end < body.length; end += 1) {
      if (body[end] === '<') angleDepth += 1;
      if (body[end] === '>') angleDepth -= 1;
      if (body[end] === ',' && angleDepth === 0) break;
    }
    fields.set(match[1], body.slice(pattern.lastIndex, end).replace(/\s+/g, ' ').trim());
  }
  return fields;
}

function swiftFieldTypes(source, name) {
  const fields = new Map();
  for (const line of structBody(source, name, 'swift').split(/\r?\n/)) {
    const match = line.match(/^\s*(?:let|var)\s+([a-z][a-z0-9_]*)\s*:\s*([^={]+?)(?:\s*=|$)/);
    if (match) fields.set(match[1], match[2].trim());
  }
  return fields;
}

function typescriptFields(source, name) {
  const fields = new Set();
  for (const line of structBody(source, name, 'typescript').split(/\r?\n/)) {
    const match = line.match(/^\s*([a-z][a-z0-9_]*)\??\s*:/);
    if (match) fields.add(match[1]);
  }
  return fields;
}

function contractJsonType(type, language) {
  if (language === 'rust') {
    if (type === 'bool') return 'boolean';
    if (/^[ui]\d+$/.test(type)) return 'integer';
    if (type === 'String') return 'string';
    if (type.includes('HashMap<')) return 'object';
  } else {
    if (type === 'Bool') return 'boolean';
    if (type === 'Int') return 'integer';
    if (type === 'String') return 'string';
    if (type.startsWith('[String:')) return 'object';
  }
  fail(`Unsupported ${language} Settings type: ${type}`);
}

function assertSchemaTypes(description, schema, actual, language) {
  for (const [field, definition] of Object.entries(schema.properties)) {
    const type = actual.get(field);
    if (!type) fail(`${description} is missing Settings field ${field}`);
    const resolved = definition.allOf?.length === 1 ? definition.allOf[0] : definition;
    const referencePrefix = '#/definitions/';
    const schemaType = resolved.$ref?.startsWith(referencePrefix)
      ? schema.definitions?.[resolved.$ref.slice(referencePrefix.length)]?.type
      : resolved.type;
    if (contractJsonType(type, language) !== schemaType) {
      fail(`${description} type drift for ${field}: ${type} vs schema ${schemaType}`);
    }
  }
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

const rustSettingsSource = readCore('src/crypto.rs');
const swiftSettingsSource = read(macRoot, 'swift-ui/Sources/TailSync/Models/Settings.swift');
const settingsSchema = JSON.parse(readFileSync(settingsSchemaPath, 'utf8'));
const schemaSettingsFields = new Set(Object.keys(settingsSchema.properties));
assertSameFields('Settings schema required list', schemaSettingsFields,
  new Set(settingsSchema.required));
assertSameFields('Rust/Settings schema', schemaSettingsFields,
  rustFields(rustSettingsSource, 'Settings'));
assertSameFields('SwiftUI/Settings schema', schemaSettingsFields,
  swiftFields(swiftSettingsSource, 'AppSettings'));
const typescriptSettingsSource = read(winRoot, 'src/types/settings.generated.ts');
assertSameFields('TypeScript/Settings schema', schemaSettingsFields,
  typescriptFields(typescriptSettingsSource, 'SettingsData'));
assertSchemaTypes('Rust Settings', settingsSchema,
  rustFieldTypes(rustSettingsSource, 'Settings'), 'rust');
assertSchemaTypes('Swift Settings', settingsSchema,
  swiftFieldTypes(swiftSettingsSource, 'AppSettings'), 'swift');

const pairingSource = readCore('src/pairing.rs');
assertSameFields('SwiftUI/Rust pairing status', rustFields(pairingSource, 'PairingStatus'),
  swiftFields(swiftSource, 'PairingStatus'));
assertSameFields('SwiftUI/Rust pairing peer', rustFields(pairingSource, 'PairingPeerStatus'),
  swiftFields(swiftSource, 'PairingPeerStatus'));

const rustHistoryFields = rustFields(readCore('src/db/types.rs'), 'HistoryEntry');
const swiftHistoryFields = swiftFields(
  read(macRoot, 'swift-ui/Sources/TailSync/Models/HistoryEntry.swift'), 'HistoryEntry');
assertSameFields('SwiftUI/Rust history entry', rustHistoryFields, swiftHistoryFields);

function assertJsonFields(description, source, fields) {
  const missing = fields.filter((field) => !new RegExp(`"${field}"\\s*:`).test(source));
  if (missing.length) fail(`${description} is missing fields used by SwiftUI: ${missing.join(', ')}`);
}
assertJsonFields('Local device snapshot', macApiContractSource,
  ['hostname', 'tailscale_ip', 'connection_mode', 'public_key', 'fingerprint']);
assertJsonFields('Daemon status', macApiContractSource,
  ['tcp_server_healthy', 'clipboard_monitor_healthy', 'active_routes']);
assertJsonFields('File progress', macApiContractSource, ['name', 'sent', 'total', 'active']);
assertJsonFields('Image thumbnail', macApiContractSource, ['width', 'height', 'rgba_b64']);

const peerInfoFields = rustFields(read(macRoot, 'src-tauri/src/network/tailscale.rs'), 'PeerInfo');
peerInfoFields.add('routes');
const swiftPeerFields = swiftFields(swiftSource, 'PeerSnapshot');
swiftPeerFields.delete('id');
assertSameFields('SwiftUI/Rust peer snapshot', peerInfoFields, swiftPeerFields);
const routeFields = ['interface', 'address', 'status', 'online', 'connected',
  'latency_ms', 'pairing_endpoint'];
assertJsonFields('Windows peer route snapshot', winApiContractSource, routeFields);
assertJsonFields('macOS peer route snapshot', macApiContractSource, routeFields);
for (const marker of [
  /struct Route: Decodable/,
  /case latencyMs = "latency_ms"/,
  /case pairingEndpoint = "pairing_endpoint"/,
]) if (!marker.test(swiftSource)) fail('SwiftUI peer route DTO is missing normalized route fields.');

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
  /cargo test .*--lib/,
  /cargo clippy .*--lib.*-D warnings/,
  /swift build .*--package-path swift-ui/,
  /\.\/build-mac\.sh/,
  /codesign --verify --deep --strict/,
  /19889/,
  /19890/,
  /get_version/,
]) if (!pattern.test(macVerifier)) fail(`macOS release verifier is missing required check: ${pattern}`);

console.log(`Cross-platform contract passed: shared Rust core, ${swiftCommands.size} Swift API commands, Swift JSON models, TCP 19890, API 19889, and macOS release requirements.`);
