import { beforeEach, describe, expect, it, vi } from "vitest";
import { getDocumentPreview } from "../tailsyncClient";
import { decodeDocumentFrame } from "./documentPreview";

const bridge = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("../diagnostics/localIpcMetrics", () => ({ invoke: bridge.invoke }));
const request = { entry_id: 7, owner: "history" as const, generation: 3, request_id: "current" };
function frame(): Uint8Array {
  const model = new TextEncoder().encode(JSON.stringify({ version: 1, blocks: [], assets: [], partial: [], styles_partial: false }));
  const header = new TextEncoder().encode(JSON.stringify({ ...request, kind: "document", name: "document", stamp: `dc1:${"0a".repeat(32)}`, model_bytes: model.length, size_bytes: model.length }));
  const bytes = new Uint8Array(9 + header.length + model.length);
  bytes.set(new TextEncoder().encode("TSDC")); bytes[4] = 1;
  new DataView(bytes.buffer).setUint32(5, header.length, true);
  bytes.set(header, 9); bytes.set(model, 9 + header.length);
  return bytes;
}
describe("document production client lifecycle", () => {
  beforeEach(() => bridge.invoke.mockReset());
  it("rejects an owner, generation, entry or request mismatch", () => {
    for (const changed of [{ ...request, owner: "favorites" as const }, { ...request, generation: 4 }, { ...request, entry_id: 8 }, { ...request, request_id: "old" }]) {
      expect(() => decodeDocumentFrame(frame(), changed)).toThrow();
    }
  });
  it("revalidates backend identity before returning installable data", async () => {
    bridge.invoke.mockResolvedValueOnce(frame()).mockResolvedValueOnce(true);
    const loaded = await getDocumentPreview(request);
    expect(loaded.metadata.owner).toBe("history");
    expect(bridge.invoke).toHaveBeenNthCalledWith(2, "validate_document_preview", { request, stamp: loaded.metadata.stamp });
  });
  it("rejects a stale database identity instead of returning the frame", async () => {
    bridge.invoke.mockResolvedValueOnce(frame()).mockResolvedValueOnce(false).mockResolvedValue(undefined);
    await expect(getDocumentPreview(request)).rejects.toThrow("stale");
    expect(bridge.invoke).toHaveBeenCalledWith("cancel_document_preview", { request });
  });
  it("cancel and late delivery never return installable data", async () => {
    let resolve: (bytes: Uint8Array) => void = () => undefined;
    bridge.invoke.mockImplementation((command: string) => command === "get_document_preview" ? new Promise<Uint8Array>((ready) => { resolve = ready; }) : Promise.resolve(undefined));
    const cancellation = new AbortController();
    const loading = getDocumentPreview(request, cancellation.signal);
    await vi.waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("get_document_preview", { request }));
    cancellation.abort(); resolve(frame());
    await expect(loading).rejects.toThrow("cancelled");
    expect(bridge.invoke).toHaveBeenCalledWith("cancel_document_preview", { request });
    expect(bridge.invoke).not.toHaveBeenCalledWith("validate_document_preview", expect.anything());
  });
  it("rejects cancellation while backend installation validation is pending", async () => {
    let validated: (current: boolean) => void = () => undefined;
    bridge.invoke.mockImplementation((command: string) => {
      if (command === "get_document_preview") return Promise.resolve(frame());
      if (command === "validate_document_preview") return new Promise<boolean>((ready) => { validated = ready; });
      return Promise.resolve(undefined);
    });
    const cancellation = new AbortController();
    const loading = getDocumentPreview(request, cancellation.signal);
    await vi.waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("validate_document_preview", expect.anything()));
    cancellation.abort(); validated(true);
    await expect(loading).rejects.toThrow("cancelled");
    expect(bridge.invoke).toHaveBeenCalledWith("cancel_document_preview", { request });
  });
});
