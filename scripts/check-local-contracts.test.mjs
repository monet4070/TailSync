import assert from "node:assert/strict";
import { test } from "node:test";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { validateFixtures } from "./check-local-contracts.mjs";

test("local contract fixtures accept valid samples and reject invalid samples", () => {
  assert.deepEqual(validateFixtures(resolve(dirname(fileURLToPath(import.meta.url)), "..")), {
    schemaVersion: 1,
    wireVersion: 5,
  });
});
