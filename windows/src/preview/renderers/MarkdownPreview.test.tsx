import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MarkdownPreview } from "./MarkdownPreview";
import { TEXT_PREVIEW_RENDER_MAX_CHARS } from "./textPreviewPolicy";

// `vi.mock` is hoisted by Vitest.  Keep the spy in a hoisted factory so the
// module under test never observes an uninitialised binding during import.
const { openMock } = vi.hoisted(() => ({
  openMock: vi.fn<() => Promise<void>>(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/plugin-shell", () => ({ open: openMock }));

function bytes(markdown: string): Uint8Array {
  return new TextEncoder().encode(markdown);
}

describe("MarkdownPreview", () => {
  beforeEach(() => openMock.mockClear());

  it("renders an article without loading embedded remote resources", () => {
    const { container } = render(
      <MarkdownPreview data={bytes("# Heading\n\n![private](https://example.com/track.png)")} />,
    );

    expect(screen.getByRole("heading", { name: "Heading" })).toBeInTheDocument();
    expect(container.querySelector("img")).toBeNull();
    expect(container.innerHTML).not.toContain("track.png");
  });

  it("opens only explicit http and https links through the system shell", () => {
    render(
      <MarkdownPreview
        data={bytes("[safe](https://example.com/read) [relative](/secret) [bad](javascript:alert(1))")}
      />,
    );

    const safe = screen.getByRole("link", { name: "safe" });
    expect(safe).toHaveAttribute("href", "https://example.com/read");
    expect(screen.getByText("relative").closest("a")).not.toHaveAttribute("href");
    expect(screen.getByText("bad").closest("a")).not.toHaveAttribute("href");

    fireEvent.click(safe);
    expect(openMock).toHaveBeenCalledWith("https://example.com/read");
  });

  it("bounds Markdown parsing for oversized sources and reports truncation", () => {
    const source = `# Heading\n\n${"paragraph ".repeat(TEXT_PREVIEW_RENDER_MAX_CHARS)}`;
    render(
      <MarkdownPreview
        data={bytes(source)}
        t={(key) => key}
      />,
    );

    expect(screen.getByTestId("preview-truncated")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Heading" })).toBeInTheDocument();
  });
});
