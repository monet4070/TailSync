#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";
import { arch, cpus, platform, release, totalmem } from "node:os";

function run(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed: ${result.stderr || result.stdout}`);
  }
  return result.stdout.trim();
}

function runOptional(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : null;
}

export function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

export function hostMetadata() {
  const processors = cpus();
  return {
    platform: platform(),
    release: release(),
    arch: arch(),
    logical_cpus: processors.length,
    cpu_model: processors[0]?.model ?? null,
    total_memory_bytes: totalmem(),
  };
}

function fileInventory(root, relativePaths) {
  return Object.fromEntries(
    relativePaths
      .filter((relativePath) => existsSync(join(root, relativePath)))
      .map((relativePath) => {
        const path = join(root, relativePath);
        return [relativePath, { bytes: statSync(path).size, sha256: sha256File(path) }];
      }),
  );
}

/**
 * Capture reproducibility metadata without reading source contents or
 * environment secrets. A dirty working tree remains explicit in the result;
 * callers can archive the JSON beside their test output.
 */
export function captureBaseline(root = process.cwd(), capturedAt = new Date()) {
  const repoRoot = resolve(root);
  const status = run("git", ["status", "--short", "--untracked-files=all"], repoRoot);
  const trackedDiff = run("git", ["diff", "--binary", "HEAD"], repoRoot);
  return {
    schema: 1,
    captured_at: capturedAt.toISOString(),
    root: repoRoot,
    git: {
      head: run("git", ["rev-parse", "HEAD"], repoRoot),
      branch: run("git", ["branch", "--show-current"], repoRoot),
      dirty: status.length > 0,
      status: status ? status.split("\n") : [],
      tracked_diff: {
        bytes: Buffer.byteLength(trackedDiff),
        sha256: createHash("sha256").update(trackedDiff).digest("hex"),
      },
    },
    host: hostMetadata(),
    toolchain: {
      rustc: run("rustc", ["--version"], repoRoot),
      cargo: run("cargo", ["--version"], repoRoot),
      node: run("node", ["--version"], repoRoot),
      swift: runOptional("swift", ["--version"], repoRoot)?.split("\n")[0] ?? null,
    },
    lockfiles: fileInventory(repoRoot, [
      "Cargo.lock",
      "macos/src-tauri/Cargo.lock",
      "windows/src-tauri/Cargo.lock",
      "windows/package-lock.json",
      "site/package-lock.json",
    ]),
    validation: {
      sensitive_content: "not captured",
      data_directory: "caller supplied isolated directory",
      network: "caller supplied isolated route or test adapter",
    },
  };
}

function option(args, name) {
  const index = args.indexOf(name);
  return index === -1 ? undefined : args[index + 1];
}

export function main(argv = process.argv.slice(2)) {
  const root = resolve(option(argv, "--root") ?? process.cwd());
  const output = option(argv, "--output");
  const baseline = captureBaseline(root);
  const encoded = `${JSON.stringify(baseline, null, 2)}\n`;
  if (output) {
    const outputPath = resolve(root, output);
    mkdirSync(dirname(outputPath), { recursive: true });
    writeFileSync(outputPath, encoded, "utf8");
  } else {
    process.stdout.write(encoded);
  }
  return baseline;
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
