import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PreviewPayload } from "../../utils/historyPreview";
import { ImagePreview } from "./ImagePreview";

const payload: PreviewPayload = {
  entry_id: 1,
  kind: "file",
  name: "image.png",
  size_bytes: 4,
  width: 100,
  height: 80,
  batch: null,
  data: new Uint8Array([1, 2, 3, 4]),
};

describe("ImagePreview", () => {
  beforeEach(() => {
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:test-image");
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
  });

  afterEach(() => vi.restoreAllMocks());

  it("supports modifier-wheel zoom and keeps fit sizing while panning", () => {
    const { container } = render(<ImagePreview payload={payload} t={(key) => key} onCorrupt={vi.fn()} />);
    const stage = container.querySelector<HTMLElement>(".preview-image-stage")!;
    const fitButton = screen.getByTitle("history.preview.fit");
    expect(fitButton).toHaveClass("is-active");

    fireEvent.wheel(stage, { ctrlKey: true, deltaY: -100 });
    expect(screen.getByText("110%")).toBeInTheDocument();

    fireEvent.click(fitButton);
    Object.defineProperty(stage, "setPointerCapture", { configurable: true, value: vi.fn() });
    fireEvent.pointerDown(stage, { button: 0, pointerId: 1, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(stage, { pointerId: 1, clientX: 35, clientY: 30 });

    expect(fitButton).toHaveClass("is-active");
    expect(container.querySelector(".preview-image-transform.is-fit")).toBeNull();
  });
});
