import test from "node:test";
import { createHash } from "node:crypto";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { generateProvenance } from "./generate-release-provenance.mjs";

test("records full source identity and hashes release artifacts", () => {
  const directory = mkdtempSync(join(tmpdir(), "tailsync-provenance-"));
  writeFileSync(
    join(directory, "TailSync-2.2.2-Windows-build.json"),
    JSON.stringify({
      product: "TailSync",
      version: "2.2.2",
      sourceDirty: false,
      artifacts: [{file: "TailSync-2.2.2-Windows.exe", bytes: 17, sha256: createHash("sha256").update("portable artifact").digest("hex")}],
      sourceCommit: "0123456789abcdef0123456789abcdef01234567",
    }),
  );
  writeFileSync(join(directory, "TailSync-2.2.2-Windows.exe"), "portable artifact");
  const result = generateProvenance({
    inputDirectory: directory,
    outputPath: join(directory, "release-provenance.json"),
    commit: "0123456789abcdef0123456789abcdef01234567",
    tag: "v2.2.2",
    workflow: "Release",
    runId: "42",
    generatedAt: "2026-09-06T00:00:00Z",
  });

  assert.equal(result.source.commit, "0123456789abcdef0123456789abcdef01234567");
  assert.equal(result.source.tag, "v2.2.2");
  assert.deepEqual(result.artifacts.map(({ file }) => file), ["TailSync-2.2.2-Windows.exe"]);
  assert.equal(result.artifacts[0].bytes, "portable artifact".length);
  assert.match(result.artifacts[0].sha256, /^[0-9a-f]{64}$/);
});

test("rejects a build manifest without source identity", () => {
  const directory = mkdtempSync(join(tmpdir(), "tailsync-provenance-missing-"));
  writeFileSync(
    join(directory, "TailSync-2.2.2-Windows-build.json"),
    JSON.stringify({ product: "TailSync", version: "2.2.2" }),
  );
  assert.throws(
    () => generateProvenance({
      inputDirectory: directory,
      outputPath: join(directory, "release-provenance.json"),
      commit: "0123456789abcdef0123456789abcdef01234567",
      tag: "v2.2.2",
    }),
    /missing sourceCommit/,
  );
});

test("rejects a build manifest from a different commit", () => {
  const directory = mkdtempSync(join(tmpdir(), "tailsync-provenance-mismatch-"));
  writeFileSync(
    join(directory, "TailSync-2.2.2-Windows-build.json"),
    JSON.stringify({
      product: "TailSync",
      version: "2.2.2",
      sourceDirty: false,
      artifacts: [{file: "TailSync-2.2.2-Windows.exe", bytes: 17, sha256: createHash("sha256").update("portable artifact").digest("hex")}],
      sourceCommit: "fedcba9876543210fedcba9876543210fedcba98",
    }),
  );
  assert.throws(
    () => generateProvenance({
      inputDirectory: directory,
      outputPath: join(directory, "release-provenance.json"),
      commit: "0123456789abcdef0123456789abcdef01234567",
      tag: "v2.2.2",
    }),
    /differs from release commit/,
  );
});
