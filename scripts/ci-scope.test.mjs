import assert from "node:assert/strict";
import test from "node:test";

import { CI_JOBS, planCiScope } from "./ci-scope.mjs";

function enabled(plan) {
  return CI_JOBS.filter((job) => plan.jobs[job]);
}

test("documentation-only changes retain the lightweight fact checks", () => {
  const plan = planCiScope(["README.md", "docs/features/pairing.md"]);
  assert.equal(plan.mode, "targeted");
  assert.deepEqual(enabled(plan), ["scripts"]);
});

test("site changes run only the site frontend job", () => {
  const plan = planCiScope(["site/src/App.tsx", "deploy/nginx/site.conf"]);
  assert.deepEqual(enabled(plan), ["frontend_site"]);
});

test("ledger gates and their executor require execution on both native jobs", () => {
  for (const file of [
    'docs/remediation-ledger/entries/S1-P1-5.json',
    'scripts/check-remediation-ledger.mjs',
    'scripts/remediation-test-results.mjs',
    'scripts/run-remediation-tests.mjs',
  ]) {
    assert.deepEqual(enabled(planCiScope([file])), [
      'rust_windows', 'rust_macos', 'scripts', 'shared_resolution',
    ], file);
  }
});

test("Windows frontend changes run frontend and contract checks", () => {
  const plan = planCiScope([
    "windows/src/pages/Settings.tsx",
    "windows/vitest.config.ts",
  ]);
  assert.deepEqual(enabled(plan), ["frontend_windows", "shared_resolution"]);
});

test("Windows native changes stay on the Windows native path", () => {
  const plan = planCiScope(["windows/src-tauri/src/commands/settings.rs"]);
  assert.deepEqual(enabled(plan), ["rust_windows", "shared_resolution"]);
});

test("macOS changes stay on the macOS path", () => {
  const plan = planCiScope([
    "macos/swift-ui/Sources/TailSync/Views/ConnectionsView.swift",
  ]);
  assert.deepEqual(enabled(plan), ["rust_macos", "shared_resolution"]);
});

test("shared core changes verify both packaged applications", () => {
  const plan = planCiScope(["shared/rust-core/src/pairing/manager.rs"]);
  assert.deepEqual(enabled(plan), [
    "rust_windows",
    "rust_macos",
    "shared_resolution",
  ]);
});

test("shared schema changes also verify the Windows frontend", () => {
  const plan = planCiScope(["shared/schema/settings.schema.json"]);
  assert.deepEqual(enabled(plan), [
    "frontend_windows",
    "rust_windows",
    "rust_macos",
    "shared_resolution",
  ]);
});

test("platform scripts verify both platforms and script policies", () => {
  const plan = planCiScope(["windows/scripts/check_cross_platform_sync.mjs"]);
  assert.deepEqual(enabled(plan), [
    "rust_windows",
    "rust_macos",
    "scripts",
    "shared_resolution",
  ]);
});

test("Rust dependency inputs run native, policy, resolution, and advisory jobs", () => {
  const plan = planCiScope(["Cargo.lock"]);
  assert.deepEqual(enabled(plan), [
    "rust_windows",
    "rust_macos",
    "scripts",
    "shared_resolution",
    "rustsec",
  ]);
});

test("platform and shared Rust dependency changes keep advisory checks", () => {
  const cases = [
    ["windows/src-tauri/Cargo.lock", ["rust_windows", "scripts", "shared_resolution", "rustsec"]],
    ["macos/src-tauri/Cargo.toml", ["rust_macos", "scripts", "shared_resolution", "rustsec"]],
    [
      "shared/rust-core/Cargo.toml",
      ["rust_windows", "rust_macos", "scripts", "shared_resolution", "rustsec"],
    ],
  ];
  for (const [file, expected] of cases) {
    assert.deepEqual(enabled(planCiScope([file])), expected, file);
  }
});

test("version-bearing frontend manifests retain consistency checks", () => {
  assert.deepEqual(enabled(planCiScope(["site/package.json"])), [
    "frontend_site",
    "scripts",
  ]);
  assert.deepEqual(enabled(planCiScope(["windows/package-lock.json"])), [
    "frontend_windows",
    "scripts",
    "shared_resolution",
  ]);
});

test("security policy changes run policy and advisory jobs", () => {
  const plan = planCiScope(["security/rustsec-exceptions.json"]);
  assert.deepEqual(enabled(plan), ["scripts", "rustsec"]);
});

test("mixed changes union the required jobs", () => {
  const plan = planCiScope(["site/src/App.tsx", "macos/src-tauri/src/api.rs"]);
  assert.deepEqual(enabled(plan), [
    "frontend_site",
    "rust_macos",
    "shared_resolution",
  ]);
});

test("cross-area moves union the deleted and added path scopes", () => {
  const plan = planCiScope([
    "windows/src/removed-from-frontend.ts",
    "docs/moved-to-documentation.md",
  ]);
  assert.deepEqual(enabled(plan), [
    "frontend_windows",
    "scripts",
    "shared_resolution",
  ]);
});

test("workflow and planner changes force full verification", () => {
  for (const file of [
    ".github/workflows/ci.yml",
    "scripts/ci-scope.mjs",
    "scripts/ci-scope.test.mjs",
  ]) {
    const plan = planCiScope([file]);
    assert.equal(plan.mode, "full");
    assert.deepEqual(enabled(plan), CI_JOBS);
  }
});

test("unknown and empty change sets fail safe to full verification", () => {
  for (const files of [["new-top-level/input.bin"], []]) {
    const plan = planCiScope(files);
    assert.equal(plan.mode, "full");
    assert.deepEqual(enabled(plan), CI_JOBS);
  }
});

test("explicit full verification overrides targeted paths", () => {
  const plan = planCiScope(["README.md"], { forceFull: true });
  assert.equal(plan.mode, "full");
  assert.deepEqual(enabled(plan), CI_JOBS);
});
