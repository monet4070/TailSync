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

// Input builder for the compact boundary descriptors in the shared document
// corpus. It only builds a document; it never validates one.
export function expandDocumentCase(expand) {
  const document = blocks => ({ version: 1, blocks, assets: [], partial: [], styles_partial: false });
  const run = (text, link) => ({
    text,
    style: { bold: false, italic: false, underline: false, strikethrough: false, subscript: false, superscript: false },
    unsupported: false,
    ...(link === undefined ? {} : { link }),
  });
  const paragraph = runs => ({ kind: "paragraph", paragraph: { runs } });
  const groups = expand.groups ?? 1;
  const counts = expand.count === undefined ? [] : Array.from({ length: groups }, (_, index) => Math.floor(expand.count / groups) + Number(index < expand.count % groups));
  const utf8Text = bytes => {
    const character = expand.character ?? "x";
    const width = new TextEncoder().encode(character).length;
    return character.repeat(Math.floor(bytes / width)) + "x".repeat(bytes % width);
  };
  switch (expand.kind) {
    case "paragraphs": return document(Array.from({ length: expand.count }, () => paragraph([run("x")])));
    case "runs": return document(counts.map(count => paragraph(Array.from({ length: count }, () => run("x")))));
    case "cells": return document([{ kind: "table", table: { rows: counts.map(count => ({ cells: Array.from({ length: count }, () => ({ header: false, runs: [run("x")] })) })), truncated: false } }]);
    case "text": return document([paragraph(Array.from({ length: groups }, (_, index) => run(
      utf8Text(Math.floor(expand.bytes / groups) + Number(index < expand.bytes % groups)),
      index === 0 && expand.linkBytes !== undefined ? `https://example.com/${"a".repeat(expand.linkBytes - 20)}` : undefined,
    )))]);
    case "assets": return {
      ...document(Array.from({ length: expand.count }, (_, index) => ({ kind: "image", image: { asset: index, availability: "available", ordinal: index } }))),
      assets: Array.from({ length: expand.count }, (_, index) => ({ index, offset: index * expand.length, length: expand.length, width: expand.width, height: expand.height, sha256: "0a".repeat(32) })),
      first_image: 0,
    };
    case "partials": return { ...document([paragraph([run("x")])]), partial: Array.from({ length: expand.count }, () => ({ code: "other_native_items", detail: "d".repeat(expand.detailBytes) })) };
    default: throw new Error(`Unknown document corpus expand kind: ${expand.kind}`);
  }
}

// The shared document corpus is the cross-language semantic guard: every case
// runs through the real generated decoder, not a second hand-written formula.
export function validateDocumentCorpus(repoRoot) {
  const cases = JSON.parse(readFileSync(join(repoRoot, "shared/schema/fixtures/document-corpus.json"), "utf8"));
  const failures = [];
  if (new Set(cases.map(item => item.name)).size !== cases.length) throw new Error("Duplicate document corpus case name");
  for (const corpusCase of cases) {
    const expected = corpusCase.expect?.javascript ?? corpusCase.valid;
    const built = "json" in corpusCase ? undefined : corpusCase.value ?? expandDocumentCase(corpusCase.expand);
    let accepted = true;
    try {
      contracts.decodeDisplayDocument("json" in corpusCase ? JSON.parse(corpusCase.json) : built);
    } catch { accepted = false; }
    if (accepted !== expected) failures.push(corpusCase.name);
  }
  if (failures.length) throw new Error(`Document corpus mismatches: ${failures.join(", ")}`);
  return cases.length;
}
export function validateDocumentFrameCorpus(repoRoot) {
  const { cases } = JSON.parse(readFileSync(join(repoRoot, "shared/schema/fixtures/document-frame-corpus.json"), "utf8"));
  if (new Set(cases.map(item => item.name)).size !== cases.length) throw new Error("Duplicate frame corpus case name");
  const failures = cases.filter(item => {
    let accepted = true;
    try { contracts.decodeDocumentFrameMetadata(item.json === undefined ? item.value : JSON.parse(item.json)); } catch { accepted = false; }
    return accepted !== item.valid;
  }).map(item => item.name);
  if (failures.length) throw new Error(`Frame corpus mismatches: ${failures.join(", ")}`);
  return cases.length;
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
  const documentCases = validateDocumentCorpus(root);
  const frameCases = validateDocumentFrameCorpus(root);
  const windowsCommands = validateWindowsCommandErrors(root);
  const macCommands = validateMacCommandErrors(root);
  console.log(`Production local contract decoders passed: schema v${schemaVersion}, wire v${wireVersion}, ${documentCases} document corpus cases, ${frameCases} frame corpus cases, ${windowsCommands} Windows and ${macCommands} macOS commands classified`);
  return 0;
}
if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try { process.exitCode = run(); } catch (error) { console.error(error); process.exitCode = 1; }
}
