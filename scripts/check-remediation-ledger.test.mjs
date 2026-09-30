import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateLedger, parseCiJobs, renderIndex, manifestCovers } from './check-remediation-ledger.mjs';

const REAL_SCHEMA = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), '..', 'docs/remediation-ledger/schema.json'),
  'utf8',
);

function makeEntry(overrides = {}) {
  const merged = {
    id: 'S1-P0-1',
    level: 'P0',
    title: 'a title long enough',
    status: 'fixed_gated',
    platform: ['shared'],
    code_symbols: ['crates/app/src/lib.rs:thing'],
    trigger: 'the trigger text',
    fix_evidence: 'the fix evidence text',
    gate: {
      kind: 'unit',
      file: 'crates/app/src/lib.rs',
      test: 'real_gate',
      command: 'cargo test --locked --manifest-path crates/app/Cargo.toml',
      asserts: 'asserts something real',
    },
    ci_job: ['rust-macos'],
    native_acceptance: null,
    pass_condition: 'pass when green',
    reopen_condition: 'reopen if red',
    last_verified_main_sha: 'b61a0a04b88f5828707f7ef64781389944471be8',
    ...overrides,
  };
  // unit is the default gate kind; tests may override it explicitly
  if (merged.gate && typeof merged.gate === 'object' && !merged.gate.kind) {
    merged.gate = { kind: 'unit', ...merged.gate };
  }
  return merged;
}

function fixture({ entry = {}, manifest = {}, ci = 'jobs:\n  rust-macos:\n    runs-on: macos-latest\n    steps:\n      - run: node scripts/run-remediation-tests.mjs --job rust-macos -- cargo test --locked --manifest-path crates/app/Cargo.toml\n      - run: node scripts/check-remediation-ledger.mjs --job rust-macos --results-dir results\n  scripts:\n    runs-on: ubuntu-latest\n' } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'ledger-'));
  mkdirSync(join(root, 'docs/remediation-ledger/entries'), { recursive: true });
  mkdirSync(join(root, 'crates/app/src'), { recursive: true });
  mkdirSync(join(root, '.github/workflows'), { recursive: true });
  writeFileSync(join(root, 'docs/remediation-ledger/schema.json'), REAL_SCHEMA);
  writeFileSync(join(root, '.github/workflows/ci.yml'), ci);
  writeFileSync(join(root, 'crates/app/Cargo.toml'), '[package]\nname = "app"\n');
  writeFileSync(join(root, 'crates/app/src/lib.rs'), '#[test]\nfn real_gate() {}\n');
  const data = makeEntry(entry);
  writeFileSync(
    join(root, 'docs/remediation-ledger/manifest.json'),
    JSON.stringify({
      schema_version: 1,
      initial_audit_sha: 'b61a0a04b88f5828707f7ef64781389944471be8',
      baseline_date: '2026-09-29',
      audit_source: 'docs/audit/CODE-REVIEW-2026-09-27.md',
      plan_source: 'docs/audit/CODE-REVIEW-REMEDIATION-PLAN-2026-09-29.md',
      expected_ids: [data.id],
      ...manifest,
    }),
  );
  writeFileSync(join(root, `docs/remediation-ledger/entries/${data.id}.json`), JSON.stringify(data));
  return { root, data };
}

test('parseCiJobs collects run commands per job and stops at the block end', () => {
  const jobs = parseCiJobs('name: CI\njobs:\n  rust-macos:\n    runs-on: macos-latest\n    steps:\n      - run: cargo test a\n  scripts:\n    runs-on: ubuntu-latest\n    steps:\n      - run: node x.mjs\n');
  assert.deepEqual(Object.keys(jobs), ['rust-macos', 'scripts']);
  assert.deepEqual(jobs['rust-macos'], ['cargo test a']);
  assert.deepEqual(jobs.scripts, ['node x.mjs']);
});

test('parseCiJobs reads single-line and block commands identically with LF and CRLF', () => {
  const workflow = `name: CI
jobs:
  rust-windows:
    steps:
      - run: node scripts/run-remediation-tests.mjs --job rust-windows -- cargo test --manifest-path crates/app/Cargo.toml
      - name: Reconcile execution
        run: |
          node scripts/check-remediation-ledger.mjs --job rust-windows --results-dir results
          echo done
  rust-macos:
    steps:
      - run: swift test --package-path macos/swift-ui
`;
  const expected = parseCiJobs(workflow);
  assert.equal(expected['rust-windows'].length, 2);
  assert.ok(expected['rust-windows'][1].includes('echo done'));
  assert.deepEqual(parseCiJobs(workflow.replaceAll('\n', '\r\n')), expected);
});

test('a CRLF workflow still certifies the configured ledger execution steps', () => {
  const { root } = fixture();
  try {
    const path = join(root, '.github/workflows/ci.yml');
    writeFileSync(path, readFileSync(path, 'utf8').replaceAll('\n', '\r\n'));
    assert.deepEqual(validateLedger(root), []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('a well-formed ledger validates clean', () => {
  const { root } = fixture();
  assert.deepEqual(validateLedger(root), []);
  rmSync(root, { recursive: true, force: true });
});

test('fixed_gated without a gate is rejected', () => {
  const { root } = fixture({ entry: { gate: null } });
  assert.ok(validateLedger(root).some((e) => /requires a gate/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('a gate whose test does not exist is rejected', () => {
  const { root } = fixture({ entry: { gate: { file: 'crates/app/src/lib.rs', test: 'not_real', command: 'cargo test --locked --manifest-path crates/app/Cargo.toml', asserts: 'asserts something real' } } });
  assert.ok(validateLedger(root).some((e) => /not found in/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('a gate pointing at a missing file is rejected', () => {
  const { root } = fixture({ entry: { gate: { file: 'crates/app/src/missing.rs', test: 'real_gate', command: 'cargo test --locked --manifest-path crates/app/Cargo.toml', asserts: 'asserts something real' } } });
  assert.ok(validateLedger(root).some((e) => /does not exist/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('an unknown ci_job is rejected', () => {
  const { root } = fixture({ entry: { ci_job: ['rust-nonexistent'] } });
  assert.ok(validateLedger(root).some((e) => /is not a job in/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('a bad status is rejected', () => {
  const { root } = fixture({ entry: { status: 'done' } });
  assert.ok(validateLedger(root).some((e) => /is not one of/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('a required field missing is rejected', () => {
  const { root, data } = fixture();
  delete data.reopen_condition;
  writeFileSync(join(root, 'docs/remediation-ledger/entries/S1-P0-1.json'), JSON.stringify(data));
  assert.ok(validateLedger(root).some((e) => /missing required field "reopen_condition"/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('a manifest id without an entry file is rejected', () => {
  const { root } = fixture({ manifest: { expected_ids: ['S1-P0-1', 'S2-F9'] } });
  assert.ok(validateLedger(root).some((e) => /no entry file exists/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('an unknown extra field is rejected (schema additionalProperties)', () => {
  const { root } = fixture({ entry: { bogus_field: 'nope' } });
  assert.ok(validateLedger(root).some((e) => /unknown field "bogus_field"/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('a gate command that does not compile the gate file is rejected', () => {
  const { root } = fixture({ entry: { gate: { file: 'crates/app/src/lib.rs', test: 'real_gate', command: 'cargo test --locked --manifest-path other/Cargo.toml', asserts: 'asserts something real' } } });
  assert.ok(validateLedger(root).some((e) => /does not compile/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('a ci_job that runs no covering command is rejected', () => {
  const ci = 'jobs:\n  rust-macos:\n    steps:\n      - run: cargo test --locked --manifest-path crates/other/Cargo.toml\n';
  const { root } = fixture({ ci });
  assert.ok(validateLedger(root).some((e) => /runs no command covering gate.file/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('a gate command with no --manifest-path is rejected', () => {
  const { root } = fixture({ entry: { gate: { file: 'crates/app/src/lib.rs', test: 'real_gate', command: 'cargo test', asserts: 'asserts something real' } } });
  assert.ok(validateLedger(root).some((e) => /must name a --manifest-path or --package-path/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('manifestCovers follows include! from another crate', () => {
  const root = mkdtempSync(join(tmpdir(), 'ledger-inc-'));
  mkdirSync(join(root, 'crates/app/src'), { recursive: true });
  mkdirSync(join(root, 'crates/other'), { recursive: true });
  mkdirSync(join(root, 'shared'), { recursive: true });
  writeFileSync(join(root, 'shared/tests.rs'), '#[test]\nfn shared_gate() {}\n');
  writeFileSync(join(root, 'crates/app/src/tests.rs'), 'include!("../../../shared/tests.rs");\n');
  assert.equal(manifestCovers(root, 'crates/app/Cargo.toml', 'shared/tests.rs'), true);
  assert.equal(manifestCovers(root, 'crates/other/Cargo.toml', 'shared/tests.rs'), false);
  rmSync(root, { recursive: true, force: true });
});

test('declared status counts that drift from the entries are rejected', () => {
  const { root } = fixture({ manifest: { status_counts: { fixed_gated: 99 } } });
  assert.ok(validateLedger(root).some((e) => /status_counts\.fixed_gated is 99 but entries contain 1/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('a malformed entry does not cascade into a bogus missing-entry error', () => {
  const { root, data } = fixture();
  data.status = 'nonsense';
  writeFileSync(join(root, 'docs/remediation-ledger/entries/S1-P0-1.json'), JSON.stringify(data));
  const errors = validateLedger(root);
  assert.ok(errors.some((e) => /is not one of/.test(e)));
  assert.ok(!errors.some((e) => /no entry file exists/.test(e)), errors.join('; '));
  rmSync(root, { recursive: true, force: true });
});

test('an integration gate requires its marker to be present in the gate file', () => {
  const { root } = fixture();
  const data = JSON.parse(readFileSync(join(root, 'docs/remediation-ledger/entries/S1-P0-1.json'), 'utf8'));
  mkdirSync(join(root, 'scripts'), { recursive: true });
  writeFileSync(join(root, 'scripts/package.ps1'), "throw 'boom'\n");
  writeFileSync(join(root, '.github/workflows/ci.yml'), 'jobs:\n  rust-windows:\n    steps:\n      - run: node scripts/run-remediation-tests.mjs --job rust-windows -- pwsh -File ./scripts/package.ps1\n      - run: node scripts/check-remediation-ledger.mjs --job rust-windows --results-dir results\n');
  data.gate = { kind: 'integration', file: 'scripts/package.ps1', marker: 'Packaged app is listening', command: './scripts/package.ps1', asserts: 'asserts something real' };
  data.ci_job = ['rust-windows'];
  writeFileSync(join(root, 'docs/remediation-ledger/entries/S1-P0-1.json'), JSON.stringify(data));
  assert.ok(validateLedger(root).some((e) => /marker not found/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('an integration gate with a present marker and a covering job is accepted', () => {
  const { root } = fixture();
  const data = JSON.parse(readFileSync(join(root, 'docs/remediation-ledger/entries/S1-P0-1.json'), 'utf8'));
  mkdirSync(join(root, 'scripts'), { recursive: true });
  writeFileSync(join(root, 'scripts/package.ps1'), "throw 'Packaged app is listening on the legacy port'\n");
  writeFileSync(join(root, '.github/workflows/ci.yml'), 'jobs:\n  rust-windows:\n    steps:\n      - run: node scripts/run-remediation-tests.mjs --job rust-windows -- pwsh -File ./scripts/package.ps1\n      - run: node scripts/check-remediation-ledger.mjs --job rust-windows --results-dir results\n');
  data.gate = { kind: 'integration', file: 'scripts/package.ps1', marker: 'Packaged app is listening', command: './scripts/package.ps1', asserts: 'asserts something real' };
  data.ci_job = ['rust-windows'];
  writeFileSync(join(root, 'docs/remediation-ledger/entries/S1-P0-1.json'), JSON.stringify(data));
  assert.deepEqual(validateLedger(root), []);
  rmSync(root, { recursive: true, force: true });
});

test('a unit gate can be covered by a --package-path (swift) command', () => {
  const { root } = fixture();
  const data = JSON.parse(readFileSync(join(root, 'docs/remediation-ledger/entries/S1-P0-1.json'), 'utf8'));
  mkdirSync(join(root, 'pkg/Sources/App'), { recursive: true });
  mkdirSync(join(root, 'pkg/Tests/AppTests'), { recursive: true });
  writeFileSync(join(root, 'pkg/Tests/AppTests/GateTests.swift'), 'func testSwiftGate() {}\n');
  writeFileSync(join(root, '.github/workflows/ci.yml'), 'jobs:\n  rust-macos:\n    steps:\n      - run: node scripts/run-remediation-tests.mjs --job rust-macos -- swift test --package-path pkg\n      - run: node scripts/check-remediation-ledger.mjs --job rust-macos --results-dir results\n');
  data.gate = { kind: 'unit', file: 'pkg/Tests/AppTests/GateTests.swift', test: 'testSwiftGate', command: 'swift test --package-path pkg', asserts: 'asserts something real' };
  data.ci_job = ['rust-macos'];
  writeFileSync(join(root, 'docs/remediation-ledger/entries/S1-P0-1.json'), JSON.stringify(data));
  assert.deepEqual(validateLedger(root), []);
  rmSync(root, { recursive: true, force: true });
});

test('renderIndex lists every entry', () => {
  const md = renderIndex(
    { initial_audit_sha: 'a'.repeat(40), baseline_date: '2026-09-29' },
    [{ id: 'S1-P0-1', level: 'P0', status: 'fixed_gated', gate: { test: 'real_gate' }, ci_job: ['rust-macos'] }],
  );
  assert.match(md, /\| S1-P0-1 \| P0 \| fixed_gated \| `real_gate` \| rust-macos \|/);
});

for (const flag of ['--no-run', '-- --list', '-- --ignored']) {
  test(`a non-executing/ignored-only gate command is rejected: ${flag}`, () => {
    const { root } = fixture({ entry: { gate: { ...makeEntry().gate, command: `cargo test --manifest-path crates/app/Cargo.toml ${flag}` } } });
    assert.ok(validateLedger(root).some((e) => /must execute tests/.test(e)));
    rmSync(root, { recursive: true, force: true });
  });
}

test('a closed gate with no CI job is rejected', () => {
  const { root } = fixture({ entry: { ci_job: [] } });
  assert.ok(validateLedger(root).some((e) => /requires at least one CI job/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

test('compilation cannot replace execution, even with the right manifest', () => {
  const { root, data } = fixture();
  const path = join(root, '.github/workflows/ci.yml');
  writeFileSync(path, readFileSync(path, 'utf8').replace('cargo test', 'cargo check'));
  assert.ok(validateLedger(root).some((e) => /runs no command covering/.test(e)));
  data.gate.command = data.gate.command.replace('cargo test', 'cargo check');
  writeFileSync(join(root, 'docs/remediation-ledger/entries/S1-P0-1.json'), JSON.stringify(data));
  assert.ok(validateLedger(root).some((e) => /must execute tests/.test(e)));
  rmSync(root, { recursive: true, force: true });
});

for (const source of ['fn real_gate() {}', '#[test]\n#[ignore]\nfn real_gate() {}']) {
  test(`an ordinary/ignored function is not a gate: ${source.split('\n')[0]}`, () => {
    const { root } = fixture();
    writeFileSync(join(root, 'crates/app/src/lib.rs'), source);
    assert.ok(validateLedger(root).some((e) => /registered, non-ignored test/.test(e)));
    rmSync(root, { recursive: true, force: true });
  });
}
