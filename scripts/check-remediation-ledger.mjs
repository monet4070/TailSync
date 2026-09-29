#!/usr/bin/env node
// Validates docs/remediation-ledger against its own schema.json and against the
// repository: ID set, status/gate consistency, that each named gate test exists
// in the named file, that the gate's command actually compiles that file, and
// that every declared ci_job runs a command covering the gate.
//
// Usage:
//   node scripts/check-remediation-ledger.mjs [--root .] [--write]
import { readFileSync, readdirSync, existsSync, writeFileSync, realpathSync, statSync } from 'node:fs';
import { join, dirname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';

const STATUSES = ['fixed_gated', 'fixed_ungated', 'partial', 'unfixed', 'needs_adjudication'];

const norm = (p) => p.replaceAll('\\', '/');

// ---------- workflow parsing ----------

export function parseCiJobs(workflowText) {
  const lines = workflowText.split('\n');
  const start = lines.findIndex((l) => /^jobs:\s*$/.test(l));
  if (start === -1) return {};
  const jobs = {};
  let current = null;
  for (let i = start + 1; i < lines.length; i++) {
    const line = lines[i];
    if (/^\S/.test(line)) break;
    const jobMatch = /^ {2}([A-Za-z0-9_-]+):\s*$/.exec(line);
    if (jobMatch) {
      current = jobMatch[1];
      jobs[current] = [];
      continue;
    }
    if (!current) continue;
    const runMatch = /^\s*(?:-\s*)?run:\s*(.*)$/.exec(line);
    if (runMatch) {
      jobs[current].push(runMatch[1]);
      const indent = line.search(/\S/);
      // block scalar: absorb following more-indented lines
      for (let j = i + 1; j < lines.length; j++) {
        const next = lines[j];
        if (next.trim() === '') continue;
        const nextIndent = next.search(/\S/);
        if (nextIndent <= indent) break;
        jobs[current][jobs[current].length - 1] += '\n' + next.trim();
      }
    }
  }
  return jobs;
}

function rustFilesUnder(root, dir) {
  const abs = join(root, dir);
  if (!existsSync(abs)) return [];
  const out = [];
  const walk = (d) => {
    for (const entry of readdirSync(d, { withFileTypes: true })) {
      const p = join(d, entry.name);
      if (entry.isDirectory()) walk(p);
      else if (entry.name.endsWith('.rs')) out.push(p);
    }
  };
  walk(abs);
  return out;
}

// A source directory covers `gateFile` if the file is inside it, or if some file
// in it `include!`s the file by basename.
export function dirCovers(root, dir, gateFile) {
  const d = norm(dir);
  const g = norm(gateFile);
  if (g === d || g.startsWith(d + '/')) return true;
  const target = basename(g);
  for (const file of rustFilesUnder(root, d)) {
    const text = readFileSync(file, 'utf8');
    const re = /include!\(\s*"([^"]+)"\s*\)/g;
    let m;
    while ((m = re.exec(text))) {
      if (basename(norm(m[1])) === target) return true;
    }
  }
  return false;
}

// A crate rooted at `manifestDir` covers `gateFile`.
export function manifestCovers(root, manifestPath, gateFile) {
  return dirCovers(root, dirname(manifestPath), gateFile);
}

// Does a shell command (cargo --manifest-path / swift --package-path) compile or
// build the directory that contains `gateFile`?
export function commandCovers(root, command, gateFile) {
  const manifests = [...command.matchAll(/--manifest-path\s+(\S+)/g)].map((m) => dirname(m[1]));
  const packages = [...command.matchAll(/--package-path\s+(\S+)/g)].map((m) => m[1]);
  return [...manifests, ...packages].some((dir) => dirCovers(root, dir, gateFile));
}

// ---------- minimal JSON-schema subset ----------

function validateSchema(schema, value, path, errors) {
  if (schema.oneOf) {
    const branches = schema.oneOf;
    const nullBranch = branches.some((b) => b.type === 'null');
    if (value === null) {
      if (!nullBranch) errors.push(`${path}: null is not allowed`);
      return;
    }
    const objectBranch = branches.find((b) => b.type === 'object');
    if (objectBranch) return validateSchema(objectBranch, value, path, errors);
    return;
  }
  if (schema.type) {
    const types = Array.isArray(schema.type) ? schema.type : [schema.type];
    const actual = value === null ? 'null' : Array.isArray(value) ? 'array' : typeof value;
    if (!types.includes(actual)) {
      errors.push(`${path}: expected ${types.join('|')}, got ${actual}`);
      return;
    }
  }
  if (schema.enum && !schema.enum.includes(value)) {
    errors.push(`${path}: "${value}" is not one of ${schema.enum.join(', ')}`);
  }
  if (typeof value === 'string') {
    if (schema.minLength && value.length < schema.minLength) errors.push(`${path}: shorter than minLength ${schema.minLength}`);
    if (schema.pattern && !new RegExp(schema.pattern).test(value)) errors.push(`${path}: does not match ${schema.pattern}`);
  }
  if (Array.isArray(value)) {
    if (schema.minItems && value.length < schema.minItems) errors.push(`${path}: needs at least ${schema.minItems} item(s)`);
    if (schema.items) value.forEach((v, i) => validateSchema(schema.items, v, `${path}[${i}]`, errors));
  }
  if (value && typeof value === 'object' && !Array.isArray(value) && schema.properties) {
    for (const key of schema.required || []) {
      if (!(key in value)) errors.push(`${path}: missing required field "${key}"`);
    }
    if (schema.additionalProperties === false) {
      for (const key of Object.keys(value)) {
        if (!(key in schema.properties)) errors.push(`${path}: unknown field "${key}"`);
      }
    }
    for (const [key, sub] of Object.entries(schema.properties)) {
      if (key in value) validateSchema(sub, value[key], `${path}.${key}`, errors);
    }
  }
}

// ---------- ledger ----------

export function loadLedger(root) {
  const dir = join(root, 'docs/remediation-ledger');
  const manifest = JSON.parse(readFileSync(join(dir, 'manifest.json'), 'utf8'));
  const entriesDir = join(dir, 'entries');
  const files = existsSync(entriesDir) ? readdirSync(entriesDir).filter((f) => f.endsWith('.json')).sort() : [];
  const entries = files.map((f) => ({
    file: `docs/remediation-ledger/entries/${f}`,
    data: JSON.parse(readFileSync(join(entriesDir, f), 'utf8')),
  }));
  return { dir, manifest, entries };
}

export function renderIndex(manifest, entries) {
  const byStatus = {};
  for (const e of entries) (byStatus[e.status] ||= []).push(e.id);
  const rows = entries
    .slice()
    .sort((a, b) => a.id.localeCompare(b.id))
    .map((e) => `| ${e.id} | ${e.level} | ${e.status} | ${e.gate ? '`' + e.gate.test + '`' : '—'} | ${e.ci_job.length ? e.ci_job.join(', ') : '—'} |`)
    .join('\n');
  const counts = STATUSES.map((s) => `${s}=${(byStatus[s] || []).length}`).join(', ');
  return [
    '# Remediation ledger index',
    '',
    '> Generated by `scripts/check-remediation-ledger.mjs --write`. Do not edit by hand.',
    `> Initial audit baseline: \`${manifest.initial_audit_sha}\` (${manifest.baseline_date}).`,
    `> Entries: ${entries.length}. Status counts: ${counts}.`,
    '',
    '| ID | Level | Status | Gate test | CI jobs |',
    '|---|---|---|---|---|',
    rows,
    '',
  ].join('\n');
}

export function validateLedger(root) {
  const errors = [];
  const schemaPath = join(root, 'docs/remediation-ledger/schema.json');
  if (!existsSync(schemaPath)) return ['docs/remediation-ledger/schema.json is missing'];
  let schema;
  try {
    schema = JSON.parse(readFileSync(schemaPath, 'utf8'));
  } catch (err) {
    return [`schema.json is not valid JSON: ${err.message}`];
  }
  let ledger;
  try {
    ledger = loadLedger(root);
  } catch (err) {
    return [`cannot load ledger: ${err.message}`];
  }
  const { manifest, entries } = ledger;

  const expected = new Set(manifest.expected_ids || []);
  if (expected.size !== (manifest.expected_ids || []).length) errors.push('manifest.expected_ids contains duplicates');

  const workflowPath = join(root, '.github/workflows/ci.yml');
  if (!existsSync(workflowPath)) return ['registry: .github/workflows/ci.yml is missing'];
  const jobs = parseCiJobs(readFileSync(workflowPath, 'utf8'));

  const seen = new Set();
  const idCounts = new Map();
  for (const { file, data } of entries) {
    if (typeof data.id === 'string') {
      // register before shape checks so a malformed entry does not cascade into
      // a bogus "no entry file exists" error
      seen.add(data.id);
      idCounts.set(data.id, (idCounts.get(data.id) || 0) + 1);
    }
    const before = errors.length;
    validateSchema(schema, data, file, errors);
    if (errors.length !== before) continue; // field-shape errors make rule checks unreliable

    const expectedFile = `${data.id}.json`;
    if (!file.endsWith(expectedFile)) errors.push(`${file}: filename does not match id (${expectedFile})`);
    if ((idCounts.get(data.id) || 0) > 1) errors.push(`${file}: duplicate id ${data.id}`);
    if (!expected.has(data.id)) errors.push(`${file}: id ${data.id} is not in manifest.expected_ids`);
    if (data.status === 'fixed_gated' && !data.gate) errors.push(`${file}: status fixed_gated requires a gate`);

    if (data.gate) {
      const g = data.gate;
      const src = join(root, g.file);
      const fileExists = existsSync(src);
      if (!fileExists) errors.push(`${file}: gate.file does not exist: ${g.file}`);
      if (g.kind === 'integration') {
        // An integration gate is a named step in a real build/smoke script: the
        // assertion is a literal marker in that script, not a unit test symbol.
        if (!g.marker) {
          errors.push(`${file}: integration gate requires a marker`);
        } else if (fileExists && !readFileSync(src, 'utf8').includes(g.marker)) {
          errors.push(`${file}: integration gate marker not found in ${g.file}`);
        }
      } else {
        if (!g.test) {
          errors.push(`${file}: unit gate requires a test`);
        } else if (fileExists) {
          const text = readFileSync(src, 'utf8');
          const re = new RegExp(`\\b(?:async\\s+)?(?:fn|func)\\s+${g.test.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\s*[<(]`);
          if (!re.test(text)) errors.push(`${file}: gate test "${g.test}" not found in ${g.file}`);
        }
        // the gate command must compile/build the file the test lives in
        if (!/--(?:manifest|package)-path\s+\S+/.test(g.command)) {
          errors.push(`${file}: gate.command must name a --manifest-path or --package-path`);
        } else if (!commandCovers(root, g.command, g.file)) {
          errors.push(`${file}: gate.command does not compile/build ${g.file}`);
        }
      }
    }

    for (const job of data.ci_job) {
      if (!(job in jobs)) {
        errors.push(`${file}: ci_job "${job}" is not a job in .github/workflows/ci.yml`);
        continue;
      }
      if (!data.gate) continue;
      const jobText = jobs[job].join('\n');
      const jobCovers =
        data.gate.kind === 'integration'
          ? jobText.includes(data.gate.file) || jobText.includes(basename(data.gate.file))
          : commandCovers(root, jobText, data.gate.file);
      if (!jobCovers) {
        errors.push(`${file}: ci_job "${job}" runs no command covering gate.file (${data.gate.file})`);
      }
    }
  }

  for (const id of expected) if (!seen.has(id)) errors.push(`manifest lists ${id} but no entry file exists`);
  for (const id of seen) if (!expected.has(id)) errors.push(`entry ${id} is not listed in manifest.expected_ids`);

  // declared counts must match the entries, otherwise they drift silently
  const statusCounts = {};
  const levelCounts = {};
  for (const { data } of entries) {
    if (typeof data.status === 'string') statusCounts[data.status] = (statusCounts[data.status] || 0) + 1;
    if (typeof data.level === 'string') levelCounts[data.level] = (levelCounts[data.level] || 0) + 1;
  }
  for (const [key, declared] of Object.entries(manifest.status_counts || {})) {
    const actual = statusCounts[key] || 0;
    if (declared !== actual) errors.push(`manifest.status_counts.${key} is ${declared} but entries contain ${actual}`);
  }
  for (const [key, declared] of Object.entries(manifest.level_counts || {})) {
    const actual = levelCounts[key] || 0;
    if (declared !== actual) errors.push(`manifest.level_counts.${key} is ${declared} but entries contain ${actual}`);
  }
  if (manifest.expected_ids && entries.length !== manifest.expected_ids.length) {
    errors.push(`entries count ${entries.length} != manifest.expected_ids length ${manifest.expected_ids.length}`);
  }

  return errors;
}

function main() {
  const args = process.argv.slice(2);
  const rootIdx = args.indexOf('--root');
  const root = rootIdx !== -1 ? args[rootIdx + 1] : '.';
  const errors = validateLedger(root);
  if (errors.length) {
    console.error(`remediation ledger: ${errors.length} error(s)`);
    for (const e of errors) console.error('  - ' + e);
    process.exit(1);
  }
  const { manifest, entries } = loadLedger(root);
  if (args.includes('--write')) {
    writeFileSync(join(root, 'docs/remediation-ledger/INDEX.md'), renderIndex(manifest, entries.map((e) => e.data)));
    console.log('wrote docs/remediation-ledger/INDEX.md');
  }
  console.log(`remediation ledger OK: ${entries.length} entries, baseline ${manifest.initial_audit_sha.slice(0, 12)}`);
}

function isMain() {
  if (!process.argv[1]) return false;
  try {
    return realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
}
if (isMain()) main();
