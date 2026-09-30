import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import { isExecutedTestCommand, parseTestOutput, sourceIdentity, testPassed } from './remediation-test-results.mjs';
import { validateExecutionResults } from './check-remediation-ledger.mjs';

const command = 'cargo test --locked --manifest-path crates/app/Cargo.toml';
const source = { sha: 'a'.repeat(40), fingerprint: 'b'.repeat(64) };
const entry = { id: 'S1-P0-1', ci_job: ['rust-macos'], gate: { kind: 'unit', file: 'crates/app/src/lib.rs', test: 'real_gate' } };
function evidence(overrides = {}) {
  const root = mkdtempSync(join(tmpdir(), 'ledger-result-'));
  const resultsDir = join(root, 'results');
  mkdirSync(join(resultsDir, 'rust-macos'), { recursive: true });
  const record = { schema_version: 1, job: 'rust-macos', command, source, exit_code: 0, source_unchanged: true,
    tests: [{ name: 'module::real_gate', status: 'passed' }], integrations: [], ...overrides };
  writeFileSync(join(resultsDir, 'rust-macos/suite.json'), JSON.stringify(record));
  return { root, verify: () => validateExecutionResults(root, [entry], { job: 'rust-macos', resultsDir, source }),
    cleanup: () => rmSync(root, { recursive: true, force: true }) };
}

test('parses actual Rust and XCTest results; a test listing cannot certify execution', () => {
  const rows = parseTestOutput("test mod::real_gate ... ok\r\ntest other ... ignored\ntest failed ... FAILED\nreal_gate: test\nTest Case '-[TailSyncTests.ApiTests testGate]' passed (0.01 seconds).\nTest Case '-[TailSyncTests.ApiTests testSkipped]' skipped (0.01 seconds).\n");
  assert.equal(rows.length, 5);
  assert.ok(testPassed(rows, 'real_gate'));
  assert.ok(testPassed(rows, 'testGate'));
  assert.ok(!testPassed(rows, 'testSkipped'));
  assert.ok(!testPassed(rows, 'failed'));
  assert.ok(!testPassed(rows, 'real'));
  assert.ok(!testPassed(parseTestOutput('real_gate: test'), 'real_gate'));
  const ignoredWithReason = parseTestOutput('test real_gate ... ignored, requires native desktop session');
  assert.deepEqual(ignoredWithReason, [{ name: 'real_gate', status: 'skipped' }]);
  assert.ok(!testPassed(ignoredWithReason, 'real_gate'));
});

test('does not combine a clippy manifest with an unrelated executing suite', () => {
  assert.ok(!isExecutedTestCommand('cargo check --manifest-path crates/app/Cargo.toml'));
  assert.ok(!isExecutedTestCommand(command + ' --no-run'));
  assert.ok(!isExecutedTestCommand(command + ' -- --list'));
  const f = evidence({ command: 'cargo clippy --manifest-path crates/app/Cargo.toml\ncargo test --manifest-path other/Cargo.toml' });
  assert.ok(f.verify().some((e) => /no actual passing/.test(e))); f.cleanup();
});

test('accepts a passing test from the executing suite and current source', () => {
  const f = evidence(); assert.deepEqual(f.verify(), []); f.cleanup();
});

for (const overrides of [
  { tests: [] }, { tests: [{ name: 'module::real_gate', status: 'skipped' }] },
  { tests: [{ name: 'module::real_gate', status: 'failed' }] }, { exit_code: 1 },
  { source_unchanged: false }, { source: { ...source, fingerprint: 'c'.repeat(64) } },
  { source: { ...source, sha: 'd'.repeat(40) } }, { command: command + ' --no-run' },
  { tests: [null] }, { tests: [{ name: 'module::real_gate', status: 'compiled' }] },
]) {
  test(`rejects missing, skipped, failed or stale proof: ${JSON.stringify(overrides)}`, () => {
    const f = evidence(overrides); assert.ok(f.verify().length); f.cleanup();
  });
}

test('successful packaging without the runtime assertion marker is not P0 proof', () => {
  const f = evidence({ command: 'pwsh -File scripts/package.ps1', tests: [] });
  const integration = { ...entry, gate: { kind: 'integration', file: 'scripts/package.ps1' } };
  const options = { job: 'rust-macos', resultsDir: join(f.root, 'results'), source };
  assert.ok(validateExecutionResults(f.root, [integration], options).length);
  writeFileSync(join(options.resultsDir, 'rust-macos/suite.json'), JSON.stringify({
    schema_version: 1, job: 'rust-macos', command: 'pwsh -File scripts/package.ps1', source,
    exit_code: 0, source_unchanged: true, tests: [], integrations: [entry.id],
  }));
  assert.deepEqual(validateExecutionResults(f.root, [integration], options), []);
  f.cleanup();
});

test('sourceIdentity ignores untracked artifacts but tracks tracked content', () => {
  // A CI job builds before it verifies. If the fingerprint included untracked
  // files, every record written before a build step looked stale and the gate
  // could never be satisfied. Tracked content changes must still invalidate it.
  const root = mkdtempSync(join(tmpdir(), 'identity-'));
  execFileSync('git', ['-C', root, 'init', '-q']);
  writeFileSync(join(root, 'tracked.txt'), 'a');
  execFileSync('git', ['-C', root, 'add', '.']);
  execFileSync('git', ['-C', root, '-c', 'user.email=t@t', '-c', 'user.name=t', 'commit', '-qm', 'init']);
  const before = sourceIdentity(root);

  writeFileSync(join(root, 'build-artifact.txt'), 'generated');
  assert.equal(
    sourceIdentity(root).fingerprint,
    before.fingerprint,
    'an untracked build artifact must not invalidate a recorded result',
  );

  writeFileSync(join(root, 'tracked.txt'), 'b');
  assert.notEqual(
    sourceIdentity(root).fingerprint,
    before.fingerprint,
    'a tracked source change must invalidate a recorded result',
  );

  // Build-generated, git-tracked files are regenerated by a CI job; rewriting them
  // must not invalidate records taken before the build (that broke Windows CI).
  mkdirSync(join(root, 'windows/src-tauri/gen/schemas'), { recursive: true });
  writeFileSync(join(root, 'windows/src-tauri/gen/schemas/windows-schema.json'), '{"a":1}');
  execFileSync('git', ['-C', root, 'add', '.']);
  execFileSync('git', ['-C', root, '-c', 'user.email=t@t', '-c', 'user.name=t', 'commit', '-qm', 'schema']);
  const withSchema = sourceIdentity(root);
  writeFileSync(join(root, 'windows/src-tauri/gen/schemas/windows-schema.json'), '{"a":2}');
  assert.equal(
    sourceIdentity(root).fingerprint,
    withSchema.fingerprint,
    'a regenerated tracked schema must not invalidate a recorded result',
  );
  rmSync(root, { recursive: true, force: true });
});
