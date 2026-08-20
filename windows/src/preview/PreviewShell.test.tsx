import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { PreviewPayload } from "../utils/historyPreview";
import { PreviewShell } from "./PreviewShell";

vi.mock("./PreviewContent", () => ({
  PreviewContent: () => <div data-testid="preview-content" />,
}));

const payload: PreviewPayload = {
  entry_id: 7,
  kind: "text",
  name: "note.txt",
  size_bytes: 4,
  width: null,
  height: null,
  batch: null,
  data: new TextEncoder().encode("note"),
};

const t = (key: string) => key;

describe("PreviewShell", () => {
  it("shows a clear restore acknowledgement and omits unused window controls", async () => {
    const onRestore = vi.fn<() => Promise<void>>(() => Promise.resolve());
    render(
      <PreviewShell
        payload={payload}
        loading={false}
        failure={null}
        onRetry={vi.fn()}
        onCorrupt={vi.fn()}
        onClose={vi.fn()}
        onRestore={onRestore}
        onPrevious={null}
        onNext={null}
        t={t}
      />,
    );

    expect(screen.queryByLabelText("history.preview.minimize")).toBeNull();
    expect(screen.queryByLabelText("history.preview.maximize")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "history.restoreEntry" }));

    await waitFor(() => expect(onRestore).toHaveBeenCalledTimes(1));
    expect(await screen.findByRole("status")).toHaveTextContent("history.preview.restored");
  });

  it("keeps restore failures visible and actionable", async () => {
    render(
      <PreviewShell
        payload={payload}
        loading={false}
        failure={null}
        onRetry={vi.fn()}
        onCorrupt={vi.fn()}
        onClose={vi.fn()}
        onRestore={() => Promise.reject(new Error("clipboard unavailable"))}
        onPrevious={null}
        onNext={null}
        t={t}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "history.restoreEntry" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("history.preview.restoreError");
  });
});
