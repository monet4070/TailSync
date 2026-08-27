#!/usr/bin/env node
// Validates the RustSec advisory exception registry
// (security/rustsec-exceptions.json) against deny.toml.
//
// cargo-deny's [advisories].ignore natively supports only an advisory id
// and a free-form reason, so the richer fields — upstream link, owner,
// expiry, tracking issue — live in this JSON registry. The registry is the
// source of truth: every entry must exist in deny.toml's ignore list, and
// vice versa (1:1). An exception whose expiry date has arrived fails the
// check, so exceptions can never silently outlive their justification.
// See docs/SECURITY-AUDIT-2026-08.md.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

function fail(message) {
  throw new Error(message);
}

const ADVISORY_ID = /^RUSTSEC-\d{4}-\d{4}$/;
const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;

export function extractDenyIgnoreIds(denyToml) {
  // Line-based section walk so comment mentions of "[advisories].ignore"
  // cannot be mistaken for the section header.
  const lines = denyToml.split(/\r?\n/);
  let inAdvisories = false;
  let section = '';
  for (const line of lines) {
    const header = /^ *\[([^\]]+)\]/.exec(line);
    if (header) {
      if (inAdvisories) break;
      inAdvisories = header[1] === 'advisories';
      continue;
    }
    if (inAdvisories) section += `${line}\n`;
  }
  if (!inAdvisories) fail('deny.toml has no [advisories] section');
  const ignore = /ignore\s*=\s*\[([^\]]*)\]/.exec(section);
  if (!ignore) fail('deny.toml [advisories] has no ignore list');
  return [...ignore[1].matchAll(/RUSTSEC-\d{4}-\d{4}/g)].map((match) => match[0]);
}

function validateEntry(entry) {
  const { id, reason, upstream, owner, expires, issue } = entry;
  if (typeof id !== 'string' || !ADVISORY_ID.test(id)) {
    fail(`Exception id must look like RUSTSEC-2024-0001, got: ${JSON.stringify(id)}`);
  }
  if (typeof reason !== 'string' || reason.trim().length < 10) {
    fail(`Exception ${id} needs a substantive reason`);
  }
  if (typeof upstream !== 'string' || !/^https:\/\//.test(upstream)) {
    fail(`Exception ${id} needs an https:// upstream link`);
  }
  if (typeof owner !== 'string' || owner.trim() === '') {
    fail(`Exception ${id} needs an owner`);
  }
  if (typeof issue !== 'string' || !/^https:\/\/github\.com\//.test(issue)) {
    fail(`Exception ${id} needs a GitHub tracking-issue link`);
  }
  if (typeof expires !== 'string' || !ISO_DATE.test(expires)) {
    fail(`Exception ${id} expires field must be an ISO date (YYYY-MM-DD)`);
  }
  const today = new Date().toISOString().slice(0, 10);
  if (expires <= today) {
    fail(
      `Exception ${id} expired on ${expires}; remove it from deny.toml and this registry, ` +
        'or justify a new expiry on the tracking issue',
    );
  }
}

export function checkRustsecExceptions(root) {
  const registryPath = resolve(root, 'security/rustsec-exceptions.json');
  const registry = JSON.parse(readFileSync(registryPath, 'utf8'));
  if (!Array.isArray(registry.exceptions)) {
    fail('security/rustsec-exceptions.json must contain an "exceptions" array');
  }
  const ids = registry.exceptions.map((entry) => {
    validateEntry(entry);
    return entry.id;
  });
  const duplicates = ids.filter((id, index) => ids.indexOf(id) !== index);
  if (duplicates.length > 0) fail(`Duplicate exception ids: ${duplicates.join(', ')}`);

  const denyPath = resolve(root, 'deny.toml');
  const denyIds = extractDenyIgnoreIds(readFileSync(denyPath, 'utf8'));

  const onlyInDeny = denyIds.filter((id) => !ids.includes(id));
  const onlyInRegistry = ids.filter((id) => !denyIds.includes(id));
  if (onlyInDeny.length > 0) {
    fail(
      `deny.toml ignores ${onlyInDeny.join(', ')} without a registry entry; ` +
        'add them to security/rustsec-exceptions.json',
    );
  }
  if (onlyInRegistry.length > 0) {
    fail(
      `Registry entries ${onlyInRegistry.join(', ')} are not present in deny.toml ` +
        '[advisories].ignore; keep both in sync',
    );
  }
  return ids.length;
}

function main() {
  const root = process.argv[2] ?? '.';
  const count = checkRustsecExceptions(root);
  process.stdout.write(
    count === 0
      ? 'OK: no RustSec exceptions registered.\n'
      : `OK: ${count} RustSec exception(s) registered, all in sync and unexpired.\n`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
