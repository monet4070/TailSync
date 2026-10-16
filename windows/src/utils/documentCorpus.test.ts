import { describe, expect, it } from "vitest";
import frameCorpus from "../../../shared/schema/fixtures/document-frame-corpus.json";
import { decodeDocumentFrame } from "./documentPreview";
import corpus from "../../../shared/schema/fixtures/document-corpus.json";
import { decodeDisplayDocument } from "../types/localContracts.generated";

type Expand =
  | { kind: "paragraphs" | "runs" | "cells"; count: number; groups?: number }
  | { kind: "text"; bytes: number; linkBytes?: number; groups?: number; character?: string }
  | { kind: "assets"; count: number; length: number; width: number; height: number }
  | { kind: "partials"; count: number; detailBytes: number };

type CorpusCase = {
  name: string;
  valid: boolean;
  expect?: { javascript?: boolean; swift?: boolean; rust?: boolean };
  value?: unknown;
  json?: string;
  expand?: Expand;
};

const style = () => ({
  bold: false,
  italic: false,
  underline: false,
  strikethrough: false,
  subscript: false,
  superscript: false,
});

const run = (text: string, link?: string) => ({
  text,
  style: style(),
  unsupported: false,
  ...(link === undefined ? {} : { link }),
});

const paragraph = (runs: unknown[]) => ({ kind: "paragraph", paragraph: { runs } });
const document = (blocks: unknown[]) => ({ version: 1, blocks, assets: [], partial: [], styles_partial: false });

// Input builder for the compact boundary descriptors. It only builds a
// document; it never validates one.
function expandDocument(expand: Expand): unknown {
  const groups = "groups" in expand ? expand.groups ?? 1 : 1;
  const counts = "count" in expand ? Array.from({ length: groups }, (_, index) => Math.floor(expand.count / groups) + Number(index < expand.count % groups)) : [];
  switch (expand.kind) {
    case "paragraphs":
      return document(Array.from({ length: expand.count }, () => paragraph([run("x")])));
    case "runs":
      return document(counts.map(count => paragraph(Array.from({ length: count }, () => run("x")))));
    case "cells":
      return document([{
        kind: "table",
        table: {
          rows: counts.map(count => ({ cells: Array.from({ length: count }, () => ({ header: false, runs: [run("x")] })) })),
          truncated: false,
        },
      }]);
    case "text":
      return document([paragraph(Array.from({ length: groups }, (_, index) => {
        const bytes = Math.floor(expand.bytes / groups) + Number(index < expand.bytes % groups);
        const character = expand.character ?? "x";
        const width = new TextEncoder().encode(character).length;
        const text = character.repeat(Math.floor(bytes / width)) + "x".repeat(bytes % width);
        return run(text, index === 0 && expand.linkBytes !== undefined ? `https://example.com/${"a".repeat(expand.linkBytes - 20)}` : undefined);
      }))]);
    case "assets":
      return {
        ...document(Array.from({ length: expand.count }, (_, index) => ({
          kind: "image",
          image: { asset: index, availability: "available", ordinal: index },
        }))),
        assets: Array.from({ length: expand.count }, (_, index) => ({
          index,
          offset: index * expand.length,
          length: expand.length,
          width: expand.width,
          height: expand.height,
          sha256: "0a".repeat(32),
        })),
        first_image: 0,
      };
    case "partials":
      return {
        ...document([paragraph([run("x")])]),
        partial: Array.from({ length: expand.count }, () => ({
          code: "other_native_items",
          detail: "d".repeat(expand.detailBytes),
        })),
      };
  }
}

const cases = corpus as unknown as CorpusCase[];

describe("shared document corpus", () => {
  it("matches the generated TypeScript decoder", () => {
    expect(cases.length).toBeGreaterThanOrEqual(100);
    expect(new Set(cases.map(item => item.name)).size).toBe(cases.length);
    for (const corpusCase of cases) {
      const expected = corpusCase.expect?.javascript ?? corpusCase.valid;
      const built = corpusCase.json !== undefined ? undefined : corpusCase.value ?? expandDocument(corpusCase.expand as Expand);
      const decode = () => decodeDisplayDocument(corpusCase.json !== undefined ? JSON.parse(corpusCase.json) : built);
      if (expected) expect(decode, corpusCase.name).not.toThrow();
      else expect(decode, corpusCase.name).toThrow();
    }
  });
});


describe("shared frame metadata corpus", () => {
  it("matches the real binary frame decoder", () => {
    const model = new TextEncoder().encode(frameCorpus.model_json);
    for (const item of frameCorpus.cases) {
      const raw = "json" in item ? item.json : JSON.stringify(item.value);
      const metadata = new TextEncoder().encode(raw);
      const frame = new Uint8Array(9 + metadata.length + model.length);
      frame.set(new TextEncoder().encode("TSDC"));
      frame[4] = 1;
      new DataView(frame.buffer).setUint32(5, metadata.length, true);
      frame.set(metadata, 9);
      frame.set(model, 9 + metadata.length);
      if (item.valid) expect(() => decodeDocumentFrame(frame), item.name).not.toThrow();
      else expect(() => decodeDocumentFrame(frame), item.name).toThrow();
    }
  });
});
