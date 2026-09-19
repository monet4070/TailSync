#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";
import * as contracts from "../shared/schema/local-contract-decoders.generated.mjs";

export function validateFixtures(repoRoot) {
  const fixtures = JSON.parse(readFileSync(join(repoRoot, "shared/schema/fixtures/local-contracts.json"), "utf8"));
  for (const fixture of fixtures) {
    const decoder = contracts[`decode${fixture.contract}`];
    if (!decoder) throw new Error(`Missing generated decoder: ${fixture.contract}`);
    let accepted = true;
    try { decoder(fixture.value); } catch { accepted = false; }
    if (accepted !== (fixture.typescriptValid ?? fixture.valid)) throw new Error(`Contract fixture failed: ${fixture.name}`);
  }
  const capabilities = fixtures.find(fixture => fixture.contract === "LocalCapabilities" && fixture.valid).value;
  return { schemaVersion: capabilities.schema_version, wireVersion: capabilities.wire_version };
}
export function run(argv = process.argv.slice(2)) {
  const index = argv.indexOf("--root");
  const root = resolve(index < 0 ? process.cwd() : argv[index + 1]);
  const generation = spawnSync(process.execPath, [join(root,"shared/schema/generate-local-contracts.mjs"), "--check"], { cwd: root, encoding: "utf8" });
  if (generation.status !== 0) throw new Error(generation.stderr || generation.stdout);
  const registry = spawnSync(process.execPath, [join(root,"shared/schema/generate-local-commands.mjs"), "--check"], { cwd: root, encoding: "utf8" });
  if (registry.status !== 0) throw new Error(registry.stderr || registry.stdout);
  const { schemaVersion, wireVersion } = validateFixtures(root);
  console.log(`Production local contract decoders passed: schema v${schemaVersion}, wire v${wireVersion}`);
  return 0;
}
if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try { process.exitCode = run(); } catch (error) { console.error(error); process.exitCode = 1; }
}
