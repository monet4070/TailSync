import test from "node:test";
import assert from "node:assert/strict";
import {
  collectProductionResolution,
  compareResolutions,
} from "./check-shared-resolution.mjs";

function metadata({ shared = "1.0.0", devOnly = false } = {}) {
  const packages = [
    { id: "path+root#0", name: "tailsync-core", version: "0.1.0", source: null },
    {
      id: `registry+dep#${shared}`,
      name: "shared-dep",
      version: shared,
      source: "registry+https://github.com/rust-lang/crates.io-index",
    },
  ];
  const deps = [{ pkg: `registry+dep#${shared}`, dep_kinds: [{ kind: null }] }];
  if (devOnly) {
    packages.push({
      id: "registry+dev-only#1",
      name: "dev-only",
      version: "1.0.0",
      source: "registry+https://github.com/rust-lang/crates.io-index",
    });
    deps.push({ pkg: "registry+dev-only#1", dep_kinds: [{ kind: "dev" }] });
  }
  return { packages, resolve: { nodes: [{ id: "path+root#0", deps }] } };
}

test("collects only production dependencies", () => {
  assert.deepEqual(
    [...collectProductionResolution(metadata({ devOnly: true })).keys()],
    ["shared-dep"],
  );
});

test("reports a common package version mismatch", () => {
  const left = collectProductionResolution(metadata({ shared: "1.0.0" }));
  const right = collectProductionResolution(metadata({ shared: "1.1.0" }));
  assert.deepEqual(compareResolutions(left, right), [
    {
      name: "shared-dep",
      left: ["1.0.0|registry+https://github.com/rust-lang/crates.io-index"],
      right: ["1.1.0|registry+https://github.com/rust-lang/crates.io-index"],
    },
  ]);
});

test("does not fail for a target-only package", () => {
  const left = collectProductionResolution(metadata());
  const right = collectProductionResolution(metadata({ devOnly: true }));
  assert.deepEqual(compareResolutions(left, right), []);
});
