#!/usr/bin/env node

import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";

export const PRODUCTION_ROOTS = new Set(["tailsync-core", "tailsync-runtime"]);

function sourceKey(pkg) {
  if (!pkg.source) return null;
  return `${pkg.version}|${pkg.source}`;
}

function isProductionDependency(dep) {
  return (dep.dep_kinds ?? []).some(
    ({ kind }) => kind === null || kind === "normal" || kind === "build",
  );
}

/** Return registry/git dependencies reachable from the shared production roots. */
export function collectProductionResolution(metadata) {
  const packageById = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]));
  const nodeById = new Map((metadata.resolve?.nodes ?? []).map((node) => [node.id, node]));
  const roots = [...nodeById.keys()].filter((id) =>
    PRODUCTION_ROOTS.has(packageById.get(id)?.name),
  );
  if (roots.length === 0) {
    throw new Error(
      `metadata has no shared production roots (${[...PRODUCTION_ROOTS].join(", ")})`,
    );
  }

  const visited = new Set();
  const pending = [...roots];
  const resolution = new Map();
  while (pending.length > 0) {
    const id = pending.pop();
    if (visited.has(id)) continue;
    visited.add(id);
    const node = nodeById.get(id);
    if (!node) continue;

    for (const dep of node.deps ?? []) {
      if (!isProductionDependency(dep) || !packageById.has(dep.pkg)) continue;
      pending.push(dep.pkg);
      const pkg = packageById.get(dep.pkg);
      const key = sourceKey(pkg);
      if (!key) continue;
      if (!resolution.has(pkg.name)) resolution.set(pkg.name, new Set());
      resolution.get(pkg.name).add(key);
    }
  }
  return resolution;
}

function sortedValues(values) {
  return [...values].sort();
}

export function compareResolutions(left, right, allowedPackages = []) {
  const allowed = new Set(allowedPackages);
  const names = [...left.keys()].filter((name) => right.has(name)).sort();
  return names
    .filter((name) => !allowed.has(name))
    .filter(
      (name) =>
        JSON.stringify(sortedValues(left.get(name))) !==
        JSON.stringify(sortedValues(right.get(name))),
    )
    .map((name) => ({
      name,
      left: sortedValues(left.get(name)),
      right: sortedValues(right.get(name)),
    }));
}

function option(args, name, fallback) {
  const index = args.indexOf(name);
  return index === -1 ? fallback : args[index + 1];
}

function cargoMetadata(manifestPath, target) {
  const result = spawnSync(
    "cargo",
    [
      "metadata",
      "--locked",
      "--format-version",
      "1",
      "--manifest-path",
      manifestPath,
      "--filter-platform",
      target,
    ],
    { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 },
  );
  if (result.status !== 0) {
    if (result.error) throw result.error;
    throw new Error(
      `cargo metadata failed for ${manifestPath} (${target}):\n${result.stderr}`,
    );
  }
  return JSON.parse(result.stdout);
}

function loadPolicy(policyPath) {
  if (!existsSync(policyPath)) return { allowedMismatches: [] };
  const policy = JSON.parse(readFileSync(policyPath, "utf8"));
  if (!Array.isArray(policy.allowedMismatches)) {
    throw new Error(`${policyPath} must define an allowedMismatches array`);
  }
  return policy;
}

export function run(argv = process.argv.slice(2)) {
  const repoRoot = resolve(option(argv, "--root", process.cwd()));
  const policyPath = resolve(
    repoRoot,
    option(argv, "--policy", "scripts/shared-resolution-policy.json"),
  );
  const macTarget = option(
    argv,
    "--mac-target",
    process.env.TAILSYNC_MAC_TARGET ?? "aarch64-apple-darwin",
  );
  const windowsTarget = option(
    argv,
    "--windows-target",
    process.env.TAILSYNC_WINDOWS_TARGET ?? "x86_64-pc-windows-msvc",
  );
  const contexts = [
    { label: "root-macos", manifest: join(repoRoot, "Cargo.toml"), target: macTarget },
    {
      label: "macos-product",
      manifest: join(repoRoot, "macos/src-tauri/Cargo.toml"),
      target: macTarget,
    },
    { label: "root-windows", manifest: join(repoRoot, "Cargo.toml"), target: windowsTarget },
    {
      label: "windows-product",
      manifest: join(repoRoot, "windows/src-tauri/Cargo.toml"),
      target: windowsTarget,
    },
  ];
  const policy = loadPolicy(policyPath);
  const resolved = contexts.map((context) => ({
    ...context,
    resolution: collectProductionResolution(cargoMetadata(context.manifest, context.target)),
  }));
  const pairs = [[resolved[0], resolved[1]], [resolved[2], resolved[3]]].map(
    ([left, right]) => ({
      left: left.label,
      right: right.label,
      mismatches: compareResolutions(
        left.resolution,
        right.resolution,
        policy.allowedMismatches,
      ),
    }),
  );

  console.log("Shared production dependency resolution");
  for (const pair of pairs) {
    console.log(
      `- ${pair.left} vs ${pair.right}: ${pair.mismatches.length === 0 ? "OK" : "MISMATCH"}`,
    );
    for (const mismatch of pair.mismatches) {
      console.log(
        `  ${mismatch.name}: ${mismatch.left.join(", ")} <> ${mismatch.right.join(", ")}`,
      );
    }
  }
  if (pairs.some(({ mismatches }) => mismatches.length > 0)) {
    throw new Error("shared production dependency resolution is inconsistent");
  }
  return 0;
}

const entrypoint =
  process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url;
if (entrypoint) {
  try {
    process.exitCode = run();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
