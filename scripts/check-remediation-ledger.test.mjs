import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { validateLedger, parseCiJobs, renderIndex } from './check-remediation-ledger.mjs';

function fixture(overrides = {}) {
  const root = mkdtempSync(join(tmpdir(), 'ledger-'));
  mkdirSync(join(root, 'docs/remediation-ledger/entries'), { recursive: true });
  mkdirSync(join(root, 'src'), { recursive: true });
  mkdirSync(join(root, '.github/workflows'), { recursive: true });
  writeFileSync(join(root, '.github/workflows/ci.yml'), 'jobs:\n  rust-macos:\n    runs-on: macos-latest\n  scripts:\n    runs-on: ubuntu-latest\n');
  writeFileSync(join(root, 'src/lib.rs'), 'fn real_gate() {}\n');
  const entry = {
    id: 'S1-P0-1', level: 'P0', title: 'a title long enough', status: 'fixed_gated',
    platform: ['shared'], code_symbols: ['src/lib.rs:x'], trigger: 'the trigger text',
    fix_evidence: 'the fix evidence', gate: { file: 'src/lib.rs', test: 'real_gate', command: 'cargo test', asserts: 'asserts something real' },
    ci_job: ['rust-macos'], native_acceptance: null, pass_condition: 'pass when green',
    reopen_condition: 'reopen if red', last_verified_main_sha: 'b61a0a04b88f5828707f7ef64781389944471be8',
  };
  const manifest = {
    schema_version: 1, initial_audit_sha: 'b61a0a04b88f5828707f7ef64781389944471be8', baseline_date: '2026-09-29',
    expected_ids: ['S1-P0-1'],
  };
  writeFileSync(join(root, 'docs/remediation-ledger/manifest.json'), JSON.stringify({ ...manifest, ...overrides.manifest }));
  writeFileSync(join(root, 'docs/remediation-ledger/entries/S1-P0-1.json'), JSON.stringify({ ...entry, ...overrides.entry }));
  return root;
}

test('parseCiJobs extracts job names and stops at the block end', () => {
  const jobs = parseCiJobs('name: CI\njobs:\n  rust-macos:\n    runs-on: macos-latest\n  scripts:\n    runs-on: ubuntu-latest\n');
  assert.deepEqual(jobs, ['rust-macos', 'scripts']);
});

test('a well-formed ledger validates clean', () => {
  const root = fixture();
  assert.deepEqual(validateLedger(root), []);
  rmSync(root, { recursive: true, force: true });
});

test('fixed_gated without a gate is rejected', () => {
  const root = fixture({ entry: { gate: null } });
  const errors = validateLedger(root);
  assert.ok(errors.some((e) => /requires a gate/.test(e)), errors.join('; '));
  rmSync(root, { recursive: true, force: true });
});

test('a gate whose test does not exist is rejected', () => {
  const root = fixture({ entry: { gate: { file: 'src/lib.rs', test: 'not_a_real_test', command: 'cargo test', asserts: 'asserts something real' } } });
  const errors = validateLedger(root);
  assert.ok(errors.some((e) => /not found in/.test(e)), errors.join('; '));
  rmSync(root, { recursive: true, force: true });
});

test('a gate pointing at a missing file is rejected', () => {
  const root = fixture({ entry: { gate: { file: 'src/missing.rs', test: 'real_gate', command: 'cargo test', asserts: 'asserts something real' } } });
  const errors = validateLedger(root);
  assert.ok(errors.some((e) => /does not exist/.test(e)), errors.join('; '));
  rmSync(root, { recursive: true, force: true });
});

test('an unknown ci_job is rejected', () => {
  const root = fixture({ entry: { ci_job: ['rust-nonexistent'] } });
  const errors = validateLedger(root);
  assert.ok(errors.some((e) => /is not a job in/.test(e)), errors.join('; '));
  rmSync(root, { recursive: true, force: true });
});

test('a bad status is rejected', () => {
  const root = fixture({ entry: { status: 'done' } });
  const errors = validateLedger(root);
  assert.ok(errors.some((e) => /bad status/.test(e)), errors.join('; '));
  rmSync(root, { recursive: true, force: true });
});

test('a required field missing is rejected', () => {
  const root = fixture();
  const p = join(root, 'docs/remediation-ledger/entries/S1-P0-1.json');
  const data = JSON.parse(readFileSync(p, 'utf8'));
  delete data.reopen_condition;
  writeFileSync(p, JSON.stringify(data));
  const errors = validateLedger(root);
  assert.ok(errors.some((e) => /missing required field "reopen_condition"/.test(e)), errors.join('; '));
  rmSync(root, { recursive: true, force: true });
});

test('a manifest id without an entry file is rejected', () => {
  const root = fixture({ manifest: { expected_ids: ['S1-P0-1', 'S2-F9'] } });
  const errors = validateLedger(root);
  assert.ok(errors.some((e) => /no entry file exists/.test(e)), errors.join('; '));
  rmSync(root, { recursive: true, force: true });
});

test('renderIndex lists every entry', () => {
  const root = fixture();
  const { manifest, entries } = { manifest: { initial_audit_sha: 'a'.repeat(40), baseline_date: '2026-09-29' }, entries: [{ id: 'S1-P0-1', level: 'P0', status: 'fixed_gated', gate: { test: 'real_gate' }, ci_job: ['rust-macos'] }] };
  const md = renderIndex(manifest, entries);
  assert.match(md, /\| S1-P0-1 \| P0 \| fixed_gated \| `real_gate` \| rust-macos \|/);
  rmSync(root, { recursive: true, force: true });
});
