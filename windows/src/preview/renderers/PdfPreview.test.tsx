import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PdfPreview } from "./PdfPreview";

const pdfMocks = vi.hoisted(() => ({
  getDocument: vi.fn(),
  textLayerCancel: vi.fn(),
  textLayerRender: vi.fn(() => Promise.resolve()),
}));

vi.mock("pdfjs-dist", () => ({
  GlobalWorkerOptions: {},
  TextLayer: class {
    cancel = pdfMocks.textLayerCancel;
    render = pdfMocks.textLayerRender;
  },
  getDocument: pdfMocks.getDocument,
}));

vi.mock("pdfjs-dist/build/pdf.worker.min.mjs?url", () => ({ default: "mock-pdf-worker" }));

function pageProxy() {
  return {
    cleanup: vi.fn(),
    getTextContent: vi.fn(() => Promise.resolve({ items: [], styles: {} })),
    getViewport: vi.fn(({ scale }: { scale: number }) => ({ height: 120, scale, width: 80 })),
    render: vi.fn(() => ({ cancel: vi.fn(), promise: Promise.resolve() })),
  };
}

describe("PdfPreview", () => {
  beforeEach(() => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({} as never);
  });

  it("cleans the previous PDF page when navigation replaces it", async () => {
    const firstPage = pageProxy();
    const secondPage = pageProxy();
    const documentProxy = {
      destroy: vi.fn(() => Promise.resolve()),
      getPage: vi.fn((pageNumber: number) => Promise.resolve(pageNumber === 1 ? firstPage : secondPage)),
      numPages: 2,
    };
    pdfMocks.getDocument.mockReturnValue({
      destroy: vi.fn(() => Promise.resolve()),
      promise: Promise.resolve(documentProxy),
    });

    render(<PdfPreview data={new Uint8Array([1, 2, 3])} t={(key) => key} onCorrupt={vi.fn()} />);

    await waitFor(() => expect(documentProxy.getPage).toHaveBeenCalledWith(1));
    fireEvent.click(screen.getByTitle("history.next"));
    await waitFor(() => expect(documentProxy.getPage).toHaveBeenCalledWith(2));
    await waitFor(() => expect(firstPage.cleanup).toHaveBeenCalledTimes(1));
  });

  it("reuses a page proxy across zoom and cleans it only on unmount", async () => {
    const firstPage = pageProxy();
    const documentProxy = {
      destroy: vi.fn(() => Promise.resolve()),
      getPage: vi.fn(() => Promise.resolve(firstPage)),
      numPages: 1,
    };
    pdfMocks.getDocument.mockReturnValue({
      destroy: vi.fn(() => Promise.resolve()),
      promise: Promise.resolve(documentProxy),
    });

    const view = render(
      <PdfPreview data={new Uint8Array([4, 5, 6])} t={(key) => key} onCorrupt={vi.fn()} />,
    );

    await waitFor(() => expect(documentProxy.getPage).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByTitle("history.preview.zoomIn"));
    await waitFor(() => expect(firstPage.render).toHaveBeenCalledTimes(2));
    expect(documentProxy.getPage).toHaveBeenCalledTimes(1);
    expect(firstPage.cleanup).not.toHaveBeenCalled();

    view.unmount();
    expect(firstPage.cleanup).toHaveBeenCalledTimes(1);
  });
});
