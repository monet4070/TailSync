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

// Shared SVG preview policy fixtures drive the macOS (XCTest) and Windows
// (vitest) trust-gate suites; both platforms must consume the same file, so
// verify it exists, parses, and carries the required sections.
{
  const fixturePath = join(dirname(sharedCoreRoot), 'svg-preview-policy-fixtures.json');
  if (!existsSync(fixturePath)) fail(`Missing shared SVG preview policy fixtures: ${fixturePath}`);
  let fixtures;
  try {
    fixtures = JSON.parse(readFileSync(fixturePath, 'utf8'));
  } catch (error) {
    fail(`Shared SVG preview policy fixtures are not valid JSON: ${error}`);
  }
  for (const section of ['trustEligibility', 'referenceExtraction', 'visualEligibility', 'csp']) {
    if (!Array.isArray(fixtures[section]) || fixtures[section].length === 0) {
      fail(`SVG preview policy fixtures section '${section}' must be a non-empty array`);
    }
  }
  for (const entry of fixtures.trustEligibility) {
    if (typeof entry.url !== 'string' || typeof entry.eligible !== 'boolean') {
      fail(`Malformed trustEligibility fixture: ${JSON.stringify(entry)}`);
    }
  }
  for (const entry of fixtures.csp) {
    if (typeof entry.expectedCSP !== 'string' || !entry.expectedCSP.includes("default-src 'none'")) {
      fail(`CSP fixture must pin a default-src 'none' policy: ${JSON.stringify(entry)}`);
    }
  }
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
  'api/routes/history.rs',
  'api/routes/peers.rs',
  'api/routes/settings.rs',
  'api/routes/theme.rs',
  'api/tests.rs',
  'clipboard/transfer.rs',
  'commands/history.rs',
  'commands/peers.rs',
  'commands/platform.rs',
  'commands/preview.rs',
  'commands/settings.rs',
  'commands/storage.rs',
  'commands/tests.rs',
  'commands/themes.rs',
  'network/tests.rs',
  'api/transport.rs',
  'clipboard.rs',
  'clipboard_change.rs',
  'clipboard_file.rs',
  'commands.rs',
  'lib.rs',
  'sync_adapter.rs',
  'network/lan.rs',
  'network/mdns.rs',
  'network/mod.rs',
  'network/health.rs',
  'network/peer_cache.rs',
  'network/tailscale.rs',
  'preview_window.rs',
  'tray.rs',
  'window_lifecycle.rs',
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

// Peer discovery/health/delivery types moved into the shared core: platform
// files must be pure re-export shims so the shared contract cannot drift.
// The checks strip comments and anchor on a statement start so a commented
// `// pub use ...` can never satisfy them.
function isReExportShim(source, corePath, required) {
  const code = source
    .split(/\r?\n/)
    .filter((line) => !line.trim().startsWith('//'))
    .join('\n');
  const statement = new RegExp(
    `^\\s*pub use ${corePath.replaceAll('.', '\\.')}::(?:\\{[^}]*\\b${required}\\b|\\b${required}\\b)`,
    'm',
  );
  return statement.test(code);
}
// Negative self-check: a commented re-export must not pass.
if (isReExportShim('// pub use tailsync_core::peer::types::{PeerInfo}', 'tailsync_core::peer::types', 'PeerInfo')) {
  fail('Drift-check shim detector accepted a commented-out re-export.');
}
for (const [root, label] of [[winRoot, 'Windows'], [macRoot, 'macOS']]) {
  const typesSource = read(root, 'src-tauri/src/network/types.rs');
  if (!isReExportShim(typesSource, 'tailsync_core::peer::types', 'PeerStatus')) {
    fail(`${label} network/types.rs must re-export the shared peer types from tailsync_core.`);
  }
  const tailscaleSource = read(root, 'src-tauri/src/network/tailscale.rs');
  if (!isReExportShim(tailscaleSource, 'tailsync_core::peer::types', 'PeerInfo')) {
    fail(`${label} network/tailscale.rs must re-export PeerInfo from tailsync_core.`);
  }
  const rateLimitSource = read(root, 'src-tauri/src/network/rate_limit.rs');
  if (!isReExportShim(rateLimitSource, 'tailsync_core::peer::rate_limit', 'check_peer_event_budget')) {
    fail(`${label} network/rate_limit.rs must re-export check_peer_event_budget from tailsync_core.`);
  }
  // The UDP discovery responder is platform I/O with allowed drift, but
  // every reply must pass the shared DiscoveryAdmission gate (2026-08
  // audit follow-up): a platform that stops consulting the core policy
  // silently loses the source filter and reply budgets.
  const lanSource = read(root, 'src-tauri/src/network/lan.rs');
  const lanCode = lanSource
    .split(/\r?\n/)
    .filter((line) => !line.trim().startsWith('//'))
    .join('\n');
  if (!/\bDiscoveryAdmission\b/.test(lanCode) || !/should_reply\s*\(/.test(lanCode)) {
    fail(
      `${label} network/lan.rs must gate UDP discovery replies through ` +
        'tailsync_core::peer::discovery_admission::DiscoveryAdmission::should_reply.',
    );
  }
}

function assertReceivedFileHistorySource(root, platform) {
  const syncAdapter = read(root, 'src-tauri/src/sync_adapter.rs');
  const storesAuthenticatedDevice = /let\s+([A-Za-z_]\w*)\s*=\s*device\.clone\(\)\s*;[\s\S]*?add_file_batch_with_status\(\s*&history_batch_id,\s*&history_files,\s*batch_total,\s*&\1,\s*true,\s*batch_complete,\s*\)/;
  if (!storesAuthenticatedDevice.test(syncAdapter)) {
    fail(`${platform} received file history must store the authenticated device name.`);
  }
}

assertReceivedFileHistorySource(winRoot, 'Windows');
assertReceivedFileHistorySource(macRoot, 'macOS');

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
const macApiTransportSource = read(macRoot, 'src-tauri/src/api/transport.rs');
const macApiRoutesSource = [
  read(macRoot, 'src-tauri/src/api/routes.rs'),
  ...treeFiles(macRoot, 'src-tauri/src/api/routes').values().map((path) => readFileSync(path, 'utf8')),
].join('\n');
const macApiContractSource = `${macApiSource}\n${macApiRoutesSource}`;
const winApiRoutesSource = [
  read(winRoot, 'src-tauri/src/api/routes.rs'),
  ...treeFiles(winRoot, 'src-tauri/src/api/routes').values().map((path) => readFileSync(path, 'utf8')),
].join('\n');
const winApiContractSource = `${read(winRoot, 'src-tauri/src/api.rs')}\n${winApiRoutesSource}`;
const swiftApiDirectory = join(macRoot, 'swift-ui/Sources/TailSync/Services');
const swiftSource = readdirSync(swiftApiDirectory)
  .filter((name) => /^ApiClient.*\.swift$/.test(name))
  .sort()
  .map((name) => readFileSync(join(swiftApiDirectory, name), 'utf8'))
  .join('\n');
const swiftAppSource = read(macRoot, 'swift-ui/Sources/TailSync/TailSyncApp.swift');
if (/environment\["TAILSYNC_API_TOKEN"\]\s*=/.test(swiftAppSource) ||
    !/TAILSYNC_API_TOKEN_STDIN/.test(swiftAppSource) ||
    !/standardInput\s*=\s*tokenPipe/.test(swiftAppSource) ||
    !/TAILSYNC_API_TOKEN_STDIN/.test(macApiSource)) {
  fail('macOS must pass the local API token through the daemon stdin pipe, not its environment.');
}
if (winPeerPort !== 19890 || macPeerPort !== 19890) {
  fail(`Peer TCP port must be 19890 (Windows=${winPeerPort}, macOS=${macPeerPort}).`);
}
if (winApiPort !== 19889) {
  fail(`Windows local API port must be 19889 (Windows=${winApiPort}).`);
}
for (const [description, source, markers] of [
  ['macOS API transport', macApiTransportSource, ['UnixListener', 'LOCAL_PEERPID', 'TAILSYNC_API_SOCKET']],
  ['SwiftUI API client', swiftSource, ['AF_UNIX', 'LOCAL_PEERPID', 'TAILSYNC_API_SOCKET']],
]) {
  const missing = markers.filter((marker) => !source.includes(marker));
  if (missing.length) fail(`${description} is missing Unix-socket security markers: ${missing.join(', ')}`);
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
    if (type === 'Option<String>') return ['string', 'null'];
    if (type.includes('HashMap<')) return 'object';
  } else {
    if (type === 'Bool') return 'boolean';
    if (type === 'Int' || type === 'UInt64') return 'integer';
    if (type === 'String') return 'string';
    if (type === 'String?') return ['string', 'null'];
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
    const contractType = contractJsonType(type, language);
    const normalize = (value) => (Array.isArray(value) ? [...value].sort() : [value]).join('|');
    if (normalize(contractType) !== normalize(schemaType)) {
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
const requiredProgressFields = ['name', 'sent', 'total', 'active', 'batch_id', 'device',
  'completed_files', 'total_files', 'speed_bytes_per_second', 'can_stop'];
const rustProgressFields = rustFields(macApiSource, 'FileProgress');
const missingProgressFields = requiredProgressFields.filter((field) => !rustProgressFields.has(field));
if (missingProgressFields.length) {
  fail(`File progress is missing fields used by SwiftUI: ${missingProgressFields.join(', ')}`);
}
const missingSwiftProgressFields = requiredProgressFields.filter((field) =>
  !new RegExp(`data\\["${field}"\\]`).test(swiftSource));
if (missingSwiftProgressFields.length) {
  fail(`SwiftUI file progress decoder is missing fields: ${missingSwiftProgressFields.join(', ')}`);
}
assertJsonFields('Image thumbnail', macApiContractSource, ['width', 'height', 'rgba_b64']);

const peerInfoFields = rustFields(readCore('src/peer/types.rs'), 'PeerInfo');
peerInfoFields.add('routes');
const swiftPeerFields = swiftFields(swiftSource, 'PeerSnapshot');
swiftPeerFields.delete('id');
assertSameFields('SwiftUI/Rust peer snapshot', peerInfoFields, swiftPeerFields);
const routeFields = ['interface', 'address', 'status', 'online', 'connected',
  'latency_ms', 'pairing_endpoint', 'rtt_capable'];
// The route row shape is defined once by the shared PeerRouteSnapshot
// struct; both API contracts and the Swift DTO must stay in lockstep.
assertSameFields('Peer route snapshot vs core struct', new Set(routeFields),
  rustFields(readCore('src/peer/types.rs'), 'PeerRouteSnapshot'));
// The serialized field names now live in the shared struct, so the API
// contracts must reference it (compile-time guarantee) rather than spell
// out JSON literals that can drift.
for (const [root, label] of [[winRoot, 'Windows'], [macRoot, 'macOS']]) {
  const routesSource = read(root, 'src-tauri/src/api/routes.rs');
  if (!routesSource.includes('tailsync_core::peer::types::PeerRouteSnapshot')) {
    fail(`${label} api routes must serialize routes via the shared PeerRouteSnapshot.`);
  }
}
for (const marker of [
  /struct Route: Decodable/,
  /case latencyMs = "latency_ms"/,
  /case pairingEndpoint = "pairing_endpoint"/,
  /case rttCapable = "rtt_capable"/,
  /rttCapable\s*=\s*try\s+values\.decodeIfPresent\(Bool\.self,\s*forKey:\s*\.rttCapable\)/,
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

const macSourceCheckPath = join(macRoot, 'scripts/check_macos_sources.sh');
const macVerifierPath = join(macRoot, 'scripts/verify_macos_bundle.sh');
if (!existsSync(macSourceCheckPath)) fail('Missing macOS source verification script.');
if (!existsSync(macVerifierPath)) fail('Missing macOS bundle verification script.');
const macSourceCheck = readFileSync(macSourceCheckPath, 'utf8');
const macVerifier = readFileSync(macVerifierPath, 'utf8');
for (const pattern of [
  /cargo test .*--all-targets/,
  /cargo clippy .*--all-targets.*-D warnings/,
  /swift build .*--package-path swift-ui/,
]) if (!pattern.test(macSourceCheck)) fail(`macOS source verifier is missing required check: ${pattern}`);
for (const pattern of [
  /codesign --verify --deep --strict/,
  /19890/,
  /-U.*TAILSYNC_API_SOCKET|TAILSYNC_API_SOCKET.*-U|tailsyncd\.sock/,
  /get_version/,
]) if (!pattern.test(macVerifier)) fail(`macOS release verifier is missing required check: ${pattern}`);

console.log(`Cross-platform contract passed: shared Rust core, ${swiftCommands.size} Swift API commands, Swift JSON models, TCP 19890, Windows API 19889, macOS Unix-socket API, and macOS release requirements.`);
