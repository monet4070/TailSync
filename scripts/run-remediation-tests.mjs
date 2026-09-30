#!/usr/bin/env node
// Runs an existing CI command once, retains its output, and fails if a declared
// gate was missing/skipped. Results are bound to the source content and commit.
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { loadLedger, commandCovers } from './check-remediation-ledger.mjs';
import { isExecutedTestCommand, parseTestOutput, sourceIdentity, testPassed } from './remediation-test-results.mjs';

const args = process.argv.slice(2);
const separator = args.indexOf('--');
const flag = (name, fallback) => { const i = args.indexOf(name); return i < 0 ? fallback : args[i + 1]; };
const root = resolve(flag('--root', '.'));
const job = flag('--job', process.env.GITHUB_JOB);
const outputDir = flag('--results-dir', join(root, '.remediation-test-results'));
const command = args.slice(separator + 1);
if (separator < 0 || !job || !/^[\w-]+$/.test(job) || command.length === 0) {
  throw new Error('Usage: run-remediation-tests.mjs --job <job> [--results-dir <dir>] -- <existing test/build command>');
}
const text = command.join(' ');
const integrationFiles = command.map((p) => resolve(root, p));
const gates = loadLedger(root).entries.map((e) => e.data).filter((e) => e.gate && e.ci_job.includes(job)
  && (e.gate.kind === 'unit' ? commandCovers(root, text, e.gate.file) : integrationFiles.includes(resolve(root, e.gate.file))));
if (!gates.length) throw new Error(`No ledger gates match ${job}: ${text}`);
if (gates.some((e) => e.gate.kind === 'unit') && !isExecutedTestCommand(text)) throw new Error('A gate requires an executing test command');
if (/-SkipSmokeTest\b/i.test(text)) throw new Error('Packaged smoke tests cannot be skipped');
const before = sourceIdentity(root);
const started = Date.now();
const child = spawn(command[0], command.slice(1), { cwd: root, env: { ...process.env, CARGO_TERM_COLOR: 'never' }, stdio: ['inherit', 'pipe', 'pipe'] });
let output = '';
for (const [stream, destination] of [[child.stdout, process.stdout], [child.stderr, process.stderr]]) {
  stream.setEncoding('utf8');
  stream.on('data', (data) => { output += data; destination.write(data); });
}
const exitCode = await new Promise((accept, reject) => { child.on('error', reject); child.on('close', (code) => accept(code ?? 1)); });
const after = sourceIdentity(root);
const tests = parseTestOutput(output);
const integrations = output.split(/\r?\n/).filter((line) => /^REMEDIATION_GATE_PASS S[\w-]+$/.test(line)).map((line) => line.split(' ')[1]);
const missing = gates.filter((e) => e.gate.kind === 'unit' ? !testPassed(tests, e.gate.test) : !integrations.includes(e.id));
const record = { schema_version: 1, job, command: text, source: before, exit_code: exitCode,
  source_unchanged: before.fingerprint === after.fingerprint && before.sha === after.sha,
  duration_ms: Date.now() - started, tests, integrations };
const dir = join(outputDir, job);
mkdirSync(dir, { recursive: true });
const suite = createHash('sha256').update(text).digest('hex').slice(0, 16);
writeFileSync(join(dir, `${suite}.json`), JSON.stringify(record, null, 2) + '\n');
writeFileSync(join(dir, `${suite}.log`), output);
if (exitCode || !record.source_unchanged || missing.length) {
  console.error(`Remediation execution failed: exit=${exitCode}, source_unchanged=${record.source_unchanged}, missing/skipped gates=${missing.map((e) => e.id).join(', ')}`);
  process.exit(exitCode || 1);
}
console.log(`Remediation execution passed: ${gates.map((e) => e.id).join(', ')}`);
