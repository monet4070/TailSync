import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFileSync, lstatSync, readlinkSync } from 'node:fs';
import { join } from 'node:path';

// Compilation, listing and filtered/ignored-only runs cannot certify a suite.
export function isExecutedTestCommand(command) {
  return /\b(?:cargo(?:\s+\+\S+)?|swift)\s+test\b/.test(command)
    && !/(?:^|\s)--(?:no-run|list(?:-tests)?|skip|ignored|filter)(?:=|\s|$)/.test(command);
}

export function parseTestOutput(output) {
  const results = [];
  for (const line of output.replace(/\x1b\[[0-9;]*m/g, '').split(/\r?\n/)) {
    const rust = /^test (\S+) \.\.\. (ok|FAILED|ignored)(?:[,\s].*)?$/.exec(line);
    if (rust) results.push({ name: rust[1], status: { ok: 'passed', FAILED: 'failed', ignored: 'skipped' }[rust[2]] });
    const swift = /^Test Case '(.+)' (passed|failed|skipped)(?:\s.*)?$/.exec(line);
    if (swift) {
      const name = swift[1].replace(/^-\[(.+) (\S+)\]$/, '$1::$2');
      results.push({ name, status: swift[2] });
    }
  }
  return results;
}

export function testPassed(results, test) {
  const matches = results.filter((r) => r.name === test || r.name.endsWith(`::${test}`) || r.name.endsWith(`.${test}`));
  return matches.length > 0 && matches.every((r) => r.status === 'passed');
}

/// Git-tracked files that a build regenerates (Tauri codegen for the capability and
/// schema manifests). Regenerating them during a CI job rewrites their bytes, which
/// would invalidate every record written before the build step — that is exactly what
/// broke the Windows verification. They are derived from tracked `tauri.conf.json`
/// sources and dedicated CI steps already assert they are unchanged after a build, so
/// they are excluded here while every hand-written source stays covered.
const GENERATED_SOURCES = ['macos/src-tauri/gen/schemas/', 'windows/src-tauri/gen/schemas/'];

export function sourceIdentity(root) {
  const sha = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  // Tracked files only. Build tooling (Vite, Tauri codegen, bundlers) creates and
  // removes untracked files during a job; including them made every record written
  // before such a step look stale, so the gate could never be satisfied in a job
  // that builds before it verifies. Reading the working-tree content still detects
  // a tracked source file being modified during a run.
  const files = execFileSync('git', ['-C', root, 'ls-files', '--cached', '-z'], { encoding: 'utf8' });
  const hash = createHash('sha256');
  for (const file of [...new Set(files.split('\0').filter(Boolean))].sort()) {
    if (GENERATED_SOURCES.some((prefix) => file.startsWith(prefix))) continue;
    const path = join(root, file);
    hash.update(file + '\0');
    try { hash.update(lstatSync(path).isSymbolicLink() ? readlinkSync(path) : readFileSync(path)); }
    catch (error) { if (error.code !== 'ENOENT') throw error; hash.update('DELETED'); }
    hash.update('\0');
  }
  return { sha, fingerprint: hash.digest('hex') };
}
