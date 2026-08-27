import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { PreviewPayload } from "../../utils/historyPreview";
import { SvgPreview } from "./SvgPreview";

const t = (key: string) => key;

function payload(source: string): PreviewPayload {
  return {
    entry_id: 42,
    kind: "file",
    name: "diagram.svg",
    size_bytes: new TextEncoder().encode(source).length,
    width: null,
    height: null,
    batch: null,
    data: new TextEncoder().encode(source),
  };
}

describe("SvgPreview", () => {
  it("renders SVG in a sandboxed iframe and exposes a loading state", async () => {
    const onCorrupt = vi.fn();
    render(
      <SvgPreview
        payload={payload(`<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40"><rect width="80" height="40" fill="red"/></svg>`)}
        t={t}
        onCorrupt={onCorrupt}
      />,
    );

    const frame = document.querySelector("iframe");
    expect(frame).not.toBeNull();
    if (!frame) return;
    expect(frame).toHaveAttribute("sandbox", "");
    expect(frame).toHaveAttribute("referrerpolicy", "no-referrer");
    expect(frame).toHaveAttribute("srcdoc");
    expect(screen.getByRole("status")).toHaveTextContent("history.preview.svgRendering");

    fireEvent.load(frame);
    await waitFor(() => expect(screen.queryByText("history.preview.svgRendering")).toBeNull());
    expect(onCorrupt).not.toHaveBeenCalled();
  });

  it("falls back to source controls when the isolated visual render times out", async () => {
    vi.useFakeTimers();
    try {
      render(
        <SvgPreview
          payload={payload(`<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40"><rect width="80" height="40"/></svg>`)}
          t={t}
          onCorrupt={vi.fn()}
        />,
      );

      expect(screen.getByText("history.preview.svgRendering")).toBeInTheDocument();
      await act(async () => {
        vi.advanceTimersByTime(4_000);
      });
      expect(screen.getByText("history.preview.svgFallback")).toBeInTheDocument();
      expect(screen.queryByText("history.preview.svgRendering")).toBeNull();
      expect(screen.getByTestId("preview-text")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows active documents as source without creating an iframe", () => {
    render(
      <SvgPreview
        payload={payload(`<svg><script>alert(1)</script></svg>`)}
        t={t}
        onCorrupt={vi.fn()}
      />,
    );

    expect(document.querySelector("iframe")).toBeNull();
    expect(screen.getByTestId("preview-text")).toBeInTheDocument();
    expect(screen.getByText("history.preview.svgBlockedContent")).toBeInTheDocument();
  });

  it("keeps visual-only controls out of the source mode", () => {
    render(
      <SvgPreview
        payload={payload(`<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40"/>`)}
        t={t}
        onCorrupt={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "history.preview.svgSource" }));
    expect(screen.queryByRole("button", { name: "history.preview.zoomIn" })).toBeNull();
    expect(screen.getByTestId("preview-text")).toBeInTheDocument();
  });

  it("rejects invalid UTF-8 instead of rendering replacement characters", () => {
    const onCorrupt = vi.fn();
    render(
      <SvgPreview
        payload={{
          ...payload("<svg></svg>"),
          data: new Uint8Array([60, 115, 118, 103, 62, 0xff, 60, 47, 115, 118, 103, 62]),
        }}
        t={t}
        onCorrupt={onCorrupt}
      />,
    );

    expect(onCorrupt).toHaveBeenCalled();
    expect(document.querySelector("iframe")).toBeNull();
  });

  it("requires explicit approval and enumerates exact external origins", () => {
    render(
      <SvgPreview
        payload={payload(`<svg><image href="https://cdn.example.com:8443/logo.png"/></svg>`)}
        t={t}
        onCorrupt={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "history.preview.svgTrustExternal" }));
    expect(screen.getByRole("dialog")).toHaveTextContent("cdn.example.com:8443");
    expect(screen.getByRole("dialog")).toHaveTextContent("history.preview.svgTrustAlertMessage");

    fireEvent.click(screen.getByRole("button", { name: "history.preview.svgTrustAlertAllow" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("button", { name: "history.preview.svgTrustedExternal" })).toBeInTheDocument();
    expect(document.querySelector("iframe")).toHaveAttribute("srcdoc", expect.stringContaining("https://cdn.example.com:8443"));
  });

  it("does not carry external trust into a replacement entry", () => {
    const first = payload(`<svg><image href="https://first.example.com/logo.png"/></svg>`);
    const { rerender } = render(
      <SvgPreview payload={first} t={t} onCorrupt={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "history.preview.svgTrustExternal" }));
    fireEvent.click(screen.getByRole("button", { name: "history.preview.svgTrustAlertAllow" }));
    expect(screen.getByRole("button", { name: "history.preview.svgTrustedExternal" })).toBeInTheDocument();

    const second = payload(`<svg><image href="https://second.example.com/logo.png"/></svg>`);
    rerender(<SvgPreview payload={{ ...second, entry_id: 43 }} t={t} onCorrupt={vi.fn()} />);
    const frame = document.querySelector("iframe");
    expect(frame).toHaveAttribute("srcdoc", expect.stringContaining("img-src data:"));
    expect(frame).not.toHaveAttribute("srcdoc", expect.stringContaining("img-src data: https://second.example.com"));
  });

  it("shows refused targets and never offers approval for a mixed unsafe document", () => {
    render(
      <SvgPreview
        payload={payload(`<svg><image href="https://cdn.example.com/logo.png"/><image href="http://127.0.0.1/private.png"/></svg>`)}
        t={t}
        onCorrupt={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "history.preview.svgExternalBlocked" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("127.0.0.1");
    expect(dialog).toHaveTextContent("cdn.example.com");
    expect(screen.queryByRole("button", { name: "history.preview.svgTrustAlertAllow" })).toBeNull();
  });
});
