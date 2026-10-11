import assert from "node:assert/strict";
import { test } from "node:test";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { validateFixtures } from "./check-local-contracts.mjs";
import { decodeDisplayDocument } from "../shared/schema/local-contract-decoders.generated.mjs";

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
  for (const character of [" ", "\u00a0", "\u1680", "\u2003", "\u2028", "\u2029", "\u202f", "\u205f", "\u3000", "\ufeff", "\\"]) {
    assert.throws(() => decodeDisplayDocument(documentWithLink(`https://example.com/${character}reading`)));
  }
  for (const link of ["https://@example.com/", "https://user:password@example.com/", "javascript:alert(1)", "file:///tmp/reading", "//example.com/reading"]) {
    assert.throws(() => decodeDisplayDocument(documentWithLink(link)));
  }
  for (const link of ["HTTP://example.com/reading", "https://example.com/中文😀", "https://example.com/@reading", "https://example.com/%00reading"]) {
    assert.doesNotThrow(() => decodeDisplayDocument(documentWithLink(link)));
  }
});
