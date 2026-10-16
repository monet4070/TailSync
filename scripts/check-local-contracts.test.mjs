import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { validateDocumentCorpus, validateDocumentFrameCorpus, validateFixtures } from "./check-local-contracts.mjs";
import { decodeDisplayDocument, decodePairingFailureKind, decodePairingStatus } from "../shared/schema/local-contract-decoders.generated.mjs";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function fixtureValue(name) {
  const fixtures = JSON.parse(readFileSync(resolve(repositoryRoot, "shared/schema/fixtures/local-contracts.json"), "utf8"));
  const fixture = fixtures.find(candidate => candidate.name === name);
  assert.ok(fixture, name);
  return fixture;
}

test("local contract fixtures accept valid samples and reject invalid samples", () => {
  assert.deepEqual(validateFixtures(resolve(dirname(fileURLToPath(import.meta.url)), "..")), {
    schemaVersion: 2,
    wireVersion: 5,
  });
});

test("document links share the bounded UTF8 text budget", () => {
  const style = { bold: false, italic: false, underline: false, strikethrough: false, subscript: false, superscript: false };
  const link = `https://example.com/${"x".repeat(2000)}`;
  const document = {
    version: 1, assets: [], partial: [], styles_partial: false,
    blocks: [{ kind: "paragraph", paragraph: { runs: Array.from({ length: 525 }, () => ({ text: "字", style, unsupported: false, link })) } }],
  };
  assert.throws(() => decodeDisplayDocument(document));
  document.blocks[0].paragraph.runs = document.blocks[0].paragraph.runs.slice(0, 2);
  assert.doesNotThrow(() => decodeDisplayDocument(document));
});

test("document links reject control scalars before URL normalization", () => {
  const style = { bold: false, italic: false, underline: false, strikethrough: false, subscript: false, superscript: false };
  const documentWithLink = link => ({
    version: 1, assets: [], partial: [], styles_partial: false,
    blocks: [{ kind: "paragraph", paragraph: { runs: [{ text: "reading", style, unsupported: false, link }] } }],
  });
  for (let scalar = 0; scalar <= 159; scalar++) {
    if (scalar > 31 && scalar < 127) continue;
    const link = `https://example.com/${String.fromCodePoint(scalar)}reading`;
    assert.throws(() => decodeDisplayDocument(documentWithLink(link)), `control scalar ${scalar}`);
  }
  // Unicode White_Space, exactly as the Rust model treats it. U+FEFF and
  // U+200B are *not* White_Space, so they must survive URL normalization.
  for (const character of [" ", "\u00a0", "\u1680", "\u2003", "\u2028", "\u2029", "\u202f", "\u205f", "\u3000", "\\"]) {
    assert.throws(() => decodeDisplayDocument(documentWithLink(`https://example.com/${character}reading`)));
  }
  for (const character of ["\u200b", "\ufeff", "\u180e", "\u2060"]) {
    assert.doesNotThrow(() => decodeDisplayDocument(documentWithLink(`https://example.com/${character}reading`)), `separator ${character.codePointAt(0).toString(16)}`);
  }
  for (const link of ["https://@example.com/", "https://user:password@example.com/", "javascript:alert(1)", "file:///tmp/reading", "//example.com/reading"]) {
    assert.throws(() => decodeDisplayDocument(documentWithLink(link)));
  }
  for (const link of ["HTTP://example.com/reading", "https://example.com/中文😀", "https://example.com/@reading", "https://example.com/%00reading"]) {
    assert.doesNotThrow(() => decodeDisplayDocument(documentWithLink(link)));
  }
});

test("the shared document corpus matches the generated decoder", () => {
  assert.ok(validateDocumentCorpus(resolve(dirname(fileURLToPath(import.meta.url)), "..")) >= 100);
});


test("the shared frame metadata corpus matches the generated decoder", () => {
  assert.ok(validateDocumentFrameCorpus(resolve(dirname(fileURLToPath(import.meta.url)), "..")) > 0);
});

test("a future pairing failure kind stays decodable and normalizes to unknown", () => {
  const production = fixtureValue("PairingStatus: production output");
  const classified = fixtureValue("PairingStatus: classified failure");
  const future = fixtureValue("PairingStatus: future failure kind normalizes to unknown");
  const nullKind = fixtureValue("PairingStatus: null failure kind stays null");
  const wrongType = fixtureValue("PairingStatus: non-string failure kind is rejected");

  // A newer producer's member must not fail the whole status, and the value the
  // consumer sees has to be the known `unknown` member, not the raw string.
  assert.equal(future.valid, true);
  assert.equal(decodePairingStatus(future.value).failure_kind, "unknown");
  // Known members and the legacy absent/null shapes are preserved verbatim.
  assert.equal(decodePairingStatus(classified.value).failure_kind, "connection_closed");
  assert.equal(decodePairingStatus(nullKind.value).failure_kind, null);
  assert.equal(Object.hasOwn(decodePairingStatus(production.value), "failure_kind"), false);
  // A non-string value is still rejected: tolerance is not a type escape hatch.
  assert.equal(wrongType.valid, false);
  assert.throws(() => decodePairingStatus(wrongType.value));

  // The standalone pairing-kind decoder is tolerant in exactly the same way, so
  // a newer producer's member cannot fail a bare kind either.
  assert.equal(decodePairingFailureKind(fixtureValue("PairingFailureKind: future member normalizes to unknown").value), "unknown");
  assert.equal(decodePairingFailureKind(fixtureValue("PairingFailureKind: known member is preserved").value), "window_closed");
  assert.throws(() => decodePairingFailureKind(fixtureValue("PairingFailureKind: non-string member is rejected").value));
});
