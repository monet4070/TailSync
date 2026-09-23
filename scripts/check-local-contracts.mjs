#!/usr/bin/env node
import { readFileSync, readdirSync } from "node:fs";
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
export function validateWindowsCommandErrors(repoRoot) {
  const sourceDir = join(repoRoot, "windows/src-tauri/src");
  const handler = readFileSync(join(sourceDir, "tauri-handler.generated.rs"), "utf8");
  const names = [...handler.matchAll(/(?:commands|preview_window)::(\w+),/g)].map(match => match[1]);
  const sources = [
    ...readdirSync(join(sourceDir, "commands")).filter(name => name.endsWith(".rs"))
      .map(name => readFileSync(join(sourceDir, "commands", name), "utf8")),
    readFileSync(join(sourceDir, "preview_window.rs"), "utf8"),
  ];
  for (const name of names) {
    const declaration = sources.map(source => source.match(new RegExp(`#\\[command\\]\\s*pub\\s+(?:async\\s+)?fn\\s+${name}\\b[\\s\\S]*?\\{`))?.[0]).find(Boolean);
    if (!declaration) throw new Error(`Missing Windows Tauri command declaration: ${name}`);
    if (declaration.includes("-> Result<") &&
        !/,\s*(?:CommandError|crate::commands::CommandError|db::PreviewErrorInfo)>\s*\{$/.test(declaration)) {
      throw new Error(`Windows Tauri command has unclassified error: ${name}`);
    }
  }
  return names.length;
}
export function validateMacCommandErrors(repoRoot) {
  const sourceDir = join(repoRoot, "macos/src-tauri/src");
  const handler = readFileSync(join(sourceDir, "tauri-handler.generated.rs"), "utf8");
  const names = [...handler.matchAll(/commands::(\w+),/g)].map(match => match[1]);
  const sources = readdirSync(join(sourceDir, "commands")).filter(name => name.endsWith(".rs"))
    .map(name => readFileSync(join(sourceDir, "commands", name), "utf8"));
  for (const name of names) {
    const declaration = sources.map(source => source.match(new RegExp(`#\\[command\\]\\s*pub\\s+(?:async\\s+)?fn\\s+${name}\\b[\\s\\S]*?\\{`))?.[0]).find(Boolean);
    if (!declaration) throw new Error(`Missing macOS Tauri command declaration: ${name}`);
    if (declaration.includes("-> Result<") && !/,\s*CommandError>\s*\{$/.test(declaration)) {
      throw new Error(`macOS Tauri command has unclassified error: ${name}`);
    }
  }
  return names.length;
}
export function run(argv = process.argv.slice(2)) {
  const index = argv.indexOf("--root");
  const root = resolve(index < 0 ? process.cwd() : argv[index + 1]);
  const generation = spawnSync(process.execPath, [join(root,"shared/schema/generate-local-contracts.mjs"), "--check"], { cwd: root, encoding: "utf8" });
  if (generation.status !== 0) throw new Error(generation.stderr || generation.stdout);
  const registry = spawnSync(process.execPath, [join(root,"shared/schema/generate-local-commands.mjs"), "--check"], { cwd: root, encoding: "utf8" });
  if (registry.status !== 0) throw new Error(registry.stderr || registry.stdout);
  const { schemaVersion, wireVersion } = validateFixtures(root);
  const windowsCommands = validateWindowsCommandErrors(root);
  const macCommands = validateMacCommandErrors(root);
  console.log(`Production local contract decoders passed: schema v${schemaVersion}, wire v${wireVersion}, ${windowsCommands} Windows and ${macCommands} macOS commands classified`);
  return 0;
}
if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try { process.exitCode = run(); } catch (error) { console.error(error); process.exitCode = 1; }
}
