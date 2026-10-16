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

const documentResult = spawnSync('cargo', ['run', '--quiet', '--locked', '-p', 'tailsync-runtime', '--example', 'generate_local_contracts', '--', '--document-policies'], { cwd: root, encoding: 'utf8' });
if (documentResult.status !== 0) throw new Error(documentResult.stderr || 'Rust document policy export failed');
const documentPolicies = JSON.parse(documentResult.stdout);
for (const key of ['version', 'max_bytes', 'max_metadata_bytes', 'max_model_bytes', 'text_limit', 'block_limit', 'run_limit', 'cell_limit', 'asset_limit', 'pixel_limit', 'detail_limit', 'link_limit']) {
  if (!Number.isSafeInteger(documentPolicies[key]) || documentPolicies[key] <= 0) throw new Error(`Document policy export has an invalid ${key}`);
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
// Enums whose Rust producer may add members in a newer build. The generated
// decoder falls back to a known member instead of failing the whole response;
// `serde(other)` on the Rust side does not propagate to generated code, so
// every tolerant enum is listed here explicitly.
const tolerantEnumFallbacks = { StableErrorCode: 'internal_error', PairingFailureKind: 'unknown' };

function swiftDefinition(name, s) {
  if (s.enum) {
    const cases = s.enum.map(v => `  case \`${v}\` = ${quote(v)}`).join('\n');
    const fallback = tolerantEnumFallbacks[name];
    if (fallback) return `enum Contract${name}: String, Codable, Sendable {\n${cases}\n  init(from decoder: Decoder) throws {\n    let value = try decoder.singleValueContainer().decode(String.self)\n    self = Self(rawValue: value) ?? .${fallback}\n  }\n  func encode(to encoder: Encoder) throws {\n    var container = encoder.singleValueContainer()\n    try container.encode(rawValue)\n  }\n}\n`;
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
function hasLoneSurrogate(value) {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xD800 && code <= 0xDBFF) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xDC00 && next <= 0xDFFF)) return true;
      index += 1;
    } else if (code >= 0xDC00 && code <= 0xDFFF) return true;
  }
  return false;
}
function validDocumentLink(value) {
  if (!value || new TextEncoder().encode(value).length > ${documentPolicies.link_limit} || value.includes("\\\\") || hasLoneSurrogate(value)) return false;
  const whiteSpace = /\\p{White_Space}/u;
  for (const character of value) {
    const scalar = character.codePointAt(0) ?? 0;
    if (whiteSpace.test(character) || scalar <= 31 || (scalar >= 127 && scalar <= 159)) return false;
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
      if (run.text.includes("\\0") || hasLoneSurrogate(run.text)) return false;
      textBytes += new TextEncoder().encode(run.text).length;
      if (run.link != null) { if (!validDocumentLink(run.link)) return false; textBytes += new TextEncoder().encode(run.link).length; }
    }
    return runs <= ${documentPolicies.run_limit} && textBytes <= ${documentPolicies.text_limit};
  };
  if (value.blocks.length > ${documentPolicies.block_limit} || value.assets.length > ${documentPolicies.asset_limit} || value.partial.length > ${documentPolicies.block_limit}) return false;
  for (const block of value.blocks) {
    if (block.kind === "paragraph") {
      if (!block.paragraph || block.table || block.image || !validRuns(block.paragraph.runs)) return false;
      if (block.paragraph.list == null && (block.paragraph.list_depth ?? 0) !== 0) return false;
    }
    else if (block.kind === "table") {
      if (!block.table || block.paragraph || block.image || block.table.rows.length > ${documentPolicies.cell_limit}) return false;
      for (const row of block.table.rows) { if (!row.cells.length || row.cells.length > ${documentPolicies.cell_limit}) return false; for (const cell of row.cells) { cells++; if (cells > ${documentPolicies.cell_limit} || !validRuns(cell.runs)) return false; } }
    } else {
      if (!block.image || block.paragraph || block.table || block.image.ordinal !== images || (block.image.availability === "available") !== (block.image.asset != null)) return false;
      if (images++ === 0) first = block.image.asset ?? null;
      if (block.image.asset != null) { if (block.image.asset >= value.assets.length) return false; used.add(block.image.asset); }
    }
  }
  if (images > ${documentPolicies.asset_limit} || (value.first_image ?? null) !== first || used.size !== value.assets.length) return false;
  for (const [index, asset] of value.assets.entries()) {
    if (asset.index !== index || asset.offset !== offset || asset.length <= 0 || asset.width <= 0 || asset.height <= 0 || !/^[0-9a-fA-F]{64}$/.test(asset.sha256)) return false;
    offset += asset.length; pixels += asset.width * asset.height;
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(pixels) || offset > ${documentPolicies.max_bytes} || pixels > ${documentPolicies.pixel_limit}) return false;
  }
  return value.partial.every(reason => !reason.detail.includes("\\0") && !hasLoneSurrogate(reason.detail) && new TextEncoder().encode(reason.detail).length <= ${documentPolicies.detail_limit});
}
function validDocumentFrameSemantics(value) {
  return typeof value.request_id === "string" && value.request_id.length > 0 && !hasLoneSurrogate(value.request_id) && new TextEncoder().encode(value.request_id).length <= 128 && value.name.length > 0 && !hasLoneSurrogate(value.name) && new TextEncoder().encode(value.name).length <= 256 && /^dc1:[0-9a-fA-F]{64}$/.test(value.stamp) && value.model_bytes > 0 && value.model_bytes <= value.size_bytes && value.batch == null;
}
`;
js += documentValidation;
ts += documentValidation.replace('function hasLoneSurrogate(value)', 'function hasLoneSurrogate(value: string)').replace('function validDocumentLink(value)', 'function validDocumentLink(value: string)').replace('function validDocumentSemantics(value)', 'function validDocumentSemantics(value: DisplayDocument)').replace('let first = null;', 'let first: number | null = null;').replace('const used = new Set();', 'const used = new Set<number>();').replace('const validRuns = items =>', 'const validRuns = (items: Array<DocumentRun>) =>').replace('function validDocumentFrameSemantics(value)', 'function validDocumentFrameSemantics(value: DocumentFrameMetadata)');
ts += `export const DOCUMENT_MAX_METADATA_BYTES = ${documentPolicies.max_metadata_bytes};\nexport const DOCUMENT_MAX_MODEL_BYTES = ${documentPolicies.max_model_bytes};\nexport const DOCUMENT_MAX_BYTES = ${documentPolicies.max_bytes};\n`;
swift += `
enum ContractDocumentPolicies {
  static let maxMetadataBytes: UInt64 = ${documentPolicies.max_metadata_bytes}
  static let maxModelBytes: UInt64 = ${documentPolicies.max_model_bytes}
  static let maxBytes: UInt64 = ${documentPolicies.max_bytes}
}

private func documentContractFailure() -> DecodingError { .dataCorrupted(.init(codingPath: [], debugDescription: "Invalid document contract")) }
private func validContractDocumentLink(_ value: String) -> Bool {
  guard !value.isEmpty, value.utf8.count <= ${documentPolicies.link_limit},
        !value.unicodeScalars.contains(where: { $0.properties.isWhitespace || $0.value <= 31 || (127...159).contains($0.value) || $0.value == 92 }),
        let components = URLComponents(string: value), let scheme = components.scheme?.lowercased(),
        ["http", "https"].contains(scheme), let host = components.host, !host.isEmpty,
        components.port.map({ (0...65535).contains($0) }) ?? true,
        !host.unicodeScalars.contains(where: { $0.properties.isWhitespace || $0.value == 64 || $0.value <= 31 || (127...159).contains($0.value) }),
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
    guard runs <= ${documentPolicies.run_limit}, textBytes <= ${documentPolicies.text_limit} else { throw documentContractFailure() }
  }
  guard value.blocks.count <= ${documentPolicies.block_limit}, value.assets.count <= ${documentPolicies.asset_limit}, value.partial.count <= ${documentPolicies.block_limit} else { throw documentContractFailure() }
  for block in value.blocks {
    switch block.kind {
    case .paragraph:
      guard let paragraph = block.paragraph, block.table == nil, block.image == nil, paragraph.list != nil || (paragraph.list_depth ?? 0) == 0 else { throw documentContractFailure() }; try validateRuns(paragraph.runs)
    case .table:
      guard let table = block.table, block.paragraph == nil, block.image == nil, table.rows.count <= ${documentPolicies.cell_limit} else { throw documentContractFailure() }
      for row in table.rows { guard !row.cells.isEmpty, row.cells.count <= ${documentPolicies.cell_limit} else { throw documentContractFailure() }; for cell in row.cells { cells += 1; guard cells <= ${documentPolicies.cell_limit} else { throw documentContractFailure() }; try validateRuns(cell.runs) } }
    case .image:
      guard let image = block.image, block.paragraph == nil, block.table == nil, image.ordinal == UInt64(images), (image.availability == .available) == (image.asset != nil) else { throw documentContractFailure() }
      if images == 0 { first = image.asset }; images += 1
      if let asset = image.asset { guard asset < UInt64(value.assets.count) else { throw documentContractFailure() }; used.insert(asset) }
    }
  }
  guard images <= ${documentPolicies.asset_limit}, value.first_image == first, used.count == value.assets.count else { throw documentContractFailure() }
  for (index, asset) in value.assets.enumerated() {
    guard asset.index == UInt64(index), asset.offset == offset, asset.length > 0, asset.length <= ${documentPolicies.max_bytes}, asset.width > 0, asset.height > 0, asset.width <= ${documentPolicies.pixel_limit}, asset.height <= ${documentPolicies.pixel_limit}, asset.sha256.count == 64, asset.sha256.allSatisfy({ "0123456789abcdefABCDEF".contains($0) }) else { throw documentContractFailure() }
    offset += asset.length; pixels += UInt64(asset.width) * UInt64(asset.height)
    guard offset <= ${documentPolicies.max_bytes}, pixels <= ${documentPolicies.pixel_limit} else { throw documentContractFailure() }
  }
  for reason in value.partial { guard !reason.detail.contains("\\0"), reason.detail.utf8.count <= ${documentPolicies.detail_limit} else { throw documentContractFailure() } }
}
private func validateContractDocumentFrame(_ value: ContractDocumentFrameMetadata) throws {
  guard let request = value.request_id, !request.isEmpty, request.utf8.count <= 128, !value.name.isEmpty, value.name.utf8.count <= 256, value.stamp.hasPrefix("dc1:"), value.stamp.count == 68, value.stamp.dropFirst(4).allSatisfy({ "0123456789abcdefABCDEF".contains($0) }), value.model_bytes > 0, value.model_bytes <= value.size_bytes, value.batch == nil else { throw documentContractFailure() }
}
`;
for (const [name, s] of Object.entries(defs)) {
  // A tolerant enum accepts any string and normalizes an unknown member; a
  // non-string value is still rejected by the predicate below.
  const tolerantEnum = tolerantEnumFallbacks[name];
  // Fields the decoder accepts as any string and normalizes afterwards. The
  // predicate alone cannot express that, so the field schema is widened here
  // and the decoder below performs the real normalization.
  const tolerantFields = { PairingStatus: ['failure_kind'] }[name] ?? [];
  const widened = Object.fromEntries(tolerantFields.map(key => [key, { anyOf: [{ type: 'string' }, { type: 'null' }] }]));
  const validationSchema = name === 'StableErrorEnvelope'
    ? { ...s, properties: { ...s.properties, detail_class: { type: 'string' } } }
    : tolerantFields.length
      ? { ...s, properties: { ...s.properties, ...widened } }
      : s;
  const semantic = name === 'DisplayDocument' ? ' && validDocumentSemantics(value)' : name === 'DocumentFrameMetadata' ? ' && validDocumentFrameSemantics(value)' : '';
  const body = `return ${tolerantEnum ? 'typeof value === "string"' : predicate(validationSchema,'value')}${semantic};`;
  const tsBody = body.replace('validDocumentSemantics(value)', 'validDocumentSemantics(value as DisplayDocument)').replace('validDocumentFrameSemantics(value)', 'validDocumentFrameSemantics(value as DocumentFrameMetadata)');
  ts += `\nexport type ${name} = ${tsType(s)};\nfunction valid${name}(value: unknown): value is ${name} { ${tsBody} }\n`;
  if (tolerantEnum) {
    const known = quote(s.enum);
    ts += `export function decode${name}(value: unknown): ${name} { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); return ${known}.includes(value as ${name}) ? value as ${name} : ${quote(tolerantEnum)}; }\n`;
    js += `\nfunction valid${name}(value) { ${body} }\nexport function decode${name}(value) { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); return ${known}.includes(value) ? value : ${quote(tolerantEnum)}; }\n`;
  } else if (name === 'StableErrorEnvelope') {
    ts += `export function decode${name}(value: unknown): ${name} { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); const code = decodeStableErrorCode(value.code); const policy = (${quote(stableErrorPolicies)} as const)[code]; return { schema_version: value.schema_version, code, ...policy }; }\n`;
    js += `\nfunction valid${name}(value) { ${body} }\nexport function decode${name}(value) { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); const code = decodeStableErrorCode(value.code); const policy = ${quote(stableErrorPolicies)}[code]; return { schema_version: value.schema_version, code, ...policy }; }\n`;
  } else if (tolerantFields.length) {
    const pascal = key => key.split('_').map(part => `${part[0].toUpperCase()}${part.slice(1)}`).join('');
    const fieldEnum = key => refName(nonNull(s.properties[key])[0]);
    const fieldFallback = key => tolerantEnumFallbacks[fieldEnum(key)];
    // The tolerant enum predicate accepts any string, so the normalization
    // compares against the known members directly instead of reusing it.
    const knownValues = key => quote(defs[fieldEnum(key)].enum);
    const normalizeTs = key => `function normalize${pascal(key)}(value: unknown): ${tsType(s.properties[key])} { if (value === null) return null; return ${knownValues(key)}.includes(value as ${fieldEnum(key)}) ? value as ${fieldEnum(key)} : ${quote(fieldFallback(key))}; }`;
    const normalizeJs = key => `function normalize${pascal(key)}(value) { if (value === null) return null; return ${knownValues(key)}.includes(value) ? value : ${quote(fieldFallback(key))}; }`;
    const absent = key => `!Object.hasOwn(value, ${quote(key)}) || value[${quote(key)}] === null`;
    const assign = key => `${key}: normalize${pascal(key)}(value[${quote(key)}])`;
    const jsAbsent = key => `!Object.hasOwn(value, ${quote(key)}) || value.${key} === null`;
    const jsAssign = key => `${key}: normalize${pascal(key)}(value.${key})`;
    ts += tolerantFields.map(normalizeTs).join('\n') + `\nexport function decode${name}(value: unknown): ${name} { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); return ${tolerantFields.map(absent).join(' || ')} ? value : { ...value, ${tolerantFields.map(assign).join(', ')} }; }\n`;
    js += `\nfunction valid${name}(value) { ${body} }\n` + tolerantFields.map(normalizeJs).join('\n') + `\nexport function decode${name}(value) { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); return ${tolerantFields.map(jsAbsent).join(' || ')} ? value : { ...value, ${tolerantFields.map(jsAssign).join(', ')} }; }\n`;
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
const tolerantEnums = new Set(Object.keys(tolerantEnumFallbacks));
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
// The pairing status gained an optional `failure_kind`. The production sample
// above stays a clean (disabled) status, so it covers the legacy shape where the
// field is absent; these cover the classified shape a typed producer emits, the
// absent/null shapes an older producer still sends, and the future member this
// build does not know: it must stay decodable and normalize to `unknown`, while
// a non-string value still fails.
fixtures.push({
  name: 'PairingStatus: classified failure',
  contract: 'PairingStatus',
  valid: true,
  value: { ...values.get('PairingStatus'), error: 'Pairing connection failed: IO error: early eof', failure_kind: 'connection_closed' },
});
fixtures.push({
  name: 'PairingStatus: future failure kind normalizes to unknown',
  contract: 'PairingStatus',
  valid: true,
  value: { ...values.get('PairingStatus'), error: 'Pairing window timed out', failure_kind: 'future_failure_kind' },
});
fixtures.push({
  name: 'PairingStatus: null failure kind stays null',
  contract: 'PairingStatus',
  valid: true,
  value: { ...values.get('PairingStatus'), failure_kind: null },
});
fixtures.push({
  name: 'PairingStatus: non-string failure kind is rejected',
  contract: 'PairingStatus',
  valid: false,
  value: { ...values.get('PairingStatus'), failure_kind: 7 },
});
// The standalone pairing-kind decoder is tolerant for the same reason the
// nested field is: a newer producer's member must not fail the response.
fixtures.push({
  name: 'PairingFailureKind: known member is preserved',
  contract: 'PairingFailureKind',
  valid: true,
  value: 'window_closed',
});
fixtures.push({
  name: 'PairingFailureKind: future member normalizes to unknown',
  contract: 'PairingFailureKind',
  valid: true,
  value: 'future_failure_kind',
});
fixtures.push({
  name: 'PairingFailureKind: non-string member is rejected',
  contract: 'PairingFailureKind',
  valid: false,
  value: 7,
});
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

// ---------------------------------------------------------------------------
// F8 document semantic corpus.
//
// One shared file drives the real Rust `serde` + `DisplayDocument::validate`
// path, the real generated JavaScript/TypeScript decoder and the real Swift
// `JSONDecoder`. Small cases carry a literal `value`; raw-JSON parser cases
// carry a `json` string; large boundaries carry a compact `expand` descriptor
// so the file stays small. `valid` is the shared expectation. `expect` records
// an explicit per-language difference where two parsers legitimately disagree.
// `expand` descriptors only build input; they never validate.
//
// expand kinds (input builders):
//   {kind:'paragraphs', count}          count paragraph blocks, one run each
//   {kind:'runs', count, groups?}       runs distributed across paragraphs
//   {kind:'cells', count, groups?}      cells distributed across table rows
//   {kind:'text', bytes, linkBytes?, groups?, character?}
//                                       UTF-8 bytes across runs, optional link
//   {kind:'assets', count, length, width, height}
//                                       count image blocks and matching assets
//   {kind:'partials', count, detailBytes}
//                                       count partial reasons with detail text
// ---------------------------------------------------------------------------
const documentStyle = { bold: false, italic: false, underline: false, strikethrough: false, subscript: false, superscript: false };
const documentRun = (text, link) => ({ text, style: { ...documentStyle }, unsupported: false, ...(link === undefined ? {} : { link }) });
const documentParagraph = (runs, extra = {}) => ({ runs: runs.map(run => typeof run === 'string' ? documentRun(run) : run), ...extra });
const documentValue = (blocks, extra = {}) => ({ version: 1, blocks, assets: [], partial: [], styles_partial: false, ...extra });
const documentAsset = (overrides = {}) => ({ index: 0, offset: 0, length: 24, width: 1, height: 1, sha256: '0a'.repeat(32), ...overrides });
const documentImage = (asset, ordinal, availability = 'available') => ({ kind: 'image', image: { asset, availability, ordinal } });
const linkDocument = link => documentValue([{ kind: 'paragraph', paragraph: documentParagraph([documentRun('reading', link)]) }]);
const documentCorpus = [];
const corpusValue = (name, valid, value) => documentCorpus.push({ name, valid, value });
const corpusExpand = (name, valid, expand) => documentCorpus.push({ name, valid, expand });
const corpusRaw = (name, valid, json) => documentCorpus.push({ name, valid, json });
const corpusLink = (name, link, valid, expect) => documentCorpus.push({ name, valid, ...(expect ? { expect } : {}), value: linkDocument(link) });

corpusValue('paragraph unicode text', true, documentValue([{ kind: 'paragraph', paragraph: documentParagraph([documentRun('中文😀 reading')]) }]));
corpusValue('paragraph empty text', true, documentValue([{ kind: 'paragraph', paragraph: documentParagraph([documentRun('')]) }]));
corpusValue('legacy reading fields omitted', true, { version: 1, blocks: [{ kind: 'paragraph', paragraph: { runs: [{ text: 'hi', style: { ...documentStyle }, unsupported: false }] } }], assets: [], partial: [], styles_partial: false });
corpusValue('paragraph reading semantics at limit', true, documentValue([{ kind: 'paragraph', paragraph: documentParagraph(['x'], { list: 'ordered', list_depth: 8, heading: 3, quote_depth: 8, code_block: true }) }]));
corpusValue('paragraph depth without list', false, documentValue([{ kind: 'paragraph', paragraph: documentParagraph(['x'], { list_depth: 1 }) }]));
corpusValue('paragraph quote depth over limit', false, documentValue([{ kind: 'paragraph', paragraph: documentParagraph(['x'], { quote_depth: 9 }) }]));
corpusValue('paragraph heading zero', false, documentValue([{ kind: 'paragraph', paragraph: documentParagraph(['x'], { heading: 0 }) }]));
corpusValue('paragraph heading over limit', false, documentValue([{ kind: 'paragraph', paragraph: documentParagraph(['x'], { heading: 4 }) }]));
corpusValue('paragraph block carrying a table', false, documentValue([{ kind: 'paragraph', paragraph: documentParagraph(['x']), table: { rows: [], truncated: false } }]));
corpusValue('table block carrying a paragraph', false, documentValue([{ kind: 'table', paragraph: documentParagraph(['x']), table: { rows: [{ cells: [{ header: false, runs: [documentRun('x')] }] }], truncated: false } }]));
corpusValue('image block carrying a table', false, documentValue([{ kind: 'image', image: { asset: null, availability: 'external_reference', ordinal: 0 }, table: { rows: [], truncated: false } }]));
corpusValue('paragraph block without payload', false, documentValue([{ kind: 'paragraph' }]));
corpusValue('unknown block kind', false, documentValue([{ kind: 'future', paragraph: documentParagraph(['x']) }]));
corpusValue('table block', true, documentValue([{ kind: 'table', table: { rows: [{ cells: [{ header: true, runs: [documentRun('x')] }, { header: false, runs: [documentRun('y')] }] }], truncated: true } }]));
corpusValue('table row without cells', false, documentValue([{ kind: 'table', table: { rows: [{ cells: [] }], truncated: false } }]));
corpusValue('unsupported version', false, documentValue([{ kind: 'paragraph', paragraph: documentParagraph(['x']) }], { version: 2 }));
corpusValue('image available with asset', true, documentValue([documentImage(0, 0)], { assets: [documentAsset()], first_image: 0 }));
corpusValue('image available without asset', false, documentValue([documentImage(null, 0)]));
corpusValue('image missing source with asset', false, documentValue([documentImage(0, 0, 'missing_source')], { assets: [documentAsset()] }));
corpusValue('image external reference without asset', true, documentValue([documentImage(null, 0, 'external_reference')]));
corpusValue('image ordinal out of order', false, documentValue([documentImage(0, 1), documentImage(1, 0)], { assets: [documentAsset(), documentAsset({ index: 1, offset: 24 })], first_image: 0 }));
corpusValue('image asset out of range', false, documentValue([documentImage(1, 0)], { assets: [documentAsset()] }));
corpusValue('unreferenced asset', false, documentValue([{ kind: 'paragraph', paragraph: documentParagraph(['x']) }], { assets: [documentAsset()] }));
corpusValue('first image mismatch', false, documentValue([documentImage(0, 0)], { assets: [documentAsset()], first_image: 1 }));
corpusValue('first image without assets', false, documentValue([{ kind: 'paragraph', paragraph: documentParagraph(['x']) }], { first_image: 0 }));
corpusValue('asset index mismatch', false, documentValue([documentImage(0, 0), documentImage(1, 1)], { assets: [documentAsset(), documentAsset({ index: 0, offset: 24 })], first_image: 0 }));
corpusValue('asset offset not contiguous', false, documentValue([documentImage(0, 0), documentImage(1, 1)], { assets: [documentAsset(), documentAsset({ index: 1, offset: 25 })], first_image: 0 }));
corpusValue('asset zero length', false, documentValue([documentImage(0, 0)], { assets: [documentAsset({ length: 0 })], first_image: 0 }));
corpusValue('asset zero width', false, documentValue([documentImage(0, 0)], { assets: [documentAsset({ width: 0 })], first_image: 0 }));
corpusValue('asset zero height', false, documentValue([documentImage(0, 0)], { assets: [documentAsset({ height: 0 })], first_image: 0 }));
corpusValue('asset digest non hex', false, documentValue([documentImage(0, 0)], { assets: [documentAsset({ sha256: 'zz'.repeat(32) })], first_image: 0 }));
corpusValue('asset digest short', false, documentValue([documentImage(0, 0)], { assets: [documentAsset({ sha256: '0a'.repeat(31) })], first_image: 0 }));
corpusValue('asset digest uppercase', true, documentValue([documentImage(0, 0)], { assets: [documentAsset({ sha256: 'AB'.repeat(32) })], first_image: 0 }));
corpusValue('asset digest mixed case', true, documentValue([documentImage(0, 0)], { assets: [documentAsset({ sha256: 'aB'.repeat(32) })], first_image: 0 }));
corpusValue('partial detail at limit', true, documentValue([{ kind: 'paragraph', paragraph: documentParagraph(['x']) }], { partial: [{ code: 'other_native_items', detail: 'd'.repeat(512) }] }));
corpusValue('partial detail over limit', false, documentValue([{ kind: 'paragraph', paragraph: documentParagraph(['x']) }], { partial: [{ code: 'other_native_items', detail: 'd'.repeat(513) }] }));
corpusValue('partial detail with nul', false, documentValue([{ kind: 'paragraph', paragraph: documentParagraph(['x']) }], { partial: [{ code: 'other_native_items', detail: 'a\0b' }] }));

for (const [name, link, valid] of [
  ['link https path', 'https://example.com/path?q=1&r=2#frag', true],
  ['link http host', 'http://example.com', true],
  ['link uppercase scheme', 'HTTP://example.com/reading', true],
  ['link unicode path', 'https://example.com/中文😀', true],
  ['link percent nul path', 'https://example.com/%00x', true],
  ['link at sign in path', 'https://example.com/@reading', true],
  ['link zero width space', 'https://example.com/\u200breading', true],
  ['link byte order mark', 'https://example.com/\ufeffreading', true],
  ['link mongolian vowel separator', 'https://example.com/\u180ex', true],
  ['link word joiner', 'https://example.com/\u2060x', true],
  ['link nbsp', 'https://example.com/\u00a0reading', false],
  ['link ideographic space', 'https://example.com/\u3000reading', false],
  ['link line separator', 'https://example.com/\u2028reading', false],
  ['link next line', 'https://example.com/\u0085reading', false],
  ['link c0 control', 'https://example.com/\u0007reading', false],
  ['link c1 control', 'https://example.com/\u009freading', false],
  ['link backslash', 'https://example.com/a\\b', false],
  ['link space', 'https://example.com/a b', false],
  ['link tab', 'https://example.com/a\tb', false],
  ['link empty', '', false],
  ['link relative', '/relative/path', false],
  ['link javascript scheme', 'javascript:alert(1)', false],
  ['link data scheme', 'data:text/html,x', false],
  ['link file scheme', 'file:///etc/passwd', false],
  ['link userinfo', 'https://user:pass@example.com/', false],
  ['link empty authority user', 'https://@example.com/', false],
  ['link empty authority', 'https:///example.com/x', false],
  ['link scheme only', 'https://', false],
  ['link protocol relative', '//example.com/reading', false],
]) corpusLink(name, link, valid);

// URL parser probes (F8 parent). Rust and JavaScript share the WHATWG/IPv4/IPv6
// grammar; Swift URLComponents accepts a few forms they reject. The port range
// and decoded-host control/credential checks are narrowed to the same security
// boundary, while the IPv6 literal and dotted-IPv4 grammar differences are
// recorded per language instead of re-implementing a parser.
for (const [name, link, valid, expect] of [
  ['url port 65535', 'https://example.com:65535/', true],
  ['url port 65536', 'https://example.com:65536/', false],
  ['url port 999999', 'https://example.com:999999/', false],
  ['url port negative', 'https://example.com:-1/', false],
  ['url port non numeric', 'https://example.com:abc/', false],
  ['url port empty', 'https://example.com:/', true],
  ['url ipv6 loopback', 'https://[::1]/', true],
  ['url ipv6 literal invalid', 'https://[invalid]/', false, { swift: true }],
  ['url trailing dot host', 'https://example.com./', true],
  ['url host percent invalid', 'https://exa%mple.com/', false],
  ['url host percent space', 'https://exa%20mple.com/', false],
  ['url host percent nul', 'https://exa%00mple.com/', false],
  ['url host underscore', 'https://a_b.example/', true],
  ['url host leading dot', 'https://.example/', true],
  ['url host dots only', 'https://../', true],
  ['url ipv4 out of range', 'https://127.0.0.256/', false, { swift: true }],
  ['url ipv4 short form', 'https://1.2.3/', true],
  ['url ipv4 hex form', 'https://0x7f000001/', true],
  ['url fragment at sign', 'https://example.com#@user', true],
  ['url query at sign', 'https://example.com?@user', true],
  ['url idn host', 'HTTPS://例子.测试/x', true],
  ['url path percent space', 'https://example.com/%20x', true],
  ['url host percent credential', 'https://user%40example.com/', false],
  ['url literal credential', 'https://user%3Apass@example.com/', false],
  ['url path emoji', 'https://example.com/😀', true],
  ['url path word joiner', 'https://example.com/\u2060x', true],
  ['url path vowel separator', 'https://example.com/\u180ex', true],
]) corpusLink(name, link, valid, expect);

const rawDocument = text => JSON.stringify(documentValue([{ kind: 'paragraph', paragraph: documentParagraph([documentRun(text)]) }]));
const rawLink = link => JSON.stringify(linkDocument(link));
const rawDetail = detail => JSON.stringify(documentValue([{ kind: 'paragraph', paragraph: documentParagraph(['x']) }], { partial: [{ code: 'other_native_items', detail }] }));
corpusRaw('raw text lone high surrogate', false, rawDocument('\ud800'));
corpusRaw('raw text lone low surrogate', false, rawDocument('\udfff'));
corpusRaw('raw text valid surrogate pair', true, rawDocument('\ud83d\ude00'));
corpusRaw('raw escaped valid surrogate pair', true, rawDocument('😀').replace('😀', '\\ud83d\\ude00'));
corpusRaw('raw link lone high surrogate', false, rawLink('https://example.com/\ud800'));
corpusRaw('raw detail lone low surrogate', false, rawDetail('a\udfffb'));
// Exact u64 literals: JavaScript numbers cannot round-trip a value above 2^53,
// so the offset-overflow case is carried as raw JSON instead of a descriptor.
const overflowAsset = (index, offset) => `{"index":${index},"offset":${offset},"length":9223372036854775808,"width":1,"height":1,"sha256":"${'0a'.repeat(32)}"}`;
corpusRaw('asset byte offset overflow', false,
  `{"version":1,"blocks":[{"kind":"image","image":{"asset":0,"availability":"available","ordinal":0}},{"kind":"image","image":{"asset":1,"availability":"available","ordinal":1}}],"assets":[${overflowAsset(0, '0')},${overflowAsset(1, '9223372036854775808')}],"first_image":0,"partial":[],"styles_partial":false}`);

corpusExpand('blocks at limit', true, { kind: 'paragraphs', count: 8192 });
corpusExpand('blocks over limit', false, { kind: 'paragraphs', count: 8193 });
corpusExpand('aggregate runs at limit', true, { kind: 'runs', count: 8192 });
corpusExpand('aggregate runs over limit', false, { kind: 'runs', count: 8193 });
corpusExpand('aggregate cells at limit', true, { kind: 'cells', count: 2048 });
corpusExpand('aggregate cells over limit', false, { kind: 'cells', count: 2049 });
corpusExpand('utf8 text at limit', true, { kind: 'text', bytes: 1048576 });
corpusExpand('utf8 text over limit', false, { kind: 'text', bytes: 1048577 });
corpusExpand('text and link within budget', true, { kind: 'text', bytes: 1048000, linkBytes: 576 });
corpusExpand('text and link over budget', false, { kind: 'text', bytes: 1048576, linkBytes: 576 });
corpusExpand('link at limit', true, { kind: 'text', bytes: 0, linkBytes: 2048 });
corpusExpand('link over limit', false, { kind: 'text', bytes: 0, linkBytes: 2049 });
corpusExpand('assets at limit', true, { kind: 'assets', count: 64, length: 24, width: 1, height: 1 });
corpusExpand('assets over limit', false, { kind: 'assets', count: 65, length: 24, width: 1, height: 1 });
corpusExpand('aggregate bytes at limit', true, { kind: 'assets', count: 1, length: 16777216, width: 1, height: 1 });
corpusExpand('aggregate bytes over limit', false, { kind: 'assets', count: 1, length: 16777217, width: 1, height: 1 });
corpusExpand('aggregate pixels at limit', true, { kind: 'assets', count: 1, length: 24, width: 4000, height: 4000 });
corpusExpand('aggregate pixels over limit', false, { kind: 'assets', count: 1, length: 24, width: 4001, height: 4000 });
corpusExpand('partial count at limit', true, { kind: 'partials', count: 8192, detailBytes: 1 });
corpusExpand('partial count over limit', false, { kind: 'partials', count: 8193, detailBytes: 1 });

// Parent review: isolate aggregate limits from each individual element limit.
corpusExpand('runs across paragraphs at limit', true, { kind: 'runs', count: 8192, groups: 2 });
corpusExpand('runs across paragraphs plus one', false, { kind: 'runs', count: 8193, groups: 2 });
corpusExpand('cells across rows at limit', true, { kind: 'cells', count: 2048, groups: 2 });
corpusExpand('cells across rows plus one', false, { kind: 'cells', count: 2049, groups: 2 });
corpusExpand('unicode text across runs at limit', true, { kind: 'text', bytes: 1048576, groups: 2, character: '中' });
corpusExpand('unicode text across runs plus one', false, { kind: 'text', bytes: 1048577, groups: 2, character: '中' });
corpusExpand('unicode text and link aggregate at limit', true, { kind: 'text', bytes: 1048000, linkBytes: 576, groups: 2, character: '😀' });
corpusExpand('unicode text and link aggregate plus one', false, { kind: 'text', bytes: 1048001, linkBytes: 576, groups: 2, character: '😀' });
const assetDocument = assets => documentValue(assets.map((_, index) => documentImage(index, index)), { assets, first_image: 0 });
corpusValue('bytes across assets at limit', true, assetDocument([documentAsset({ length: 8388608 }), documentAsset({ index: 1, offset: 8388608, length: 8388608 })]));
corpusValue('bytes across assets plus one', false, assetDocument([documentAsset({ length: 8388608 }), documentAsset({ index: 1, offset: 8388608, length: 8388609 })]));
const pixelAssets = [documentAsset({ width: 2000, height: 4000 }), documentAsset({ index: 1, offset: 24, width: 2000, height: 4000 })];
corpusValue('pixels across assets at limit', true, assetDocument(pixelAssets));
corpusValue('pixels across assets plus one', false, assetDocument([...pixelAssets, documentAsset({ index: 2, offset: 48 })]));
corpusLink('unicode link at byte limit', 'https://example.com/' + '中'.repeat(676), true);
corpusLink('unicode link byte limit plus one', 'https://example.com/' + '中'.repeat(676) + 'x', false);
corpusValue('unicode partial detail at byte limit', true, documentValue([], { partial: [{ code: 'other_native_items', detail: '😀'.repeat(128) }] }));
corpusValue('unicode partial detail byte limit plus one', false, documentValue([], { partial: [{ code: 'other_native_items', detail: '😀'.repeat(128) + 'x' }] }));
corpusRaw('malformed model JSON', false, '{"version":1,');

outputs['shared/schema/fixtures/document-corpus.json'] = JSON.stringify(documentCorpus,null,2) + '\n';

// Frame metadata shares its semantic cases with the real runtime codec and shells.
const frameModelJson = JSON.stringify(documentValue([]));
const frameMetadata = { request_id: 'fixture-frame', entry_id: 7, kind: 'document', name: 'document', stamp: 'dc1:' + '0a'.repeat(32), owner: 'history', generation: 0, model_bytes: Buffer.byteLength(frameModelJson), size_bytes: Buffer.byteLength(frameModelJson) };
const frameCases = [];
const frameCase = (name, valid, patch = {}) => frameCases.push({ name, valid, value: { ...frameMetadata, ...patch } });
frameCase('lowercase stamp', true);
frameCase('uppercase stamp', true, { stamp: 'dc1:' + 'AB'.repeat(32) });
frameCase('mixed case stamp', true, { stamp: 'dc1:' + 'aB'.repeat(32) });
frameCase('nonhex stamp', false, { stamp: 'dc1:' + 'zz'.repeat(32) });
frameCase('short stamp', false, { stamp: 'dc1:' + '0a'.repeat(31) });
frameCase('uppercase stamp prefix', false, { stamp: 'DC1:' + '0a'.repeat(32) });
frameCase('request UTF8 exact limit', true, { request_id: '😀'.repeat(32) });
frameCase('request UTF8 plus one', false, { request_id: '😀'.repeat(32) + 'x' });
frameCase('name UTF8 exact limit', true, { name: '中'.repeat(85) + 'x' });
frameCase('name UTF8 plus one', false, { name: '中'.repeat(85) + 'xx' });
frameCase('missing request id', false, { request_id: null });
frameCase('empty name', false, { name: '' });
frameCase('safe generation exact limit', true, { generation: 9007199254740991 });
frameCase('generation plus one', false, { generation: 9007199254740992 });
frameCase('unknown owner', false, { owner: 'future' });
frameCase('unknown kind', false, { kind: 'image' });
frameCase('zero model bytes', false, { model_bytes: 0 });
for (const [name, patch] of [['request lone surrogate', { request_id: '\ud800' }], ['name lone surrogate', { name: '\udfff' }]]) {
  frameCases.push({ name, valid: false, json: JSON.stringify({ ...frameMetadata, ...patch }) });
}
outputs['shared/schema/fixtures/document-frame-corpus.json'] = JSON.stringify({ model_json: frameModelJson, cases: frameCases }, null, 2) + '\n';

outputs['macos/swift-ui/Tests/TailSyncTests/LocalContractFixtures.generated.swift'] = banner + `import Foundation\n@testable import TailSync\n\nfunc validateGeneratedFixture(_ contract: String, data: Data) throws {\n  switch contract {\n${Object.keys(defs).map(name => `  case "${name}": _ = try JSONDecoder().decode(Contract${name}.self, from: data)`).join('\n')}\n  default: throw NSError(domain: "Unknown contract fixture", code: 1)\n  }\n}\n`;
for (const [file, content] of Object.entries(outputs)) {
  const path = resolve(root,file);
  if (process.argv.includes('--check')) {
    if (readFileSync(path,'utf8').replaceAll('\r\n','\n') !== content) throw new Error(`${file} is stale`);
  } else writeFileSync(path,content);
}
console.log(`Generated local contracts: ${Object.keys(defs).length} production DTOs`);
