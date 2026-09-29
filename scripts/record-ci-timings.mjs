#!/usr/bin/env node
// Records per-job/per-step durations and failure status for recent CI runs so
// the team can see test-suite cost trends over time. Read-only against the API.
//
// Usage: GITHUB_TOKEN=... node scripts/record-ci-timings.mjs [--repo owner/name] [--runs 5] [--out docs/ci-timings.jsonl]
// With no --out it prints JSONL to stdout.
import { writeFileSync, appendFileSync } from 'node:fs';

const args = process.argv.slice(2);
const flag = (name, dflt) => {
  const i = args.indexOf(name);
  return i !== -1 ? args[i + 1] : dflt;
};
const repo = flag('--repo', process.env.GITHUB_REPOSITORY || 'monet4070/TailSync');
const runs = Number(flag('--runs', '5'));
const out = flag('--out', null);
const branch = flag('--branch', null);
const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;

if (!token) {
  console.error('GITHUB_TOKEN (or GH_TOKEN) is required');
  process.exit(1);
}

const api = async (path) => {
  const res = await fetch(`https://api.github.com${path}`, {
    headers: { authorization: `Bearer ${token}`, accept: 'application/vnd.github+json', 'user-agent': 'tailsync-ci-timings' },
  });
  if (!res.ok) throw new Error(`${path} -> ${res.status} ${res.statusText}`);
  return res.json();
};

const ms = (a, b) => (a && b ? new Date(b) - new Date(a) : null);

const runList = await api(`/repos/${repo}/actions/runs?per_page=${runs}${branch ? `&branch=${branch}` : ''}`);
const lines = [];
for (const run of runList.workflow_runs) {
  const jobs = await api(`/repos/${repo}/actions/runs/${run.id}/jobs?per_page=100`);
  for (const job of jobs.jobs) {
    lines.push(JSON.stringify({
      recorded_at: new Date().toISOString(),
      run_id: run.id,
      run_conclusion: run.conclusion,
      head_sha: run.head_sha,
      branch: run.head_branch,
      workflow: run.name,
      job: job.name,
      job_conclusion: job.conclusion,
      job_duration_ms: ms(job.started_at, job.completed_at),
      steps: (job.steps || []).map((s) => ({
        name: s.name,
        conclusion: s.conclusion,
        duration_ms: ms(s.started_at, s.completed_at),
      })),
    }));
  }
}

const payload = lines.join('\n') + (lines.length ? '\n' : '');
if (out) {
  appendFileSync(out, payload);
  console.log(`appended ${lines.length} job records to ${out}`);
} else {
  process.stdout.write(payload);
}
