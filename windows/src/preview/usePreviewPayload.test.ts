import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { usePreviewPayload } from "./usePreviewPayload";

const getPreview = vi.hoisted(() => vi.fn());
vi.mock("../tailsyncClient", () => ({ getPreview }));
beforeEach(() => localStorage.clear());

it("rejects stale demo IDs without requesting a real history payload", async () => {
  const target = { entryId: 8_000_000_001, batchId: null };
  const hook = renderHook(() => usePreviewPayload(target));
  await waitFor(() => expect(hook.result.current.state.status).toBe("error"));
  expect(getPreview).not.toHaveBeenCalled();
});
