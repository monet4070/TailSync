import { readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const result = spawnSync('cargo', ['run', '--quiet', '--locked', '-p', 'tailsync-runtime', '--example', 'generate_local_contracts'], { cwd: root, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
if (result.status !== 0) throw new Error(result.stderr || 'Rust contract export failed');
const schema = JSON.parse(result.stdout);
const defs = schema.$defs;
const quote = JSON.stringify;
const refName = s => s.$ref.slice('#/$defs/'.length);
const variants = s => s.anyOf ?? (Array.isArray(s.type) ? s.type.map(type => ({ ...s, type })) : null);
const nonNull = s => variants(s)?.filter(v => v.type !== 'null') ?? [s];
const nullable = s => variants(s)?.some(v => v.type === 'null') ?? s.type === 'null';
const policiesResult = spawnSync('cargo', ['run', '--quiet', '--locked', '-p', 'tailsync-runtime', '--example', 'generate_local_contracts', '--', '--stable-error-policies'], { cwd: root, encoding: 'utf8' });
if (policiesResult.status !== 0) throw new Error(policiesResult.stderr || 'Rust stable error policy export failed');
const stableErrorPolicies = Object.fromEntries(JSON.parse(policiesResult.stdout).map(({ code, retryable, message_key, detail_class }) =>
  [code, { retryable, message_key, detail_class }]));
if (quote(Object.keys(stableErrorPolicies).sort()) !== quote([...defs.StableErrorCode.enum].sort())) {
  throw new Error('Stable error policies do not cover every Rust code');
}

function tsType(s) {
  if (s.$ref) return refName(s);
  if (variants(s)) return variants(s).map(tsType).join(' | ');
  if (s.enum) return s.enum.map(quote).join(' | ');
  if (s.type === 'array') return `Array<${tsType(s.items)}>`;
  if (s.type === 'object') {
    if (!s.properties && s.additionalProperties) return `Record<string, ${tsType(s.additionalProperties)}>`;
    return `{\n${Object.entries(s.properties ?? {}).map(([k,v]) => `  ${quote(k)}${s.required?.includes(k) ? '' : '?'}: ${tsType(v)};`).join('\n')}\n}`;
  }
  if (s.type === 'integer' || s.type === 'number') return 'number';
  if (['string','boolean','null'].includes(s.type)) return s.type;
  throw new Error(`Unsupported schema: ${quote(s)}`);
}

function predicate(s, value) {
  if (s.$ref) return `valid${refName(s)}(${value})`;
  if (variants(s)) return `(${variants(s).map(v => predicate(v,value)).join(' || ')})`;
  if (s.allOf) return `(${s.allOf.map(v => predicate(v,value)).join(' && ')})`;
  const checks = [];
  if (s.type === 'object') {
    checks.push(`isRecord(${value})`);
    for (const [key, field] of Object.entries(s.properties ?? {})) {
      const has = `Object.hasOwn(${value}, ${quote(key)})`;
      const check = predicate(field, `${value}[${quote(key)}]`);
      checks.push(s.required?.includes(key) ? `(${has} && ${check})` : `(!${has} || ${check})`);
    }
    const keys = quote(Object.keys(s.properties ?? {}));
    if (s.additionalProperties === false) checks.push(`Object.keys(${value}).every(key => ${keys}.includes(key))`);
    else if (typeof s.additionalProperties === 'object') checks.push(Object.keys(s.properties ?? {}).length
      ? `Object.entries(${value}).every(([key, item]) => ${keys}.includes(key) || ${predicate(s.additionalProperties, 'item')})`
      : `Object.values(${value}).every(item => ${predicate(s.additionalProperties, 'item')})`);
  } else if (s.type === 'array') {
    checks.push(`Array.isArray(${value})`, `${value}.every(item => ${predicate(s.items,'item')})`);
    if (s.minItems != null) checks.push(`${value}.length >= ${s.minItems}`);
    if (s.maxItems != null) checks.push(`${value}.length <= ${s.maxItems}`);
  } else if (s.type === 'null') checks.push(`${value} === null`);
  else if (s.type === 'integer' || s.type === 'number') {
    checks.push(`typeof ${value} === "number"`, `Number.${s.type === 'integer' ? 'isSafeInteger' : 'isFinite'}(${value})`);
    if (s.minimum != null && s.maximum === s.minimum) checks.push(`${value} === ${s.minimum}`);
    else {
      if (s.minimum != null) checks.push(`${value} >= ${s.minimum}`);
      if (s.maximum != null) checks.push(`${value} <= ${s.maximum}`);
    }
  } else if (s.type) checks.push(`typeof ${value} === ${quote(s.type)}`);
  if (s.enum) checks.push(`(${s.enum.map(item => `${value} === ${quote(item)}`).join(' || ')})`);
  if (s.minLength != null) checks.push(`${value}.length >= ${s.minLength}`);
  if (s.maxLength != null) checks.push(`${value}.length <= ${s.maxLength}`);
  if (!checks.length) throw new Error(`Unsupported validator schema ${quote(s)}`);
  return `(${checks.join(' && ')})`;
}

function swiftType(s) {
  if (s.$ref) return `Contract${refName(s)}`;
  if (nullable(s)) return `${swiftType(nonNull(s)[0])}?`;
  if (s.type === 'array') return `[${swiftType(s.items)}]`;
  if (s.type === 'object' && s.additionalProperties) return `[String: ${swiftType(s.additionalProperties)}]`;
  if (s.type === 'integer') return /^uint/.test(s.format ?? '') ? (s.format === 'uint8' ? 'UInt8' : s.format === 'uint32' ? 'UInt32' : 'UInt64') : 'Int64';
  if (s.type === 'number') return 'Double';
  if (s.type === 'string') return 'String';
  if (s.type === 'boolean') return 'Bool';
  throw new Error(`Unsupported Swift schema ${quote(s)}`);
}
function swiftValidation(s, expression, optional = false) {
  const conditional = nullable(s) || optional;
  const value = conditional ? 'value' : expression;
  const checks = [];
  if (s.enum) checks.push(`${quote(s.enum)}.contains(${value})`);
  if (s.minimum != null && !(s.minimum === 0 && /^uint/.test(s.format ?? ''))) checks.push(`${value} >= ${s.minimum}`);
  if (s.maximum != null && !(s.maximum === 255 && s.format === 'uint8')) checks.push(`${value} <= ${s.maximum}`);
  if (s.minLength != null) checks.push(`${value}.count >= ${s.minLength}`);
  if (s.maxLength != null) checks.push(`${value}.count <= ${s.maxLength}`);
  if (s.maxItems != null) checks.push(`${value}.count <= ${s.maxItems}`);
  if (!checks.length) return '';
  const guard = `guard ${checks.join(', ')} else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }`;
  return conditional ? `    if let value = ${expression} { ${guard} }\n` : `    ${guard}\n`;
}
function swiftDefinition(name, s) {
  if (s.enum) {
    const cases = s.enum.map(v => `  case \`${v}\` = ${quote(v)}`).join('\n');
    if (name === 'StableErrorCode') return `enum Contract${name}: String, Codable, Sendable {\n${cases}\n  init(from decoder: Decoder) throws {\n    let value = try decoder.singleValueContainer().decode(String.self)\n    self = Self(rawValue: value) ?? .internal_error\n  }\n  func encode(to encoder: Encoder) throws {\n    var container = encoder.singleValueContainer()\n    try container.encode(rawValue)\n  }\n}\n`;
    return `enum Contract${name}: String, Codable, Sendable {\n${cases}\n}\n`;
  }
  if (s.type !== 'object') return `typealias Contract${name} = ${swiftType(s)}\n`;
  if (name === 'StableErrorEnvelope') return `struct ContractStableErrorEnvelope: Codable, Sendable {
  let schema_version: UInt32
  let code: ContractStableErrorCode
  let retryable: Bool
  let message_key: String
  let detail_class: ContractStableErrorDetailClass
  private enum CodingKeys: String, CodingKey { case schema_version, code, retryable, message_key, detail_class }
  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    let version = try container.decode(UInt32.self, forKey: .schema_version)
    guard version == 1 else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Unsupported stable error schema version")) }
    self.schema_version = version
    let rawCode = try container.decode(String.self, forKey: .code)
    _ = try container.decode(Bool.self, forKey: .retryable)
    _ = try container.decode(String.self, forKey: .message_key)
    _ = try container.decode(String.self, forKey: .detail_class)
    self.code = ContractStableErrorCode(rawValue: rawCode) ?? .internal_error
    switch self.code {
${Object.entries(stableErrorPolicies).map(([code, policy]) => `    case .${code}:
      self.retryable = ${policy.retryable}
      self.message_key = ${quote(policy.message_key)}
      self.detail_class = .${policy.detail_class}`).join('\n')}
    }
  }
}
`;
  const fields = Object.entries(s.properties);
  const type = (key, value) => swiftType(value) + (!s.required?.includes(key) && !nullable(value) ? '?' : '');
  let text = `struct Contract${name}: Codable, Sendable {\n`;
  text += fields.map(([key,value]) => `  let \`${key}\`: ${type(key,value)}`).join('\n') + '\n';
  text += `  private enum CodingKeys: String, CodingKey { case ${fields.map(([key]) => `\`${key}\``).join(', ')} }\n`;
  text += '  init(from decoder: Decoder) throws {\n    let container = try decoder.container(keyedBy: CodingKeys.self)\n';
  for (const [key,value] of fields) {
    const required = s.required?.includes(key);
    const base = swiftType(nullable(value) ? nonNull(value)[0] : value);
    const access = `.${'`'+key+'`'}`;
    if (required && nullable(value)) text += `    guard container.contains(${access}) else { throw DecodingError.keyNotFound(CodingKeys.\`${key}\`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }\n`;
    const decode = `try container.${nullable(value) ? 'decodeIfPresent' : 'decode'}(${base}.self, forKey: ${access})`;
    text += `    self.\`${key}\` = ${!required && !nullable(value) ? `container.contains(${access}) ? ${decode} : nil` : decode}\n`;
    text += swiftValidation(value, `self.\`${key}\``, !required);
  }
  if (name === 'DisplayDocument') text += '    try validateContractDocument(self)\n';
  if (name === 'DocumentFrameMetadata') text += '    try validateContractDocumentFrame(self)\n';
  return text + '  }\n}\n';
}

const banner = '// Generated from production Rust DTOs by shared/schema/generate-local-contracts.mjs. Do not edit.\n';
let ts = banner + 'const isRecord = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === "object" && !Array.isArray(value);\n';
let js = banner + 'const isRecord = value => value !== null && typeof value === "object" && !Array.isArray(value);\n';
let swift = banner + 'import Foundation\n\n';
const documentValidation = `
function validDocumentLink(value) {
  if (!value || new TextEncoder().encode(value).length > 2048 || value.includes("\\\\")) return false;
  for (const character of value) {
    const scalar = character.codePointAt(0) ?? 0;
    if (character.trim() === "" || scalar <= 31 || (scalar >= 127 && scalar <= 159)) return false;
  }
  const authority = value.match(/^https?:\\/\\/([^/?#]*)/i)?.[1];
  if (!authority || authority.includes("@")) return false;
  try { const parsed = new URL(value); return (parsed.protocol === "http:" || parsed.protocol === "https:") && !!parsed.hostname && !parsed.username && !parsed.password; } catch { return false; }
}
function validDocumentSemantics(value) {
  let textBytes = 0, runs = 0, cells = 0, images = 0, offset = 0, pixels = 0;
  let first = null;
  const used = new Set();
  const validRuns = items => {
    runs += items.length;
    for (const run of items) {
      if (run.text.includes("\\0")) return false;
      textBytes += new TextEncoder().encode(run.text).length;
      if (run.link != null) { if (!validDocumentLink(run.link)) return false; textBytes += new TextEncoder().encode(run.link).length; }
    }
    return runs <= 8192 && textBytes <= 1048576;
  };
  if (value.blocks.length > 8192 || value.assets.length > 64 || value.partial.length > 8192) return false;
  for (const block of value.blocks) {
    if (block.kind === "paragraph") {
      if (!block.paragraph || block.table || block.image || !validRuns(block.paragraph.runs)) return false;
      if (block.paragraph.list == null && (block.paragraph.list_depth ?? 0) !== 0) return false;
    }
    else if (block.kind === "table") {
      if (!block.table || block.paragraph || block.image || block.table.rows.length > 2048) return false;
      for (const row of block.table.rows) { if (!row.cells.length || row.cells.length > 2048) return false; for (const cell of row.cells) { cells++; if (cells > 2048 || !validRuns(cell.runs)) return false; } }
    } else {
      if (!block.image || block.paragraph || block.table || block.image.ordinal !== images || (block.image.availability === "available") !== (block.image.asset != null)) return false;
      if (images++ === 0) first = block.image.asset ?? null;
      if (block.image.asset != null) { if (block.image.asset >= value.assets.length) return false; used.add(block.image.asset); }
    }
  }
  if (images > 64 || (value.first_image ?? null) !== first || used.size !== value.assets.length) return false;
  for (const [index, asset] of value.assets.entries()) {
    if (asset.index !== index || asset.offset !== offset || asset.length <= 0 || asset.width <= 0 || asset.height <= 0 || !/^[0-9a-f]{64}$/.test(asset.sha256)) return false;
    offset += asset.length; pixels += asset.width * asset.height;
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(pixels) || offset > 16777216 || pixels > 16000000) return false;
  }
  return value.partial.every(reason => !reason.detail.includes("\\0") && new TextEncoder().encode(reason.detail).length <= 512);
}
function validDocumentFrameSemantics(value) {
  return typeof value.request_id === "string" && value.request_id.length > 0 && new TextEncoder().encode(value.request_id).length <= 128 && value.name.length > 0 && new TextEncoder().encode(value.name).length <= 256 && /^dc1:[0-9a-f]{64}$/.test(value.stamp) && value.model_bytes > 0 && value.model_bytes <= value.size_bytes && value.batch == null;
}
`;
js += documentValidation;
ts += documentValidation.replace('function validDocumentLink(value)', 'function validDocumentLink(value: string)').replace('function validDocumentSemantics(value)', 'function validDocumentSemantics(value: DisplayDocument)').replace('let first = null;', 'let first: number | null = null;').replace('const used = new Set();', 'const used = new Set<number>();').replace('const validRuns = items =>', 'const validRuns = (items: Array<DocumentRun>) =>').replace('function validDocumentFrameSemantics(value)', 'function validDocumentFrameSemantics(value: DocumentFrameMetadata)');
swift += `
private func documentContractFailure() -> DecodingError { .dataCorrupted(.init(codingPath: [], debugDescription: "Invalid document contract")) }
private func validContractDocumentLink(_ value: String) -> Bool {
  guard !value.isEmpty, value.utf8.count <= 2048,
        !value.unicodeScalars.contains(where: { CharacterSet.whitespacesAndNewlines.contains($0) || $0.value <= 31 || (127...159).contains($0.value) || $0.value == 92 }),
        let components = URLComponents(string: value), let scheme = components.scheme?.lowercased(),
        ["http", "https"].contains(scheme), let host = components.host, !host.isEmpty,
        components.user == nil, components.password == nil,
        value.range(of: "^https?://[^/?#]+", options: [.regularExpression, .caseInsensitive]) != nil else { return false }
  return true
}
private func validateContractDocument(_ value: ContractDisplayDocument) throws {
  var textBytes = 0, runs = 0, cells = 0, images = 0
  var offset: UInt64 = 0, pixels: UInt64 = 0
  var first: UInt64?
  var used = Set<UInt64>()
  func validateRuns(_ items: [ContractDocumentRun]) throws {
    runs += items.count
    for run in items {
      guard !run.text.contains("\\0") else { throw documentContractFailure() }; textBytes += run.text.utf8.count
      if let link = run.link { guard validContractDocumentLink(link) else { throw documentContractFailure() }; textBytes += link.utf8.count }
    }
    guard runs <= 8192, textBytes <= 1048576 else { throw documentContractFailure() }
  }
  guard value.blocks.count <= 8192, value.assets.count <= 64, value.partial.count <= 8192 else { throw documentContractFailure() }
  for block in value.blocks {
    switch block.kind {
    case .paragraph:
      guard let paragraph = block.paragraph, block.table == nil, block.image == nil, paragraph.list != nil || (paragraph.list_depth ?? 0) == 0 else { throw documentContractFailure() }; try validateRuns(paragraph.runs)
    case .table:
      guard let table = block.table, block.paragraph == nil, block.image == nil, table.rows.count <= 2048 else { throw documentContractFailure() }
      for row in table.rows { guard !row.cells.isEmpty, row.cells.count <= 2048 else { throw documentContractFailure() }; for cell in row.cells { cells += 1; guard cells <= 2048 else { throw documentContractFailure() }; try validateRuns(cell.runs) } }
    case .image:
      guard let image = block.image, block.paragraph == nil, block.table == nil, image.ordinal == UInt64(images), (image.availability == .available) == (image.asset != nil) else { throw documentContractFailure() }
      if images == 0 { first = image.asset }; images += 1
      if let asset = image.asset { guard asset < UInt64(value.assets.count) else { throw documentContractFailure() }; used.insert(asset) }
    }
  }
  guard images <= 64, value.first_image == first, used.count == value.assets.count else { throw documentContractFailure() }
  for (index, asset) in value.assets.enumerated() {
    guard asset.index == UInt64(index), asset.offset == offset, asset.length > 0, asset.length <= 16777216, asset.width > 0, asset.height > 0, asset.width <= 16000000, asset.height <= 16000000, asset.sha256.count == 64, asset.sha256.allSatisfy({ "0123456789abcdef".contains($0) }) else { throw documentContractFailure() }
    offset += asset.length; pixels += UInt64(asset.width) * UInt64(asset.height)
    guard offset <= 16777216, pixels <= 16000000 else { throw documentContractFailure() }
  }
  for reason in value.partial { guard !reason.detail.contains("\\0"), reason.detail.utf8.count <= 512 else { throw documentContractFailure() } }
}
private func validateContractDocumentFrame(_ value: ContractDocumentFrameMetadata) throws {
  guard let request = value.request_id, !request.isEmpty, request.utf8.count <= 128, !value.name.isEmpty, value.name.utf8.count <= 256, value.stamp.hasPrefix("dc1:"), value.stamp.count == 68, value.stamp.dropFirst(4).allSatisfy({ "0123456789abcdef".contains($0) }), value.model_bytes > 0, value.model_bytes <= value.size_bytes, value.batch == nil else { throw documentContractFailure() }
}
`;
for (const [name, s] of Object.entries(defs)) {
  const stableErrorCode = name === 'StableErrorCode';
  const validationSchema = name === 'StableErrorEnvelope'
    ? { ...s, properties: { ...s.properties, detail_class: { type: 'string' } } }
    : s;
  const semantic = name === 'DisplayDocument' ? ' && validDocumentSemantics(value)' : name === 'DocumentFrameMetadata' ? ' && validDocumentFrameSemantics(value)' : '';
  const body = `return ${stableErrorCode ? 'typeof value === "string"' : predicate(validationSchema,'value')}${semantic};`;
  const tsBody = body.replace('validDocumentSemantics(value)', 'validDocumentSemantics(value as DisplayDocument)').replace('validDocumentFrameSemantics(value)', 'validDocumentFrameSemantics(value as DocumentFrameMetadata)');
  ts += `\nexport type ${name} = ${tsType(s)};\nfunction valid${name}(value: unknown): value is ${name} { ${tsBody} }\n`;
  if (stableErrorCode) {
    const known = quote(s.enum);
    ts += `export function decode${name}(value: unknown): ${name} { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); return ${known}.includes(value as ${name}) ? value as ${name} : "internal_error"; }\n`;
    js += `\nfunction valid${name}(value) { ${body} }\nexport function decode${name}(value) { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); return ${known}.includes(value) ? value : "internal_error"; }\n`;
  } else if (name === 'StableErrorEnvelope') {
    ts += `export function decode${name}(value: unknown): ${name} { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); const code = decodeStableErrorCode(value.code); const policy = (${quote(stableErrorPolicies)} as const)[code]; return { schema_version: value.schema_version, code, ...policy }; }\n`;
    js += `\nfunction valid${name}(value) { ${body} }\nexport function decode${name}(value) { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); const code = decodeStableErrorCode(value.code); const policy = ${quote(stableErrorPolicies)}[code]; return { schema_version: value.schema_version, code, ...policy }; }\n`;
  } else {
    ts += `export function decode${name}(value: unknown): ${name} { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); return value; }\n`;
    js += `\nfunction valid${name}(value) { ${body} }\nexport function decode${name}(value) { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); return value; }\n`;
  }
  swift += swiftDefinition(name,s) + '\n';
}
swift = `${swift.trimEnd()}\n`;
const outputs = {
  'shared/schema/local-contract.schema.json': result.stdout,
  'shared/schema/local-contract-decoders.generated.mjs': js,
  'windows/src/types/localContracts.generated.ts': ts,
  'macos/swift-ui/Sources/TailSync/Models/LocalContracts.generated.swift': swift,
};
// Serialize real production DTOs, then derive negative boundary fixtures from
// their exported field requirements. Tests use the generated runtime decoders.
const examples = spawnSync('cargo', ['run', '--quiet', '--locked', '-p', 'tailsync-runtime', '--example', 'generate_local_contracts', '--', '--fixtures'], { cwd: root, encoding: 'utf8' });
if (examples.status !== 0) throw new Error(examples.stderr);
const values = new Map();
function visit(s,value) {
  if (value == null) return;
  if (s.$ref) { const name = refName(s); values.set(name,value); visit(defs[name],value); return; }
  if (variants(s)) { for (const variant of nonNull(s)) visit(variant,value); return; }
  if (s.type === 'array') value.forEach(item => visit(s.items,item));
  if (s.type === 'object') {
    for (const [key,child] of Object.entries(s.properties ?? {})) if (Object.hasOwn(value,key)) visit(child,value[key]);
    if (typeof s.additionalProperties === 'object') Object.values(value).forEach(item => visit(s.additionalProperties,item));
  }
}
visit(schema, JSON.parse(examples.stdout));
// `StableErrorCode` has a tolerant *standalone* decoder: an unknown code is mapped to
// `internal_error` rather than rejected, and it has its own explicit fixture.
const tolerantEnums = new Set(['StableErrorCode']);
// Inside an error envelope the decoder deliberately does not re-validate either the
// code or the coarse detail class, so an unknown member there is accepted. The
// standalone `StableErrorDetailClass` decoder *is* strict, which is why this set is
// not the same as `tolerantEnums` (R5-05).
const envelopeTolerantEnums = new Set(['StableErrorCode', 'StableErrorDetailClass']);
const fixtures = [];
const legacyDocument = structuredClone(values.get('DisplayDocument'));
for (const block of legacyDocument.blocks) {
  if (block.paragraph) {
    for (const key of ['heading', 'quote_depth', 'code_block', 'list_depth']) delete block.paragraph[key];
  }
  const runs = block.paragraph?.runs ?? block.table?.rows.flatMap(row => row.cells.flatMap(cell => cell.runs)) ?? [];
  for (const run of runs) {
    delete run.link;
    delete run.style.monospace;
    delete run.style.highlight;
  }
}
fixtures.push({ name: 'DisplayDocument: legacy v1 reading fields absent', contract: 'DisplayDocument', valid: true, value: legacyDocument });
for (const link of ['javascript:alert(1)', 'file:///tmp/secret', 'https://@example.com/', 'https://user:password@example.com/', 'https://exa\tmple.com/', 'https://example.com/\\path', `https://example.com/${'中'.repeat(800)}`]) {
  const invalid = structuredClone(values.get('DisplayDocument'));
  invalid.blocks[0].paragraph.runs[0].link = link;
  fixtures.push({ name: `DisplayDocument: unsafe reading link ${fixtures.length}`, contract: 'DisplayDocument', valid: false, value: invalid });
}
for (const [name,value] of values) {
  const definition = defs[name];
  fixtures.push({ name: `${name}: production output`, contract: name, valid: true, value });
  if (definition.type === 'object') {
    fixtures.push({ name: `${name}: additive field`, contract: name, valid: true, value: { ...value, future_optional_field: 'accepted' } });
    for (const key of definition.required ?? []) {
      const missing = structuredClone(value); delete missing[key];
      fixtures.push({ name: `${name}: missing ${key}`, contract: name, valid: false, value: missing });
      const wrong = structuredClone(value); wrong[key] = Array.isArray(wrong[key]) ? {} : [];
      fixtures.push({ name: `${name}: wrong type ${key}`, contract: name, valid: false, value: wrong });
    }
    // Nested property enums (e.g. CompositeInfo.completeness) previously had no
    // unknown-value fixture at all, so nothing proved a future enum member would be
    // rejected rather than silently accepted (R5-05).
    for (const [key, child] of Object.entries(definition.properties ?? {})) {
      if (!Object.hasOwn(value, key)) continue;
      const resolved = child.$ref ? defs[refName(child)] : child;
      if (!resolved?.enum) continue;
      if (child.$ref && envelopeTolerantEnums.has(refName(child))) continue;
      const unknown = structuredClone(value);
      unknown[key] = 'unknown_future_enum';
      fixtures.push({ name: `${name}: unknown enum ${key}`, contract: name, valid: false, value: unknown });
    }
  } else if (definition.enum) fixtures.push({ name: `${name}: unknown enum`, contract: name, valid: tolerantEnums.has(name), value: 'unknown_future_enum' });
}
const legacyHistory = structuredClone(values.get('HistoryEntry'));
legacyHistory.type = 'text';
delete legacyHistory.composite;
fixtures.push({name: 'HistoryEntry: legacy record without composite metadata', contract:'HistoryEntry', valid:true, value:legacyHistory});
fixtures.push({
  name: 'StableErrorEnvelope: unknown code maps to internal_error',
  contract: 'StableErrorEnvelope',
  valid: true,
  value: { ...values.get('StableErrorEnvelope'), code: 'unknown_future_error', detail_class: 'future_detail' },
});
fixtures.push({
  name: 'StableErrorEnvelope: known code ignores untrusted policy',
  contract: 'StableErrorEnvelope',
  valid: true,
  value: { ...values.get('StableErrorEnvelope'), code: 'unauthorized', retryable: true, message_key: 'untrusted.message', detail_class: 'future_detail' },
});
fixtures.push({ name: 'safe integer boundary', contract: 'WindowsRuntimeSnapshot', valid: true, typescriptValid: false,
  value: { ...values.get('WindowsRuntimeSnapshot'), revision: 9007199254740992 } });
outputs['shared/schema/fixtures/local-contracts.json'] = JSON.stringify(fixtures,null,2) + '\n';
outputs['macos/swift-ui/Tests/TailSyncTests/LocalContractFixtures.generated.swift'] = banner + `import Foundation\n@testable import TailSync\n\nfunc validateGeneratedFixture(_ contract: String, data: Data) throws {\n  switch contract {\n${Object.keys(defs).map(name => `  case "${name}": _ = try JSONDecoder().decode(Contract${name}.self, from: data)`).join('\n')}\n  default: throw NSError(domain: "Unknown contract fixture", code: 1)\n  }\n}\n`;
for (const [file, content] of Object.entries(outputs)) {
  const path = resolve(root,file);
  if (process.argv.includes('--check')) {
    if (readFileSync(path,'utf8').replaceAll('\r\n','\n') !== content) throw new Error(`${file} is stale`);
  } else writeFileSync(path,content);
}
console.log(`Generated local contracts: ${Object.keys(defs).length} production DTOs`);
