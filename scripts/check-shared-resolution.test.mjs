import test from "node:test";
import assert from "node:assert/strict";
import {
  collectProductionResolution,
  comparePackageIdentities,
  compareResolutions,
  validatePolicy,
} from "./check-shared-resolution.mjs";

function metadata({ shared = "1.0.0", features = [], devOnly = false } = {}) {
  const packages = [
    {
      id: "path+root#0",
      name: "tailsync-core",
      version: "0.1.0",
      source: null,
    },
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
  return {
    packages,
    resolve: {
      nodes: [
        { id: "path+root#0", deps, features: [] },
        { id: `registry+dep#${shared}`, deps: [], features },
      ],
    },
  };
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
      left: [
        "1.0.0|registry+https://github.com/rust-lang/crates.io-index|features=-",
      ],
      right: [
        "1.1.0|registry+https://github.com/rust-lang/crates.io-index|features=-",
      ],
      leftPaths: ["tailsync-core -> shared-dep"],
      rightPaths: ["tailsync-core -> shared-dep"],
    },
  ]);
});

test("reports a feature-only production mismatch with its shortest chain", () => {
  const left = collectProductionResolution(metadata({ features: ["alpha"] }));
  const right = collectProductionResolution(metadata({ features: ["beta"] }));
  const [mismatch] = compareResolutions(left, right);
  assert.equal(mismatch.name, "shared-dep");
  assert.match(mismatch.left[0], /features=alpha$/);
  assert.match(mismatch.right[0], /features=beta$/);
  assert.equal(mismatch.leftPaths[0], "tailsync-core -> shared-dep");
});

test("keeps feature union visible without treating it as an identity mismatch", () => {
  const left = collectProductionResolution(metadata({ features: ["alpha"] }));
  const right = collectProductionResolution(metadata({ features: ["beta"] }));
  assert.deepEqual(comparePackageIdentities(left, right), []);
  assert.equal(compareResolutions(left, right).length, 1);
});

test("does not fail for a target-only package", () => {
  const left = collectProductionResolution(metadata());
  const right = collectProductionResolution(metadata({ devOnly: true }));
  assert.deepEqual(compareResolutions(left, right), []);
});

test("requires structured, reviewable mismatch exceptions", () => {
  assert.throws(
    () =>
      validatePolicy({
        allowedMismatches: ["shared-dep"],
        requiredFeatureMatches: [],
      }),
    /must define package, two contexts, targets, reason, owner, and reviewWhen/,
  );
  assert.doesNotThrow(() =>
    validatePolicy({
      requiredFeatureMatches: ["shared-dep"],
      allowedMismatches: [
        {
          package: "shared-dep",
          contexts: ["root-macos", "macos-product"],
          targets: ["aarch64-apple-darwin"],
          reason: "platform adapter",
          owner: "maintainers",
          reviewWhen: "dependency changes",
        },
      ],
    }),
  );
});

test("requires feature enforcement policy to be explicit", () => {
  assert.throws(
    () => validatePolicy({ allowedMismatches: [] }),
    /requiredFeatureMatches string array/,
  );
});
