import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MarkdownPreview } from "./MarkdownPreview";
import { TEXT_PREVIEW_RENDER_MAX_CHARS } from "./textPreviewPolicy";
import { installMarkdownTestWorker, MarkdownTestWorker } from '../../test/markdownWorker';

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
  beforeEach(() => { openMock.mockClear(); installMarkdownTestWorker(); });

  it("renders an article without loading embedded remote resources", async () => {
    const { container } = render(
      <MarkdownPreview data={bytes("# Heading\n\n![private](https://example.com/track.png)")} />,
    );

    expect(await screen.findByRole("heading", { name: "Heading" })).toBeInTheDocument();
    expect(container.querySelector("img")).toBeNull();
    expect(container.innerHTML).not.toContain("track.png");
  });

  it("opens only explicit http and https links through the system shell", async () => {
    render(
      <MarkdownPreview
        data={bytes("[safe](https://example.com/read) [relative](/secret) [bad](javascript:alert(1))")}
      />,
    );

    const safe = await screen.findByRole("link", { name: "safe" });
    expect(safe).toHaveAttribute("href", "https://example.com/read");
    expect(screen.getByText("relative").closest("a")).not.toHaveAttribute("href");
    expect(screen.getByText("bad").closest("a")).not.toHaveAttribute("href");

    fireEvent.click(safe);
    expect(openMock).toHaveBeenCalledWith("https://example.com/read");
  });

  it("bounds Markdown parsing for oversized sources and reports truncation", async () => {
    const source = `# Heading\n\n${"paragraph ".repeat(TEXT_PREVIEW_RENDER_MAX_CHARS)}`;
    render(
      <MarkdownPreview
        data={bytes(source)}
        t={(key) => key}
      />,
    );

    expect(await screen.findByTestId("preview-truncated")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Heading" })).toBeInTheDocument();
  });

  it('discards stale worker replies and does not parse again for zoom', async () => {
    MarkdownTestWorker.autoReply = false;
    const view = render(<MarkdownPreview data={bytes('# Old')} />);
    const old = MarkdownTestWorker.instances[0];
    view.rerender(<MarkdownPreview data={bytes('# New')} />);
    const current = MarkdownTestWorker.instances[1];
    await act(async () => current.reply());
    await act(async () => old.reply());
    expect(screen.getByRole('heading', { name: 'New' })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Old' })).toBeNull();
    fireEvent.wheel(view.container.querySelector('.preview-markdown')!, { ctrlKey: true, deltaY: -100 });
    expect(MarkdownTestWorker.instances).toHaveLength(2);
    view.unmount(); expect(old.terminated && current.terminated).toBe(true);
  });

  it('falls back to readable text after a worker failure', async () => {
    MarkdownTestWorker.autoReply = false;
    render(<MarkdownPreview data={bytes('# Keep the source')} />);
    await act(async () => MarkdownTestWorker.instances[0].onerror?.());
    await waitFor(() => expect(screen.getByText('# Keep the source')).toBeInTheDocument());
  });
});
