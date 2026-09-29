#!/usr/bin/env node

import fs from "node:fs";
import { pathToFileURL } from "node:url";

export const CI_JOBS = Object.freeze([
  "frontend_windows",
  "frontend_site",
  "rust_windows",
  "rust_macos",
  "scripts",
  "shared_resolution",
  "rustsec",
]);

const FULL_SCOPE_PATHS = new Set([
  ".github/workflows/ci.yml",
  ".github/workflows/release.yml",
  ".github/workflows/rustsec.yml",
  "scripts/ci-scope.mjs",
  "scripts/ci-scope.test.mjs",
]);

const ROOT_RUST_INPUTS = new Set([
  "Cargo.lock",
  "Cargo.toml",
  "deny.toml",
  "rust-toolchain.toml",
]);

const VERSION_POLICY_PATHS = new Set([
  "README.md",
  "README.zh-CN.md",
  "CONTEXT.md",
  "docs/USER_GUIDE.zh-CN.md",
  "docs/THEMING.md",
  "windows/package.json",
  "windows/package-lock.json",
  "site/package.json",
  "site/package-lock.json",
  "windows/src-tauri/tauri.conf.json",
  "macos/src-tauri/tauri.conf.json",
  "macos/swift-ui/Sources/TailSync/TailSyncApp.swift",
]);

function emptyJobs() {
  return Object.fromEntries(CI_JOBS.map((job) => [job, false]));
}

function allJobs() {
  return Object.fromEntries(CI_JOBS.map((job) => [job, true]));
}

function enable(jobs, ...names) {
  for (const name of names) jobs[name] = true;
}

function normalizePath(file) {
  return file.replaceAll("\\", "/").replace(/^\.\/+/, "");
}

function isDocumentation(file) {
  return (
    file === "LICENSE" ||
    file.endsWith(".md") ||
    file.startsWith("docs/") ||
    file.startsWith(".github/ISSUE_TEMPLATE/") ||
    file.startsWith(".github/PULL_REQUEST_TEMPLATE/")
  );
}

function isWindowsFrontend(file) {
  return (
    file.startsWith("windows/src/") ||
    file.startsWith("windows/public/") ||
    /^windows\/(?:[^/]+\.html|package(?:-lock)?\.json|(?:tsconfig|vite|vitest|eslint)[^/]*\.(?:js|json|ts)|\.oxlintrc\.json)$/.test(
      file,
    )
  );
}

function classify(file, jobs, categories) {
  if (FULL_SCOPE_PATHS.has(file) || file.startsWith(".github/workflows/")) {
    categories.add("ci-policy");
    return false;
  }

  if (VERSION_POLICY_PATHS.has(file)) {
    enable(jobs, "scripts");
    categories.add("version-policy");
  }

  if (file.startsWith("site/") || file.startsWith("deploy/")) {
    enable(jobs, "frontend_site");
    categories.add("site");
    return true;
  }

  if (isDocumentation(file)) {
    // The script gate checks checked-in version markers and related facts.
    enable(jobs, "scripts");
    categories.add("documentation");
    return true;
  }

  if (file.startsWith("windows/scripts/") || file.startsWith("macos/scripts/")) {
    enable(
      jobs,
      "rust_windows",
      "rust_macos",
      "scripts",
      "shared_resolution",
    );
    categories.add("platform-scripts");
    return true;
  }

  if (isWindowsFrontend(file)) {
    enable(jobs, "frontend_windows", "shared_resolution");
    categories.add("windows-frontend");
    return true;
  }

  if (/^windows\/src-tauri\/Cargo\.(?:toml|lock)$/.test(file)) {
    enable(jobs, "rust_windows", "scripts", "shared_resolution", "rustsec");
    categories.add("windows-rust-dependencies");
    return true;
  }

  if (file.startsWith("windows/")) {
    enable(jobs, "rust_windows", "shared_resolution");
    categories.add("windows-native");
    return true;
  }

  if (/^macos\/src-tauri\/Cargo\.(?:toml|lock)$/.test(file)) {
    enable(jobs, "rust_macos", "scripts", "shared_resolution", "rustsec");
    categories.add("macos-rust-dependencies");
    return true;
  }

  if (file.startsWith("macos/")) {
    enable(jobs, "rust_macos", "shared_resolution");
    categories.add("macos");
    return true;
  }

  if (/^shared\/[^/]+\/Cargo\.toml$/.test(file)) {
    enable(
      jobs,
      "rust_windows",
      "rust_macos",
      "scripts",
      "shared_resolution",
      "rustsec",
    );
    categories.add("shared-rust-dependencies");
    return true;
  }

  if (file.startsWith("shared/schema/")) {
    enable(
      jobs,
      "frontend_windows",
      "rust_windows",
      "rust_macos",
      "shared_resolution",
    );
    categories.add("shared-schema");
    return true;
  }

  if (file.startsWith("shared/")) {
    enable(jobs, "rust_windows", "rust_macos", "shared_resolution");
    categories.add("shared");
    return true;
  }

  if (file.startsWith("scripts/")) {
    enable(jobs, "scripts", "shared_resolution");
    categories.add("repository-scripts");
    return true;
  }

  if (file === ".github/dependabot.yml") {
    enable(jobs, "scripts");
    categories.add("dependency-policy");
    return true;
  }

  if (file.startsWith("security/")) {
    enable(jobs, "scripts", "rustsec");
    categories.add("security-policy");
    return true;
  }

  if (ROOT_RUST_INPUTS.has(file)) {
    enable(
      jobs,
      "rust_windows",
      "rust_macos",
      "scripts",
      "shared_resolution",
      "rustsec",
    );
    categories.add("root-rust-input");
    return true;
  }

  if (file.startsWith("themes/") || file.startsWith("assets/")) {
    enable(jobs, "rust_windows", "rust_macos", "scripts");
    categories.add("packaged-assets");
    return true;
  }

  return false;
}

export function planCiScope(files, { forceFull = false } = {}) {
  const normalizedFiles = [...new Set(files.map(normalizePath).filter(Boolean))];

  if (forceFull) {
    return {
      mode: "full",
      reason: "non-PR or explicitly requested full verification",
      files: normalizedFiles,
      categories: ["full"],
      jobs: allJobs(),
    };
  }

  if (normalizedFiles.length === 0) {
    return {
      mode: "full",
      reason: "empty change set; fail-safe full verification",
      files: normalizedFiles,
      categories: ["empty-change-set"],
      jobs: allJobs(),
    };
  }

  const jobs = emptyJobs();
  const categories = new Set();
  const unknownFiles = [];

  for (const file of normalizedFiles) {
    if (!classify(file, jobs, categories)) unknownFiles.push(file);
  }

  if (unknownFiles.length > 0) {
    return {
      mode: "full",
      // Do not copy repository path text into GITHUB_OUTPUT. Git permits
      // newlines in file names, so raw paths could corrupt the output file.
      reason: `unclassified path count: ${unknownFiles.length}`,
      files: normalizedFiles,
      categories: [...categories, "unclassified"].sort(),
      jobs: allJobs(),
    };
  }

  return {
    mode: "targeted",
    reason: "all changed paths matched explicit routing rules",
    files: normalizedFiles,
    categories: [...categories].sort(),
    jobs,
  };
}

function parseArguments(argv) {
  const args = { files: null, forceFull: false, githubOutput: null };
  for (let index = 0; index < argv.length; index += 1) {
    const name = argv[index];
    const value = argv[index + 1];
    if (name === "--files" && value) {
      args.files = value;
      index += 1;
    } else if (name === "--force-full" && value) {
      args.forceFull = value === "true";
      index += 1;
    } else if (name === "--github-output" && value) {
      args.githubOutput = value;
      index += 1;
    } else {
      throw new Error(`Unknown or incomplete argument: ${name}`);
    }
  }
  if (!args.files) throw new Error("--files is required");
  return args;
}

function readNullDelimitedFiles(file) {
  const bytes = fs.readFileSync(file);
  return bytes
    .toString("utf8")
    .split("\0")
    .filter(Boolean);
}

function writeGitHubOutputs(file, plan) {
  const lines = [
    `mode=${plan.mode}`,
    `reason=${plan.reason}`,
    `categories=${plan.categories.join(",")}`,
    ...CI_JOBS.map((job) => `${job}=${plan.jobs[job]}`),
  ];
  fs.appendFileSync(file, `${lines.join("\n")}\n`);
}

function printPlan(plan) {
  const enabled = CI_JOBS.filter((job) => plan.jobs[job]);
  const skipped = CI_JOBS.filter((job) => !plan.jobs[job]);
  console.log(`CI mode: ${plan.mode}`);
  console.log(`Reason: ${plan.reason}`);
  console.log(`Categories: ${plan.categories.join(", ") || "none"}`);
  console.log(`Changed files: ${plan.files.length}`);
  console.log(`Run: ${enabled.join(", ") || "no scoped jobs"}`);
  console.log(`Skip: ${skipped.join(", ") || "none"}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = parseArguments(process.argv.slice(2));
  const plan = planCiScope(readNullDelimitedFiles(args.files), {
    forceFull: args.forceFull,
  });
  printPlan(plan);
  if (args.githubOutput) writeGitHubOutputs(args.githubOutput, plan);
}
