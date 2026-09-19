import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { hostMetadata, sha256File } from "./capture-optimization-baseline.mjs";

test("hashes baseline inputs without exposing their contents", () => {
  const directory = mkdtempSync(join(tmpdir(), "tailsync-baseline-"));
  const path = join(directory, "Cargo.lock");
  writeFileSync(path, "lock input");
  assert.equal(
    sha256File(path),
    "f02fd2a3bd91fd94aa57cead50fa8f6d84f5b0801cda0c1ca5c4c2c7a5ef40db",
  );
});

test("records bounded hardware metadata for comparable runs", () => {
  const host = hostMetadata();
  assert.match(host.platform, /^(darwin|linux|win32|aix|freebsd|openbsd|sunos)$/);
  assert.ok(host.arch.length > 0);
  assert.ok(host.logical_cpus > 0);
  assert.ok(host.total_memory_bytes > 0);
  assert.ok(host.cpu_model === null || host.cpu_model.length > 0);
});
