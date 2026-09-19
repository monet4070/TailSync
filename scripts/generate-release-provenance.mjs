#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync, lstatSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { pathToFileURL } from "node:url";

function fail(message) {
  throw new Error(message);
}

function option(args, name) {
  const index = args.indexOf(name);
  return index === -1 ? undefined : args[index + 1];
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function readBuildManifests(inputDirectory) {
  return readdirSync(inputDirectory)
    .filter((name) => /-build\.json$/.test(name))
    .sort()
    .map((name) => {
      const path = join(inputDirectory, name);
      try {
        return JSON.parse(readFileSync(path, "utf8"));
      } catch (error) {
        fail(`Invalid build manifest ${name}: ${error.message}`);
      }
    });
}

export function generateProvenance({
  inputDirectory,
  outputPath,
  commit,
  tag,
  workflow,
  runId,
  generatedAt,
}) {
  if (!existsSync(inputDirectory) || !statSync(inputDirectory).isDirectory()) {
    fail(`Release input directory does not exist: ${inputDirectory}`);
  }
  const buildManifests = readBuildManifests(inputDirectory);
  if (buildManifests.length === 0) fail("No release build manifests found");
  if (!/^[0-9a-f]{40}$/.test(commit)) fail(`Release commit must be a full SHA-1: ${commit}`);
  if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag)) {
    fail(`Release tag is invalid: ${tag}`);
  }
  const declared = new Map();
  for (const manifest of buildManifests) {
    const manifestCommit = manifest.sourceCommit ?? manifest.source_commit ?? manifest.commit;
    if (manifestCommit === undefined || manifestCommit === null) {
      fail("Build manifest is missing sourceCommit");
    }
    if (!/^[0-9a-f]{40}$/.test(manifestCommit)) {
      fail(`Build manifest sourceCommit must be a full SHA-1: ${manifestCommit}`);
    }
    if (manifestCommit !== commit) {
      fail(`Build manifest sourceCommit ${manifestCommit} differs from release commit ${commit}`);
    }
    if (manifest.sourceDirty !== false) fail("Build manifest must describe a clean source tree");
    if (manifest.product !== "TailSync" || manifest.version !== tag.slice(1)) {
      fail("Build manifest product/version differs from the release tag");
    }
    if (!Array.isArray(manifest.artifacts) || manifest.artifacts.length === 0) {
      fail("Build manifest has no artifact hashes");
    }
    for (const artifact of manifest.artifacts) {
      const name = artifact.file;
      if (typeof name !== "string" || !name || name !== basename(name) || /[\\/\x00]/.test(name)) {
        fail("Invalid artifact filename in build manifest");
      }
      if (declared.has(name)) fail(`Duplicate build artifact: ${name}`);
      const path = join(inputDirectory, name);
      if (!existsSync(path) || !lstatSync(path).isFile()) fail(`Missing or unsafe build artifact: ${name}`);
      if (!Number.isSafeInteger(artifact.bytes) || artifact.bytes < 0 || statSync(path).size !== artifact.bytes
        || !/^[0-9a-f]{64}$/.test(artifact.sha256) || sha256(path) !== artifact.sha256) {
        fail(`Build artifact hash or length mismatch: ${name}`);
      }
      declared.set(name, artifact);
    }
  }

  const artifacts = readdirSync(inputDirectory)
    .filter((name) => name !== basename(outputPath) && !/-build\.json$/.test(name))
    .sort()
    .map((name) => {
      const path = join(inputDirectory, name);
      if (!lstatSync(path).isFile()) fail(`Unexpected non-file release input: ${name}`);
      if (!declared.has(name) && !/^release-(windows|darwin)-(x86_64|aarch64)\.json$/.test(name)
          && !name.endsWith(".sha256") && name !== "latest.json") {
        fail(`Release artifact is not covered by a build manifest: ${name}`);
      }
      const bytes = statSync(path).size;
      return { file: name, bytes, sha256: sha256(path) };
    })
    .filter(Boolean);

  return {
    schema: 1,
    product: "TailSync",
    source: {
      commit,
      tag,
      workflow: workflow || null,
      run_id: runId || null,
    },
    generated_at: new Date(generatedAt || Date.now()).toISOString(),
    build_manifests: buildManifests,
    artifacts,
  };
}

export function main(argv = process.argv) {
  const inputDirectory = option(argv, "--input");
  const outputPath = option(argv, "--output");
  if (!inputDirectory || !outputPath) {
    fail("Usage: generate-release-provenance.mjs --input DIR --output FILE");
  }
  const provenance = generateProvenance({
    inputDirectory,
    outputPath,
    commit: process.env.GITHUB_SHA,
    tag: process.env.GITHUB_REF_NAME,
    workflow: process.env.GITHUB_WORKFLOW,
    runId: process.env.GITHUB_RUN_ID,
  });
  writeFileSync(outputPath, `${JSON.stringify(provenance, null, 2)}\n`, "utf8");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
