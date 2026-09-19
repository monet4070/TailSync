#!/usr/bin/env node

import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";

export const PRODUCTION_ROOTS = new Set(["tailsync-core", "tailsync-runtime"]);

function sourceKey(pkg, node) {
  if (!pkg.source) return null;
  const features = [...(node?.features ?? [])].sort();
  return `${pkg.version}|${pkg.source}|features=${features.join(",") || "-"}`;
}

function isProductionDependency(dep) {
  return (dep.dep_kinds ?? []).some(
    ({ kind }) => kind === null || kind === "normal" || kind === "build",
  );
}

/** Return registry/git dependencies reachable from the shared production roots. */
export function collectProductionResolution(metadata) {
  const packageById = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]));
  const nodeById = new Map(
    (metadata.resolve?.nodes ?? []).map((node) => [node.id, node]),
  );
  const roots = [...nodeById.keys()].filter((id) =>
    PRODUCTION_ROOTS.has(packageById.get(id)?.name),
  );
  if (roots.length === 0) {
    throw new Error(
      `metadata has no shared production roots (${[...PRODUCTION_ROOTS].join(", ")})`,
    );
  }

  const visited = new Set();
  const pending = roots.map((id) => ({
    id,
    path: [packageById.get(id)?.name ?? id],
  }));
  const resolution = new Map();
  resolution.paths = new Map();
  while (pending.length > 0) {
    const { id, path } = pending.shift();
    if (visited.has(id)) continue;
    visited.add(id);
    const node = nodeById.get(id);
    if (!node) continue;

    for (const dep of node.deps ?? []) {
      if (!isProductionDependency(dep) || !packageById.has(dep.pkg)) continue;
      const pkg = packageById.get(dep.pkg);
      const dependencyPath = [...path, pkg.name];
      pending.push({ id: dep.pkg, path: dependencyPath });
      const key = sourceKey(pkg, nodeById.get(dep.pkg));
      if (!key) continue;
      if (!resolution.has(pkg.name)) resolution.set(pkg.name, new Set());
      resolution.get(pkg.name).add(key);
      const pathKey = `${pkg.name}\0${key}`;
      if (!resolution.paths.has(pathKey)) {
        resolution.paths.set(pathKey, dependencyPath.join(" -> "));
      }
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
      leftPaths: sortedValues(left.get(name)).map(
        (value) => left.paths?.get(`${name}\0${value}`) ?? null,
      ),
      rightPaths: sortedValues(right.get(name)).map(
        (value) => right.paths?.get(`${name}\0${value}`) ?? null,
      ),
    }));
}

function identityKey(value) {
  return value.replace(/\|features=[^|]*$/, "");
}

function projectResolution(resolution, projector) {
  const projected = new Map();
  projected.paths = new Map();
  for (const [name, values] of resolution) {
    for (const value of values) {
      const nextValue = projector(value);
      if (!projected.has(name)) projected.set(name, new Set());
      projected.get(name).add(nextValue);
      const nextPathKey = `${name}\0${nextValue}`;
      if (!projected.paths.has(nextPathKey)) {
        projected.paths.set(
          nextPathKey,
          resolution.paths?.get(`${name}\0${value}`) ?? null,
        );
      }
    }
  }
  return projected;
}

/** Compare package version/source identity while ignoring Cargo's global feature union. */
export function comparePackageIdentities(left, right, allowedPackages = []) {
  return compareResolutions(
    projectResolution(left, identityKey),
    projectResolution(right, identityKey),
    allowedPackages,
  );
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

export function validatePolicy(
  policy,
  policyPath = "shared resolution policy",
) {
  if (!Array.isArray(policy.allowedMismatches)) {
    throw new Error(`${policyPath} must define an allowedMismatches array`);
  }
  if (
    !Array.isArray(policy.requiredFeatureMatches) ||
    !policy.requiredFeatureMatches.every(
      (value) => typeof value === "string" && value.trim(),
    )
  ) {
    throw new Error(
      `${policyPath} must define a requiredFeatureMatches string array`,
    );
  }
  for (const [index, entry] of policy.allowedMismatches.entries()) {
    if (
      !entry ||
      typeof entry !== "object" ||
      typeof entry.package !== "string" ||
      !Array.isArray(entry.contexts) ||
      entry.contexts.length !== 2 ||
      !entry.contexts.every((value) => typeof value === "string" && value) ||
      !Array.isArray(entry.targets) ||
      entry.targets.length === 0 ||
      !entry.targets.every((value) => typeof value === "string" && value) ||
      typeof entry.reason !== "string" ||
      !entry.reason.trim() ||
      typeof entry.owner !== "string" ||
      !entry.owner.trim() ||
      typeof entry.reviewWhen !== "string" ||
      !entry.reviewWhen.trim()
    ) {
      throw new Error(
        `${policyPath} allowedMismatches[${index}] must define package, two contexts, targets, reason, owner, and reviewWhen`,
      );
    }
  }
  return policy;
}

function loadPolicy(policyPath) {
  if (!existsSync(policyPath)) {
    return { allowedMismatches: [], requiredFeatureMatches: [] };
  }
  const policy = JSON.parse(readFileSync(policyPath, "utf8"));
  return validatePolicy(policy, policyPath);
}

function allowedPackagesForPair(policy, left, right) {
  return policy.allowedMismatches
    .filter(
      (entry) =>
        entry.contexts.includes(left.label) &&
        entry.contexts.includes(right.label) &&
        entry.targets.includes(left.target) &&
        entry.targets.includes(right.target),
    )
    .map((entry) => entry.package);
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
    {
      label: "root-macos",
      manifest: join(repoRoot, "Cargo.toml"),
      target: macTarget,
    },
    {
      label: "macos-product",
      manifest: join(repoRoot, "macos/src-tauri/Cargo.toml"),
      target: macTarget,
    },
    {
      label: "root-windows",
      manifest: join(repoRoot, "Cargo.toml"),
      target: windowsTarget,
    },
    {
      label: "windows-product",
      manifest: join(repoRoot, "windows/src-tauri/Cargo.toml"),
      target: windowsTarget,
    },
  ];
  const policy = loadPolicy(policyPath);
  const resolved = contexts.map((context) => ({
    ...context,
    resolution: collectProductionResolution(
      cargoMetadata(context.manifest, context.target),
    ),
  }));
  const pairs = [
    [resolved[0], resolved[1]],
    [resolved[2], resolved[3]],
  ].map(([left, right]) => {
    const allowedPackages = allowedPackagesForPair(policy, left, right);
    const identityMismatches = comparePackageIdentities(
      left.resolution,
      right.resolution,
      allowedPackages,
    );
    const identityMismatchNames = new Set(
      identityMismatches.map(({ name }) => name),
    );
    const featureDifferences = compareResolutions(
      left.resolution,
      right.resolution,
      allowedPackages,
    ).filter(({ name }) => !identityMismatchNames.has(name));
    const requiredFeatureMatches = new Set(policy.requiredFeatureMatches);
    const requiredFeatureMismatches = featureDifferences.filter(({ name }) =>
      requiredFeatureMatches.has(name),
    );
    return {
      left: left.label,
      right: right.label,
      mismatches: [...identityMismatches, ...requiredFeatureMismatches],
      featureDifferences,
    };
  });

  console.log("Shared production dependency resolution");
  for (const pair of pairs) {
    console.log(
      `- ${pair.left} vs ${pair.right}: ${pair.mismatches.length === 0 ? "OK" : "MISMATCH"}`,
    );
    const informationalFeatures = pair.featureDifferences.filter(
      ({ name }) => !policy.requiredFeatureMatches.includes(name),
    );
    if (informationalFeatures.length > 0) {
      console.log(
        `  resolved feature differences (visible, non-blocking): ${informationalFeatures
          .map(({ name }) => name)
          .join(", ")}`,
      );
    }
    for (const mismatch of pair.mismatches) {
      console.log(
        `  ${mismatch.name}: ${mismatch.left.join(", ")} <> ${mismatch.right.join(", ")}`,
      );
      console.log(`    ${pair.left}: ${mismatch.leftPaths.join(" | ")}`);
      console.log(`    ${pair.right}: ${mismatch.rightPaths.join(" | ")}`);
    }
  }
  if (pairs.some(({ mismatches }) => mismatches.length > 0)) {
    throw new Error("shared production dependency resolution is inconsistent");
  }
  return 0;
}

const entrypoint =
  process.argv[1] &&
  pathToFileURL(resolve(process.argv[1])).href === import.meta.url;
if (entrypoint) {
  try {
    process.exitCode = run();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
