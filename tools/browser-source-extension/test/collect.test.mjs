import test from "node:test";
import assert from "node:assert/strict";
import { deflateSync } from "node:zlib";
import { collectInPage, SOURCE_PACKET_KIND } from "../collect.js";

function crc32(buffer) {
  let crc = ~0;
  for (const byte of buffer) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
  }
  return (~crc) >>> 0;
}

function chunk(type, data) {
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const out = Buffer.alloc(8 + data.length + 4);
  out.writeUInt32BE(data.length, 0);
  body.copy(out, 4);
  out.writeUInt32BE(crc32(body), 8 + data.length);
  return out;
}

function makePng(width, height, animated = false) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  const raw = Buffer.alloc(height * (1 + width * 4));
  const parts = [Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr)];
  if (animated) {
    const actl = Buffer.alloc(8);
    actl.writeUInt32BE(1, 0);
    actl.writeUInt32BE(0, 4);
    parts.push(chunk("acTL", actl));
  }
  parts.push(chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0)));
  return Buffer.concat(parts);
}

async function sha256Hex(value) {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)));
  return [...digest].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function image(raw, resolved) {
  return { getAttribute: (name) => (name === "src" ? raw : null), currentSrc: resolved, src: resolved };
}

function installDocument(images) {
  globalThis.location = { origin: "https://example.com" };
  globalThis.document = { querySelectorAll: (selector) => (selector === "img" ? images : []) };
}

function response(bytes, { mime = "image/png", ok = true } = {}) {
  let read = false;
  return {
    ok,
    headers: { get: (name) => (name.toLowerCase() === "content-type" ? mime : null) },
    body: {
      getReader: () => ({
        read: async () => {
          if (read) return { done: true };
          read = true;
          return { done: false, value: bytes };
        },
        cancel: async () => {},
      }),
    },
  };
}

test("collects an exact same-origin PNG and keeps credentials same-origin", async () => {
  const png = makePng(2, 2);
  const src = "blob:https://example.com/one";
  const hash = await sha256Hex(src);
  const calls = [];
  installDocument([image(src, src)]);
  globalThis.fetch = async (url, init) => {
    calls.push({ url, init });
    return response(png);
  };
  const result = await collectInPage([{ index: 0, src_sha256: hash }], "https://example.com");
  assert.deepEqual(result.failures, []);
  assert.equal(result.entries.length, 1);
  assert.equal(result.entries[0].index, 0);
  assert.equal(result.entries[0].src_sha256, hash);
  assert.equal(result.entries[0].png_b64, Buffer.from(png).toString("base64"));
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, src);
  assert.equal(calls[0].init.credentials, "same-origin");
  assert.equal(calls[0].init.redirect, "error");
});

test("matches the absolute resolved src as well as the raw attribute", async () => {
  const png = makePng(1, 1);
  const raw = "/image.png";
  const resolved = "https://example.com/image.png";
  const hash = await sha256Hex(resolved);
  installDocument([image(raw, resolved)]);
  globalThis.fetch = async () => response(png);
  const result = await collectInPage([{ index: 0, src_sha256: hash }], "https://example.com");
  assert.equal(result.entries.length, 1);
  assert.deepEqual(result.failures, []);
});

test("refuses ambiguous, missing and cross-origin nodes without fetching", async () => {
  let fetches = 0;
  globalThis.fetch = async () => {
    fetches += 1;
    return response(makePng(1, 1));
  };
  const raw = "https://example.com/a.png";
  const hash = await sha256Hex(raw);
  installDocument([image(raw, raw), image(raw, raw)]);
  const ambiguous = await collectInPage([{ index: 0, src_sha256: hash }], "https://example.com");
  assert.deepEqual(ambiguous.failures, [{ index: 0, code: "source_ambiguous" }]);
  assert.equal(ambiguous.entries.length, 0);

  installDocument([image("https://example.com/other.png", "https://example.com/other.png")]);
  const missing = await collectInPage([{ index: 1, src_sha256: hash }], "https://example.com");
  assert.deepEqual(missing.failures, [{ index: 1, code: "source_node_missing" }]);

  installDocument([image("https://cdn.example.com/a.png", "https://cdn.example.com/a.png")]);
  const cross = await collectInPage(
    [{ index: 2, src_sha256: await sha256Hex("https://cdn.example.com/a.png") }],
    "https://example.com",
  );
  assert.deepEqual(cross.failures, [{ index: 2, code: "source_origin_mismatch" }]);
  assert.equal(fetches, 0);
});

test("refuses non-PNG, APNG and oversized bytes", async () => {
  const raw = "https://example.com/a.png";
  const hash = await sha256Hex(raw);
  installDocument([image(raw, raw)]);

  globalThis.fetch = async () => response(Buffer.from("not an image"), { mime: "text/html" });
  assert.deepEqual((await collectInPage([{ index: 0, src_sha256: hash }], "https://example.com")).failures, [
    { index: 0, code: "source_mime" },
  ]);

  globalThis.fetch = async () => response(makePng(1, 1, true));
  assert.deepEqual((await collectInPage([{ index: 0, src_sha256: hash }], "https://example.com")).failures, [
    { index: 0, code: "source_decode" },
  ]);

  globalThis.fetch = async () => response(Buffer.alloc(9 * 1024 * 1024));
  assert.deepEqual((await collectInPage([{ index: 0, src_sha256: hash }], "https://example.com")).failures, [
    { index: 0, code: "source_resource_too_large" },
  ]);
});

test("declares the packet kind the TailSync importer expects", () => {
  assert.equal(SOURCE_PACKET_KIND, "tailsync-source-packet");
});

test("body timeout remains bounded when cancellation is ignored", async () => {
  const src = "https://example.com/a.png";
  installDocument([image(src, src)]);
  globalThis.fetch = async () => ({ ok: true, headers: { get: () => "image/png" }, body: { getReader: () => ({ read: () => new Promise(() => {}), cancel: () => new Promise(() => {}) }) } });
  const result = await collectInPage([{ index: 0, src_sha256: await sha256Hex(src) }], "https://example.com", { requestTimeoutMs: 10, totalDeadlineMs: 30 });
  assert.deepEqual(result.failures, [{ index: 0, code: "source_deadline" }]);
});

test("DOM inventory has a hard cap before hashing or network access", async () => {
  const src = "https://example.com/a.png";
  installDocument(Array.from({ length: 4097 }, () => image(src, src)));
  globalThis.fetch = () => { throw new Error("network must not run"); };
  const result = await collectInPage([{ index: 0, src_sha256: await sha256Hex(src) }], "https://example.com");
  assert.deepEqual(result.failures, [{ index: 0, code: "source_dom_limit" }]);
});

test("invalid PNG chunk traversal is refused before packet export", async () => {
  const src = "https://example.com/a.png";
  installDocument([image(src, src)]);
  const png = makePng(1, 1);
  png.writeUInt32BE(0xffffffff, 33);
  globalThis.fetch = async () => response(png);
  const result = await collectInPage([{ index: 0, src_sha256: await sha256Hex(src) }], "https://example.com");
  assert.equal(result.entries.length, 0);
  assert.deepEqual(result.failures, [{ index: 0, code: "source_decode" }]);
});
