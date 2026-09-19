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
  } else if (s.type === 'null') checks.push(`${value} === null`);
  else if (s.type === 'integer' || s.type === 'number') {
    checks.push(`typeof ${value} === "number"`, `Number.${s.type === 'integer' ? 'isSafeInteger' : 'isFinite'}(${value})`);
    if (s.minimum != null) checks.push(`${value} >= ${s.minimum}`);
    if (s.maximum != null) checks.push(`${value} <= ${s.maximum}`);
  } else if (s.type) checks.push(`typeof ${value} === ${quote(s.type)}`);
  if (s.enum) checks.push(`(${s.enum.map(item => `${value} === ${quote(item)}`).join(' || ')})`);
  if (s.minLength != null) checks.push(`${value}.length >= ${s.minLength}`);
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
function swiftValidation(s, expression) {
  const value = nullable(s) ? 'value' : expression;
  const checks = [];
  if (s.enum) checks.push(`${quote(s.enum)}.contains(${value})`);
  if (s.minimum != null && !(s.minimum === 0 && /^uint/.test(s.format ?? ''))) checks.push(`${value} >= ${s.minimum}`);
  if (s.maximum != null && !(s.maximum === 255 && s.format === 'uint8')) checks.push(`${value} <= ${s.maximum}`);
  if (s.minLength != null) checks.push(`${value}.count >= ${s.minLength}`);
  if (!checks.length) return '';
  const guard = `guard ${checks.join(', ')} else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Local contract constraint failed")) }`;
  return nullable(s) ? `    if let value = ${expression} { ${guard} }\n` : `    ${guard}\n`;
}
function swiftDefinition(name, s) {
  if (s.enum) return `enum Contract${name}: String, Codable, Sendable {\n${s.enum.map(v => `  case \`${v}\` = ${quote(v)}`).join('\n')}\n}\n`;
  if (s.type !== 'object') return `typealias Contract${name} = ${swiftType(s)}\n`;
  const fields = Object.entries(s.properties);
  const type = (key, value) => swiftType(value) + (!s.required?.includes(key) && !nullable(value) ? '?' : '');
  let text = `struct Contract${name}: Codable, Sendable {\n`;
  text += fields.map(([key,value]) => `  let \`${key}\`: ${type(key,value)}`).join('\n') + '\n';
  text += `  private enum CodingKeys: String, CodingKey { case ${fields.map(([key]) => `\`${key}\``).join(', ')} }\n`;
  text += '  init(from decoder: Decoder) throws {\n    let c = try decoder.container(keyedBy: CodingKeys.self)\n';
  for (const [key,value] of fields) {
    const required = s.required?.includes(key);
    const base = swiftType(nullable(value) ? nonNull(value)[0] : value);
    const access = `.${'`'+key+'`'}`;
    if (required && nullable(value)) text += `    guard c.contains(${access}) else { throw DecodingError.keyNotFound(CodingKeys.\`${key}\`, .init(codingPath: decoder.codingPath, debugDescription: "Required nullable field is missing")) }\n`;
    const decode = `try c.${nullable(value) ? 'decodeIfPresent' : 'decode'}(${base}.self, forKey: ${access})`;
    text += `    self.\`${key}\` = ${!required && !nullable(value) ? `c.contains(${access}) ? ${decode} : nil` : decode}\n`;
    text += swiftValidation(value, `self.\`${key}\``);
  }
  return text + '  }\n}\n';
}

const banner = '// Generated from production Rust DTOs by shared/schema/generate-local-contracts.mjs. Do not edit.\n';
let ts = banner + 'const isRecord = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === "object" && !Array.isArray(value);\n';
let js = banner + 'const isRecord = value => value !== null && typeof value === "object" && !Array.isArray(value);\n';
let swift = banner + 'import Foundation\n\n';
for (const [name, s] of Object.entries(defs)) {
  const body = `return ${predicate(s,'value')};`;
  ts += `\nexport type ${name} = ${tsType(s)};\nfunction valid${name}(value: unknown): value is ${name} { ${body} }\n`;
  ts += `export function decode${name}(value: unknown): ${name} { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); return value; }\n`;
  js += `\nfunction valid${name}(value) { ${body} }\nexport function decode${name}(value) { if (!valid${name}(value)) throw new Error("Invalid ${name} response"); return value; }\n`;
  swift += swiftDefinition(name,s) + '\n';
}
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
const fixtures = [];
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
  } else if (definition.enum) fixtures.push({ name: `${name}: unknown enum`, contract: name, valid: false, value: 'unknown_future_enum' });
}
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
