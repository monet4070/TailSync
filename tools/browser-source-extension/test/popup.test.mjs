import test from "node:test";
import assert from "node:assert/strict";
import { validateTicket } from "../ticket.js";

const makeTicket = () => ({ kind: "tailsync-source-ticket", version: 1, grant_id: "ab".repeat(32), origin: "https://example.com", expires_at: Math.floor(Date.now() / 1000) + 200, budget_bytes: 1024, nodes: [{ index: 0, src_sha256: "cd".repeat(32) }] });

test("strict ticket scope refuses credentials, extra fields, expiry, duplicates and inflated budgets", () => {
  assert.deepEqual(validateTicket(makeTicket()), makeTicket());
  for (const change of [
    { consent: true }, { grant_id: "x" }, { origin: "https://user:pass@example.com" },
    { origin: "https://example.com/path" }, { origin: "blob:https://example.com/id" },
    { expires_at: Math.floor(Date.now() / 1000) - 1 }, { expires_at: Number.NaN },
    { expires_at: Math.floor(Date.now() / 1000) + 600 }, { budget_bytes: 32 * 1024 * 1024 },
    { nodes: [] }, { nodes: [makeTicket().nodes[0], makeTicket().nodes[0]] },
    { nodes: [{ index: -1, src_sha256: "cd".repeat(32) }] },
    { nodes: [{ index: 0, src_sha256: "cd".repeat(32), url: "secret" }] },
  ]) assert.throws(() => validateTicket({ ...makeTicket(), ...change }));
});

test("popup bounds ticket reads, rechecks expiry and never downloads an empty packet", async () => {
  const elements = new Map(["ticket", "scope", "collect", "status"].map(id => [id, { addEventListener(event, handler) { this[event] = handler; }, dataset: {}, files: [] }]));
  let downloads = 0;
  let scripts = 0;
  globalThis.document = {
    getElementById: id => elements.get(id),
    createElement: () => ({ click() { downloads += 1; }, remove() {} }),
    body: { appendChild() {} },
  };
  globalThis.chrome = {
    tabs: { query: async () => [{ id: 1 }] },
    scripting: { executeScript: async request => {
      scripts += 1;
      return [{ result: request.args ? { entries: [], failures: [{ index: 0, code: "source_node_missing" }] } : "https://example.com" }];
    } },
  };
  await import("../popup.js");
  const input = elements.get("ticket");
  const button = elements.get("collect");
  let reads = 0;
  input.files = [{ size: 65 * 1024, text: async () => { reads += 1; return "{}"; } }];
  await input.change();
  assert.equal(reads, 0);
  assert.equal(button.disabled, true);
  const ticket = makeTicket();
  input.files = [{ size: 512, text: async () => JSON.stringify(ticket) }];
  await input.change();
  await button.click();
  assert.equal(downloads, 0);
  assert.match(elements.get("status").textContent, /nothing was downloaded/);
  const realNow = Date.now;
  Date.now = () => ticket.expires_at * 1000 + 1;
  try {
    scripts = 0;
    await button.click();
    assert.equal(scripts, 0);
    assert.equal(downloads, 0);
  } finally {
    Date.now = realNow;
  }
});
