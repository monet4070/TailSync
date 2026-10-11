import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import en from "../../i18n/en.json";
import zhCN from "../../i18n/zh-CN.json";
import {
  buildPngAsset,
  concatAssets,
  documentAsset,
  documentImageBlock,
  documentParagraph,
  documentRun,
  documentTableBlock,
  encodeDocumentFrame,
  type BuiltDocumentAsset,
  type DocumentFrameControl,
} from "../../test/documentFrames";
import { decodeDocumentFrame, type DecodedDocumentFrame } from "../../utils/documentPreview";
import type * as Contract from "../../types/localContracts.generated";
import { planDocumentImages, prepareDocumentAssets, type PreparedDocumentAssets } from "../documentAssets";
import { DocumentPreview } from "./DocumentPreview";
import { headingLevel, readableTextColor, safeDocumentLink } from "../documentPresentation";

const control: DocumentFrameControl = { entry_id: 7, owner: "history", generation: 23, request_id: "render-test" };
const request: import("../../types/localContracts.generated").DocumentRequest = {
  entry_id: 7, owner: "history", generation: 23, request_id: "render-test",
};
const translate = (key: string) => key;
const english = (key: string) => (en as Record<string, string>)[key] ?? key;

async function prepare(document: Record<string, unknown>, assets: BuiltDocumentAsset[]): Promise<PreparedDocumentAssets> {
  const frame = decodeDocumentFrame(encodeDocumentFrame(control, document, concatAssets(assets)), request);
  return prepareDocumentAssets(frame);
}

function paragraph(runs: Array<Record<string, unknown>>, extra: Record<string, unknown> = {}): Record<string, unknown> {
  return { kind: "paragraph", paragraph: { runs, ...extra } };
}

function stubPrepared(document: Record<string, unknown>): PreparedDocumentAssets {
  return {
    frame: {
      metadata: {
        request_id: "stub",
        entry_id: 7,
        kind: "document",
        name: "document",
        stamp: `dc1:${"0a".repeat(32)}`,
        owner: "history",
        generation: 23,
        model_bytes: 1,
        size_bytes: 1,
        batch: null,
      },
      document,
      assets: new Uint8Array(),
    },
    urls: new Map<number, string>(),
    retainedBytes: 0,
    imagePlan: { placements: new Map(), pixelBudgetExceeded: false },
    release: () => undefined,
  } as unknown as PreparedDocumentAssets;
}

describe("DocumentPreview", () => {
  beforeEach(() => {
    let counter = 0;
    vi.spyOn(URL, "createObjectURL").mockImplementation(() => `blob:asset-${counter++}`);
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
  });

  afterEach(() => vi.restoreAllMocks());

  it("renders Chinese/emoji text, basic run styles, ordered images, empty table cells and literal markup", async () => {
    const red = documentAsset(0, 0, buildPngAsset(4, 4, 0x11));
    const blue = documentAsset(1, red.bytes.byteLength, buildPngAsset(4, 4, 0x22));
    const prepared = await prepare({
      version: 1,
      blocks: [
        documentParagraph([documentRun("中文😀 ", { bold: true, color: { red: 255, green: 0, blue: 0 } })]),
        documentImageBlock(0, 0),
        documentParagraph([documentRun("两图之间🧪")]),
        documentImageBlock(1, 1),
        documentTableBlock([[{ header: true, runs: [documentRun("A")] }, { header: false, runs: [] }, { header: false, runs: [documentRun("B")] }]]),
        documentParagraph([documentRun("<script>never execute</script> https://bad.invalid file:///private/secret")]),
      ],
      assets: [red.asset, blue.asset],
      partial: [],
      styles_partial: false,
      first_image: 0,
    }, [red, blue]);

    const { container } = render(<DocumentPreview prepared={prepared} translate={translate} />);

    const text = screen.getByTestId("preview-document").textContent ?? "";
    expect(text).toContain("中文😀 ");
    expect(text).toContain("两图之间🧪");
    expect(text).toContain("<script>never execute</script> https://bad.invalid file:///private/secret");
    expect(container.querySelectorAll("script")).toHaveLength(0);
    expect(container.querySelectorAll("a, iframe")).toHaveLength(0);

    const boldRun = container.querySelector(".preview-document-paragraph span");
    expect(boldRun?.textContent).toBe("中文😀 ");
    expect(boldRun).toHaveStyle({ fontWeight: "700", color: "rgb(255, 0, 0)" });

    const images = screen.getAllByTestId("preview-document-image");
    expect(images).toHaveLength(2);
    expect(images[0].getAttribute("src")).toBe("blob:asset-0");
    expect(images[1].getAttribute("src")).toBe("blob:asset-1");
    expect(images.map((image) => image.getAttribute("data-ordinal"))).toEqual(["0", "1"]);

    const cells = screen.getAllByTestId("preview-document-cell");
    expect(cells).toHaveLength(3);
    expect(cells[0].textContent).toBe("A");
    expect(cells[1].textContent).toBe("");
    expect(cells[2].textContent).toBe("B");
    expect(screen.getAllByRole("columnheader")).toHaveLength(1);
  });

  it("keeps an unavailable first image as a labelled placeholder and never promotes the second", async () => {
    const blue = documentAsset(0, 0, buildPngAsset(2, 2, 0x44));
    const prepared = await prepare({
      version: 1,
      blocks: [documentImageBlock(null, 0, "external_reference"), documentImageBlock(0, 1)],
      assets: [blue.asset],
      partial: [{ code: "external_reference", detail: "first image unavailable" }],
      styles_partial: true,
      first_image: null,
    }, [blue]);

    render(<DocumentPreview prepared={prepared} translate={translate} />);

    const placeholders = screen.getAllByTestId("preview-document-image-placeholder");
    expect(placeholders).toHaveLength(1);
    expect(placeholders[0].getAttribute("data-availability")).toBe("external_reference");
    expect(placeholders[0].getAttribute("data-ordinal")).toBe("0");
    expect(placeholders[0].getAttribute("aria-label")).toContain("history.preview.imageUnavailable");
    const images = screen.getAllByTestId("preview-document-image");
    expect(images).toHaveLength(1);
    expect(images[0].getAttribute("data-ordinal")).toBe("1");

    expect(screen.getByRole("note", { name: "history.preview.partialTitle" })).toBeInTheDocument();
    expect(screen.getByText("first image unavailable")).toBeInTheDocument();
    expect(screen.queryByText("history.preview.partial.nativeUnavailable")).toBeNull();
  });

  it("labels a missing saved image as missing and warns without a partial reason", async () => {
    const prepared = await prepare({
      version: 1,
      blocks: [documentImageBlock(null, 0, "missing_resource")],
      assets: [],
      partial: [],
      styles_partial: false,
      first_image: null,
    }, []);

    render(<DocumentPreview prepared={prepared} translate={translate} />);

    const placeholder = screen.getByTestId("preview-document-image-placeholder");
    expect(placeholder.getAttribute("data-availability")).toBe("missing_resource");
    expect(placeholder.getAttribute("aria-label")).not.toContain("history.preview.imageAvailability.available");
    expect(placeholder.getAttribute("aria-label")).toContain("history.preview.imageAvailability.missing_resource");
    expect(screen.getByRole("note", { name: "history.preview.partialTitle" })).toBeInTheDocument();
    expect(screen.getByText("history.preview.partial.missingImage")).toBeInTheDocument();
    expect(screen.getByText("missing_image_source")).toBeInTheDocument();
  });

  it("never labels a placeholder as available when the saved asset cannot be rendered", async () => {
    const frame = decodeDocumentFrame(encodeDocumentFrame(control, {
      version: 1, blocks: [documentImageBlock(null, 0, "missing_resource")], assets: [],
      partial: [], styles_partial: false, first_image: null,
    }, new Uint8Array()), request);
    const patched: DecodedDocumentFrame = {
      ...frame,
      document: {
        ...frame.document,
        blocks: [{ kind: "image", image: { availability: "available", ordinal: 0 } }],
      },
    };

    const prepared = await prepareDocumentAssets(patched);
    render(<DocumentPreview prepared={prepared} translate={translate} />);

    const placeholder = screen.getByTestId("preview-document-image-placeholder");
    expect(placeholder.getAttribute("data-availability")).toBe("missing_resource");
    expect(placeholder.getAttribute("aria-label")).not.toContain("history.preview.imageAvailability.available");
    expect(screen.getByRole("note", { name: "history.preview.partialTitle" })).toBeInTheDocument();
    expect(screen.getByText("missing_image_source")).toBeInTheDocument();
  });

  it("does not warn about a saved image that renders", async () => {
    const red = documentAsset(0, 0, buildPngAsset(4, 4, 0xaa));
    const prepared = await prepare({
      version: 1, blocks: [documentImageBlock(0, 0)], assets: [red.asset],
      partial: [], styles_partial: false, first_image: 0,
    }, [red]);

    const { container } = render(<DocumentPreview prepared={prepared} translate={translate} />);

    expect(screen.getByTestId("preview-document-image")).toBeInTheDocument();
    expect(container.querySelector(".preview-document-notices")).toBeNull();
    expect(screen.queryByTestId("preview-document-diagnostics")).toBeNull();
  });

  it("warns when a saved image has metadata but no renderable URL", async () => {
    const red = documentAsset(0, 0, buildPngAsset(4, 4, 0x99));
    const frame = decodeDocumentFrame(encodeDocumentFrame(control, {
      version: 1, blocks: [documentImageBlock(0, 0)], assets: [red.asset],
      partial: [], styles_partial: false, first_image: 0,
    }, concatAssets([red])), request);
    const imagePlan = planDocumentImages(frame.document, new Map());
    const prepared: PreparedDocumentAssets = {
      frame, urls: new Map(), retainedBytes: 0, imagePlan, release: () => undefined,
    };

    render(<DocumentPreview prepared={prepared} translate={translate} />);

    expect(screen.getByTestId("preview-document-image-placeholder")).toBeInTheDocument();
    expect(screen.queryByTestId("preview-document-image")).toBeNull();
    expect(screen.getByText("unsupported_image")).toBeInTheDocument();
    expect(screen.getByText("history.preview.partial.unsupportedImage")).toBeInTheDocument();
  });

  it("separates an unsaved image from one that cannot be displayed", async () => {
    const red = documentAsset(0, 0, buildPngAsset(4, 4, 0xcc));
    const prepared = await prepare({
      version: 1,
      blocks: [documentImageBlock(null, 0, "missing_resource"), documentImageBlock(0, 1)],
      assets: [red.asset], partial: [], styles_partial: false, first_image: null,
    }, [red]);

    render(<DocumentPreview prepared={prepared} translate={translate} />);

    const [unsaved] = screen.getAllByTestId("preview-document-image-placeholder");
    expect(unsaved.getAttribute("aria-label")).toContain("history.preview.imageUnavailable");
    expect(unsaved.getAttribute("aria-label")).not.toContain("history.preview.imageCannotDisplay");

    fireEvent.error(screen.getByTestId("preview-document-image"));

    const placeholders = screen.getAllByTestId("preview-document-image-placeholder");
    expect(placeholders).toHaveLength(2);
    expect(placeholders[1].getAttribute("aria-label")).toContain("history.preview.imageCannotDisplay");
    expect(placeholders[1].getAttribute("aria-label")).not.toContain("history.preview.imageUnavailable");
  });

  it("reports only the pixel budget warning for budget-limited placeholders", async () => {
    const large = documentAsset(0, 0, buildPngAsset(2048, 2048, 0xbb));
    const prepared = await prepare({
      version: 1, blocks: Array.from({ length: 5 }, (_, ordinal) => documentImageBlock(0, ordinal)),
      assets: [large.asset], partial: [], styles_partial: false, first_image: 0,
    }, [large]);

    const { container } = render(<DocumentPreview prepared={prepared} translate={translate} />);

    const codes = Array.from(container.querySelectorAll("li[data-partial-code]"))
      .map((item) => item.getAttribute("data-partial-code"));
    expect(codes).toEqual(["pixel_budget"]);
    const placeholders = screen.getAllByTestId("preview-document-image-placeholder");
    expect(placeholders).toHaveLength(2);
    expect(placeholders[0].getAttribute("aria-label")).toContain("history.preview.imageCannotDisplay");
    expect(placeholders[0].getAttribute("aria-label")).not.toContain("history.preview.imageUnavailable");
  });

  it("does not carry a decode failure warning into a new entry or stamp", async () => {
    vi.spyOn(URL, "createObjectURL").mockImplementation(() => "blob:shared-asset");
    const first = documentAsset(0, 0, buildPngAsset(4, 4, 0x66));
    const prepared = await prepare({
      version: 1, blocks: [documentImageBlock(0, 0)], assets: [first.asset],
      partial: [], styles_partial: false, first_image: 0,
    }, [first]);
    const view = render(<DocumentPreview prepared={prepared} translate={translate} />);
    fireEvent.error(screen.getByTestId("preview-document-image"));
    expect(screen.getByText("unsupported_image")).toBeInTheDocument();

    const second = documentAsset(0, 0, buildPngAsset(4, 4, 0x77));
    const nextControl: DocumentFrameControl = {
      ...control, entry_id: 8, generation: 24, request_id: "render-test-2", stamp: `dc1:${"1b".repeat(32)}`,
    };
    const nextFrame = decodeDocumentFrame(encodeDocumentFrame(nextControl, {
      version: 1, blocks: [documentImageBlock(0, 0)], assets: [second.asset],
      partial: [], styles_partial: false, first_image: 0,
    }, concatAssets([second])), {
      entry_id: 8, owner: "history", generation: 24, request_id: "render-test-2",
    });
    const nextPrepared = await prepareDocumentAssets(nextFrame);

    view.rerender(<DocumentPreview prepared={nextPrepared} translate={translate} />);

    expect(screen.getByTestId("preview-document-image")).toBeInTheDocument();
    expect(screen.queryByText("unsupported_image")).toBeNull();
    expect(view.container.querySelector(".preview-document-notices")).toBeNull();
  });

  it("withdraws a decode failure warning when the image URL recovers", async () => {
    const red = documentAsset(0, 0, buildPngAsset(4, 4, 0x88));
    const frame = decodeDocumentFrame(encodeDocumentFrame(control, {
      version: 1, blocks: [documentImageBlock(0, 0)], assets: [red.asset],
      partial: [], styles_partial: false, first_image: 0,
    }, concatAssets([red])), request);
    const first = await prepareDocumentAssets(frame);
    const view = render(<DocumentPreview prepared={first} translate={translate} />);
    fireEvent.error(screen.getByTestId("preview-document-image"));
    expect(screen.getByText("unsupported_image")).toBeInTheDocument();

    const recovered = await prepareDocumentAssets(frame);
    view.rerender(<DocumentPreview prepared={recovered} translate={translate} />);

    expect(screen.getByTestId("preview-document-image")).toBeInTheDocument();
    expect(screen.queryByText("unsupported_image")).toBeNull();
    expect(view.container.querySelector(".preview-document-notices")).toBeNull();
  });

  it("keeps ordinary style simplification out of the warning banner", async () => {
    const prepared = await prepare({
      version: 1,
      blocks: [documentParagraph([documentRun("styled")])],
      assets: [],
      partial: [],
      styles_partial: true,
    }, []);

    const { container } = render(<DocumentPreview prepared={prepared} translate={english} />);

    expect(container.querySelector(".preview-document-notices")).toBeNull();
    expect(screen.queryByText("history.preview.partial.nativeUnavailable")).toBeNull();
    const diagnostics = screen.getByTestId("preview-document-diagnostics");
    expect(diagnostics.textContent).toContain("Preview simplifications (1)");
    expect(diagnostics.textContent).toContain("styles_simplified");
    expect(diagnostics.textContent).toContain("Some markup or text formatting was simplified for reading.");
  });

  it("collapses duplicate partial reasons by code and detail", async () => {
    const prepared = await prepare({
      version: 1,
      blocks: [documentParagraph([documentRun("x")])],
      assets: [],
      partial: [
        { code: "external_reference", detail: "same detail" },
        { code: "external_reference", detail: "same detail" },
      ],
      styles_partial: false,
    }, []);

    render(<DocumentPreview prepared={prepared} translate={translate} />);

    expect(screen.getAllByText("history.preview.partial.missingImage")).toHaveLength(1);
    expect(screen.getAllByText("same detail")).toHaveLength(1);
  });

  it("keeps the original code and detail reachable from the folded details", async () => {
    const prepared = await prepare({
      version: 1,
      blocks: [documentParagraph([documentRun("x")])],
      assets: [],
      partial: [{ code: "external_reference", detail: "first image unavailable" }],
      styles_partial: false,
    }, []);

    render(<DocumentPreview prepared={prepared} translate={translate} />);

    expect(screen.getByText("external_reference")).toBeInTheDocument();
    expect(screen.getByText("first image unavailable")).toBeInTheDocument();
  });

  it("keeps real missing image, truncation, structure and provenance warnings", async () => {
    const prepared = await prepare({
      version: 1,
      blocks: [documentParagraph([documentRun("x")])],
      assets: [],
      partial: [
        { code: "missing_image_source", detail: "no image bytes" },
        { code: "text_limit", detail: "text budget reached" },
        { code: "native_table_structure_unavailable", detail: "native tables are counts" },
        { code: "unverified_provenance", detail: "origin could not be verified" },
      ],
      styles_partial: false,
    }, []);

    render(<DocumentPreview prepared={prepared} translate={translate} />);

    expect(screen.getByRole("note", { name: "history.preview.partialTitle" })).toBeInTheDocument();
    expect(screen.getByText("history.preview.partial.missingImage")).toBeInTheDocument();
    expect(screen.getByText("no image bytes")).toBeInTheDocument();
    expect(screen.getByText("origin could not be verified")).toBeInTheDocument();
    expect(screen.queryByText("history.preview.partial.nativeUnavailable")).toBeNull();
  });

  it("degrades repeated placements to placeholders instead of raising the pixel budget", async () => {
    const large = documentAsset(0, 0, buildPngAsset(2048, 2048, 0x55));
    const prepared = await prepare({
      version: 1,
      blocks: Array.from({ length: 5 }, (_, ordinal) => documentImageBlock(0, ordinal)),
      assets: [large.asset],
      partial: [],
      styles_partial: false,
      first_image: 0,
    }, [large]);

    render(<DocumentPreview prepared={prepared} translate={translate} />);

    expect(screen.getAllByTestId("preview-document-image")).toHaveLength(3);
    const placeholders = screen.getAllByTestId("preview-document-image-placeholder");
    expect(placeholders).toHaveLength(2);
    expect(placeholders.every((element) => element.getAttribute("data-budget-limited") === "true")).toBe(true);
    expect(screen.getByText("history.preview.partial.pixelBudget")).toBeInTheDocument();
  });

  it("renders bullet and ordered lists with list semantics", async () => {
    const prepared = await prepare({
      version: 1,
      blocks: [
        documentParagraph([documentRun("first")], "bullet"),
        documentParagraph([documentRun("second")], "bullet"),
        documentParagraph([documentRun("third")], "ordered"),
      ],
      assets: [],
      partial: [],
      styles_partial: false,
    }, []);

    render(<DocumentPreview prepared={prepared} translate={translate} />);
    expect(screen.getAllByRole("list")).toHaveLength(2);
    expect(screen.getAllByRole("listitem").map((item) => item.textContent)).toEqual(["first", "second", "third"]);
  });
  it("degrades an image that fails to decode into an honest placeholder", async () => {
    const red = documentAsset(0, 0, buildPngAsset(4, 4, 0x33));
    const prepared = await prepare({
      version: 1,
      blocks: [documentImageBlock(0, 0)],
      assets: [red.asset],
      partial: [],
      styles_partial: false,
      first_image: 0,
    }, [red]);

    render(<DocumentPreview prepared={prepared} translate={translate} />);
    fireEvent.error(screen.getByTestId("preview-document-image"));

    const placeholder = await screen.findByTestId("preview-document-image-placeholder");
    expect(placeholder.getAttribute("data-availability")).toBe("unsupported");
    expect(placeholder.getAttribute("aria-label")).toContain("history.preview.imageAvailability.unsupported");
    expect(placeholder.getAttribute("aria-label")).toContain("history.preview.imageCannotDisplay");
    expect(placeholder.getAttribute("aria-label")).not.toContain("history.preview.imageUnavailable");
    expect(screen.queryByTestId("preview-document-image")).toBeNull();
    expect(screen.getByRole("note", { name: "history.preview.partialTitle" })).toBeInTheDocument();
    expect(screen.getByText("unsupported_image")).toBeInTheDocument();
    expect(screen.getByText("history.preview.partial.unsupportedImage")).toBeInTheDocument();
  });

  it("merges several warnings into one summary and folds the original reasons", async () => {
    const prepared = await prepare({
      version: 1,
      blocks: [documentParagraph([documentRun("x")])],
      assets: [],
      partial: [
        { code: "missing_image_source", detail: "no image bytes" },
        { code: "text_limit", detail: "text budget reached" },
      ],
      styles_partial: false,
    }, []);

    const { container } = render(<DocumentPreview prepared={prepared} translate={english} />);

    const summaries = container.querySelectorAll(".preview-document-notice-summary");
    expect(summaries).toHaveLength(1);
    expect(summaries[0].textContent).toContain("Images not saved");
    expect(summaries[0].textContent).toContain("Content truncated");
    const details = container.querySelector("details.preview-document-notice-details");
    expect(details).not.toBeNull();
    expect(details?.querySelectorAll("li")).toHaveLength(2);
    expect(details?.textContent).toContain("missing_image_source");
    expect(details?.textContent).toContain("no image bytes");
  });

  it("offers a low-noise diagnostics entry when only formatting was simplified", async () => {
    const prepared = await prepare({
      version: 1,
      blocks: [documentParagraph([documentRun("x")])],
      assets: [],
      partial: [{ code: "native_styles_unavailable", detail: "native styles are not positional" }],
      styles_partial: true,
    }, []);

    const { container } = render(<DocumentPreview prepared={prepared} translate={english} />);

    expect(container.querySelector(".preview-document-notices")).toBeNull();
    const diagnostics = screen.getByTestId("preview-document-diagnostics");
    expect(diagnostics.textContent).toContain("Preview simplifications (1)");
    expect(diagnostics.textContent).toContain("native_styles_unavailable");
  });

  it("classifies known partial codes like the macOS renderer", async () => {
    const warningCodes = [
      "text_limit",
      "pixel_budget",
      "malformed_html",
      "unsupported_dependency",
      "missing_image_source",
      "external_reference",
      "native_analysis_unavailable",
      "native_table_structure_unavailable",
      "native_partial",
      "other_native_items",
      "unverified_provenance",
    ];
    const diagnosticCodes = ["native_styles_unavailable"];
    for (const code of warningCodes) {
      const prepared = await prepare({
        version: 1, blocks: [documentParagraph([documentRun("x")])], assets: [],
        partial: [{ code, detail: `${code} detail` }], styles_partial: false,
      }, []);
      const view = render(<DocumentPreview prepared={prepared} translate={english} />);
      expect(view.container.querySelector(".preview-document-notices"), `${code} should warn`).not.toBeNull();
      expect(view.container.querySelector(".preview-document-diagnostics"), `${code} should not be a diagnostic`).toBeNull();
      view.unmount();
    }
    for (const code of diagnosticCodes) {
      const prepared = await prepare({
        version: 1, blocks: [documentParagraph([documentRun("x")])], assets: [],
        partial: [{ code, detail: `${code} detail` }], styles_partial: false,
      }, []);
      const view = render(<DocumentPreview prepared={prepared} translate={english} />);
      expect(view.container.querySelector(".preview-document-notices"), `${code} should not warn`).toBeNull();
      expect(view.container.querySelector(".preview-document-diagnostics"), `${code} should be a diagnostic`).not.toBeNull();
      view.unmount();
    }
  });

  it("always warns about unsupported markup even when styles were also simplified", async () => {
    for (const stylesPartial of [false, true]) {
      const prepared = await prepare({
        version: 1, blocks: [documentParagraph([documentRun("x")])], assets: [],
        partial: [{ code: "unsupported_markup", detail: "markup detail" }], styles_partial: stylesPartial,
      }, []);
      const view = render(<DocumentPreview prepared={prepared} translate={english} />);
      expect(view.container.querySelector(".preview-document-notices"), `styles_partial=${stylesPartial}`).not.toBeNull();
      const summary = view.container.querySelector(".preview-document-notice-summary");
      expect(summary?.textContent).toContain("Markup not shown as stored");
      expect(screen.getByText("unsupported_markup")).toBeInTheDocument();
      if (stylesPartial) {
        expect(view.container.querySelector(".preview-document-notice-details")?.textContent).toContain("styles_simplified");
      }
      view.unmount();
    }
  });

  it("keeps an unknown partial code reachable as a diagnostic instead of dropping it", async () => {
    const frame = decodeDocumentFrame(encodeDocumentFrame(control, {
      version: 1, blocks: [documentParagraph([documentRun("x")])], assets: [], partial: [], styles_partial: false, first_image: null,
    }, new Uint8Array()), request);
    const patched: DecodedDocumentFrame = {
      ...frame,
      document: {
        ...frame.document,
        partial: [{ code: "future_reason", detail: "why it happened" }] as unknown as Contract.DocumentPartial[],
      },
    };

    const prepared = await prepareDocumentAssets(patched);
    const { container } = render(<DocumentPreview prepared={prepared} translate={english} />);

    expect(container.querySelector(".preview-document-notices")).toBeNull();
    const diagnostics = screen.getByTestId("preview-document-diagnostics");
    expect(diagnostics.textContent).toContain("Preview simplifications (1)");
    expect(diagnostics.textContent).toContain("future_reason");
    expect(diagnostics.textContent).toContain("why it happened");
  });

  it("does not duplicate the pixel budget reason when the document already reports it", async () => {
    const large = documentAsset(0, 0, buildPngAsset(2048, 2048, 0x55));
    const prepared = await prepare({
      version: 1,
      blocks: Array.from({ length: 5 }, (_, ordinal) => documentImageBlock(0, ordinal)),
      assets: [large.asset],
      partial: [{ code: "pixel_budget", detail: "placement budget reached" }],
      styles_partial: false,
      first_image: 0,
    }, [large]);

    const { container } = render(<DocumentPreview prepared={prepared} translate={english} />);

    expect(container.querySelectorAll('li[data-partial-code="pixel_budget"]')).toHaveLength(1);
  });

  it("labels an unsaved image position without implying a download failure", () => {
    const locales: Array<Record<string, string>> = [en, zhCN];
    expect(en["history.preview.imageUnavailable"]).toBe("Image not saved");
    expect(zhCN["history.preview.imageUnavailable"]).toBe("图片未保存");
    expect(en["history.preview.imageCannotDisplay"]).toBe("Image cannot be displayed");
    expect(zhCN["history.preview.imageCannotDisplay"]).toBe("图片无法显示");
    for (const locale of locales) {
      expect(locale["history.preview.imageUnavailable"]).not.toMatch(/download|下载失败/i);
      expect(locale["history.preview.imageCannotDisplay"]).not.toMatch(/download|下载失败/i);
      for (const code of ["missing_resource", "missing_source", "external_reference"]) {
        expect(locale[`history.preview.imageAvailability.${code}`]).not.toMatch(/download failed|下载失败/i);
      }
    }
  });

  it("provides every notice string in both languages", () => {
    const englishMessages: Record<string, string> = en;
    const chineseMessages: Record<string, string> = zhCN;
    const keys = Object.keys(englishMessages).filter((key) => key.startsWith("history.preview.notice"));
    expect(keys.length).toBeGreaterThan(0);
    for (const key of keys) expect(chineseMessages[key]).toBeTruthy();
    for (const key of [
      "history.preview.partial.simplified",
      "history.preview.partial.nativeTableStructure",
      "history.preview.partial.unsupportedMarkup",
      "history.preview.partial.unsupportedImage",
      "history.preview.notice.nativeUnavailable",
      "history.preview.notice.imageUnsupported",
      "history.preview.imageCannotDisplay",
    ]) {
      expect(chineseMessages[key]).toBeTruthy();
    }
  });

  it("renders three heading levels, quote depth, code whitespace and list depth", async () => {
    const prepared = await prepare({
      version: 1,
      blocks: [
        paragraph([documentRun("一级")], { heading: 1 }),
        paragraph([documentRun("二级")], { heading: 2 }),
        paragraph([documentRun("三级")], { heading: 3 }),
        paragraph([documentRun("引用一")], { quote_depth: 1 }),
        paragraph([documentRun("引用二")], { quote_depth: 1 }),
        paragraph([documentRun("引用深层")], { quote_depth: 2 }),
        paragraph([documentRun(" 保留   空白\n第二行 ")], { code_block: true }),
        paragraph([documentRun("顶层")], { list: "bullet", list_depth: 0 }),
        paragraph([documentRun("嵌套")], { list: "bullet", list_depth: 1 }),
        paragraph([documentRun("行内", { monospace: true })]),
      ],
      assets: [],
      partial: [],
      styles_partial: false,
    }, []);

    const { container } = render(<DocumentPreview prepared={prepared} translate={translate} />);

    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("一级");
    expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("二级");
    expect(screen.getByRole("heading", { level: 3 }).textContent).toBe("三级");

    const quotes = container.querySelectorAll("blockquote.preview-document-quote");
    expect([...quotes].map((quote) => quote.getAttribute("data-quote-depth"))).toEqual(["1", "2"]);
    expect(quotes[0].querySelectorAll("p")).toHaveLength(2);
    expect(quotes[1].textContent).toBe("引用深层");

    const code = container.querySelector("pre.preview-document-code > code");
    expect(code?.textContent).toBe(" 保留   空白\n第二行 ");

    const lists = container.querySelectorAll("ol.preview-document-list, ul.preview-document-list");
    expect([...lists].map((list) => list.getAttribute("data-list-depth"))).toEqual(["0", "1"]);
    expect(lists[0].textContent).toBe("顶层");
    expect(lists[1].textContent).toBe("嵌套");

    expect(container.querySelector("code.preview-document-inline-code")?.textContent).toBe("行内");
    expect(screen.getByTestId("preview-document").textContent).toContain(" 保留   空白");
  });

  it("keeps highlighted runs readable on light and dark highlights without replacing explicit colours", async () => {
    const prepared = await prepare({
      version: 1,
      blocks: [
        paragraph([documentRun("亮底", { highlight: { red: 255, green: 255, blue: 255 } })]),
        paragraph([documentRun("暗底", { highlight: { red: 0, green: 0, blue: 0 } })]),
        paragraph([documentRun("原色", { color: { red: 255, green: 0, blue: 0 }, highlight: { red: 0, green: 0, blue: 0 } })]),
      ],
      assets: [],
      partial: [],
      styles_partial: false,
    }, []);

    const { container } = render(<DocumentPreview prepared={prepared} translate={translate} />);
    const runs = container.querySelectorAll(".preview-document-paragraph span");
    expect(runs[0]).toHaveStyle({ backgroundColor: "rgb(255, 255, 255)", color: "rgb(0, 0, 0)" });
    expect(runs[1]).toHaveStyle({ backgroundColor: "rgb(0, 0, 0)", color: "rgb(255, 255, 255)" });
    expect(runs[2]).toHaveStyle({ backgroundColor: "rgb(0, 0, 0)", color: "rgb(255, 0, 0)" });
  });

  it("adapts ordinary explicit colours when the preview background theme changes", async () => {
    const originalGetStyle = window.getComputedStyle.bind(window);
    let background = "rgb(0, 0, 0)";
    vi.spyOn(window, "getComputedStyle").mockImplementation((element, pseudo) => {
      const computed = originalGetStyle(element, pseudo);
      if (!element.classList.contains("preview-document")) return computed;
      return new Proxy(computed, {
        get(target, property) {
          return property === "backgroundColor" ? background : Reflect.get(target, property, target);
        },
      });
    });
    const dark = { red: 0, green: 0, blue: 0 };
    const light = { red: 255, green: 255, blue: 255 };
    const prepared = await prepare({
      version: 1,
      blocks: [paragraph([documentRun("dark", { color: dark })]), paragraph([documentRun("light", { color: light })])],
      assets: [], partial: [], styles_partial: false,
    }, []);
    const { container } = render(<div className="app dark"><DocumentPreview prepared={prepared} translate={translate} /></div>);
    const runs = container.querySelectorAll(".preview-document-paragraph span");
    expect(runs[0]).toHaveStyle({ color: "rgb(255, 255, 255)" });
    expect(runs[1]).toHaveStyle({ color: "rgb(255, 255, 255)" });
    background = "rgb(255, 255, 255)";
    container.querySelector(".app")!.className = "app light";
    await waitFor(() => {
      expect(runs[0]).toHaveStyle({ color: "rgb(0, 0, 0)" });
      expect(runs[1]).toHaveStyle({ color: "rgb(0, 0, 0)" });
    });
    expect(prepared.frame.document.blocks[0].paragraph?.runs[0].style.color).toEqual(dark);
    expect(prepared.frame.document.blocks[1].paragraph?.runs[0].style.color).toEqual(light);
  });

  it("adjusts unreadable explicit highlight colours without changing the source model", async () => {
    const dark = { red: 0, green: 0, blue: 0 };
    const light = { red: 255, green: 255, blue: 255 };
    const prepared = await prepare({
      version: 1,
      blocks: [
        paragraph([documentRun("dark", { color: dark, highlight: dark })]),
        paragraph([documentRun("light", { color: light, highlight: light })]),
      ],
      assets: [], partial: [], styles_partial: false,
    }, []);
    const { container } = render(<DocumentPreview prepared={prepared} translate={translate} />);
    const runs = container.querySelectorAll(".preview-document-paragraph span");
    expect(runs[0]).toHaveStyle({ backgroundColor: "rgb(0, 0, 0)", color: "rgb(255, 255, 255)" });
    expect(runs[1]).toHaveStyle({ backgroundColor: "rgb(255, 255, 255)", color: "rgb(0, 0, 0)" });
    expect(prepared.frame.document.blocks[0].paragraph?.runs[0].style.color).toEqual(dark);
    expect(prepared.frame.document.blocks[1].paragraph?.runs[0].style.color).toEqual(light);
  });

  it("resumes ordered parent numbering after nested lists and resets after plain text", async () => {
    const prepared = await prepare({
      version: 1,
      blocks: [
        paragraph([documentRun("parent")], { list: "ordered", list_depth: 0 }),
        paragraph([documentRun("child")], { list: "ordered", list_depth: 1 }),
        paragraph([documentRun("bullet")], { list: "bullet", list_depth: 2 }),
        paragraph([documentRun("child two")], { list: "ordered", list_depth: 1 }),
        paragraph([documentRun("parent two")], { list: "ordered", list_depth: 0 }),
        paragraph([documentRun("new child")], { list: "ordered", list_depth: 1 }),
        paragraph([documentRun("parent three")], { list: "ordered", list_depth: 0 }),
        paragraph([documentRun("plain")]),
        paragraph([documentRun("new list")], { list: "ordered", list_depth: 0 }),
      ],
      assets: [], partial: [], styles_partial: false,
    }, []);
    const { container } = render(<DocumentPreview prepared={prepared} translate={translate} />);
    const lists = container.querySelectorAll<HTMLOListElement>("ol.preview-document-list");
    expect([...lists].map((list) => list.start)).toEqual([1, 1, 2, 2, 1, 3, 1]);
  });

  it("opens a safe link only through the injected opener on an explicit activation", async () => {
    const prepared = await prepare({
      version: 1,
      blocks: [
        paragraph([{ ...documentRun("示例"), link: "https://example.com/docs" }]),
        paragraph([documentRun("纯文本")]),
      ],
      assets: [],
      partial: [],
      styles_partial: false,
    }, []);

    const openExternal = vi.fn();
    const { container } = render(
      <DocumentPreview prepared={prepared} translate={translate} openExternal={openExternal} />,
    );

    const links = container.querySelectorAll("a.preview-document-link");
    expect(links).toHaveLength(1);
    expect(openExternal).not.toHaveBeenCalled();
    expect(links[0]).not.toHaveAttribute("href");
    expect(links[0]).toHaveAttribute("role", "link");
    expect(links[0]).toHaveAttribute("tabindex", "0");
    expect(links[0]).toHaveAttribute("data-href", "https://example.com/docs");
    expect(links[0]).toHaveAttribute("title", "https://example.com/docs");
    expect(links[0].textContent).toBe("示例");

    fireEvent.click(links[0]);
    expect(openExternal).toHaveBeenCalledTimes(1);
    expect(openExternal).toHaveBeenCalledWith("https://example.com/docs");

    fireEvent.keyDown(links[0], { key: "Enter" });
    expect(openExternal).toHaveBeenCalledTimes(2);

    fireEvent.keyDown(links[0], { key: " " });
    expect(openExternal).toHaveBeenCalledTimes(3);

    fireEvent.keyDown(links[0], { key: "Tab" });
    expect(openExternal).toHaveBeenCalledTimes(3);
    expect(container.querySelectorAll(".preview-document-paragraph span")).toHaveLength(1);
    expect(container.textContent).toContain("纯文本");
  });

  it("never turns an unsafe link target into an anchor", () => {
    const targets = [
      "javascript:alert(1)",
      "file:///private/secret",
      "https://user:pass@example.com/",
      "https://@example.com/",
      "/relative/path",
      "https://example.com/\\evil",
      "https://example.com/pa\nth",
      "mailto:someone@example.com",
      `https://example.com/${"a".repeat(2100)}`,
      " https://example.com/",
    ];
    const openExternal = vi.fn();
    const prepared = stubPrepared({
      version: 1,
      blocks: targets.map((target) => paragraph([{ ...documentRun("目标"), link: target }])),
      assets: [],
      partial: [],
      styles_partial: false,
    });

    const { container } = render(
      <DocumentPreview prepared={prepared} translate={translate} openExternal={openExternal} />,
    );

    expect(container.querySelectorAll("a")).toHaveLength(0);
    expect(container.textContent).toBe("目标".repeat(targets.length));
    expect(openExternal).not.toHaveBeenCalled();
  });

  it("keeps legacy documents unchanged when the new paragraph and run fields are absent", async () => {
    const prepared = await prepare({
      version: 1,
      blocks: [paragraph([documentRun("旧文稿")], {})],
      assets: [],
      partial: [],
      styles_partial: false,
    }, []);

    const { container } = render(<DocumentPreview prepared={prepared} translate={translate} />);

    expect(container.querySelector("h1, h2, h3, blockquote, pre, code, a")).toBeNull();
    const legacy = container.querySelector("p.preview-document-paragraph");
    expect(legacy?.textContent).toBe("旧文稿");
    expect(legacy?.querySelector("span")?.textContent).toBe("旧文稿");
  });

  it("accepts only absolute credential-free http(s) targets without whitespace, control characters or backslashes", () => {
    expect(safeDocumentLink("https://example.com/a?b=c#d")).toBe("https://example.com/a?b=c#d");
    expect(safeDocumentLink("http://example.com/")).toBe("http://example.com/");
    expect(safeDocumentLink("HTTPS://example.com/")).toBe("https://example.com/");
    for (const rejected of [
      null,
      undefined,
      "",
      "   ",
      "https://example.com/a b",
      "https://example.com/a\tb",
      "https://example.com/a\nb",
      "https://example.com/a\rb",
      "https://example.com/a\u0000b",
      "https://example.com/a\u0085b",
      "https://example.com/a\u3000b",
      "https://example.com/a\\b",
      "https://user@example.com/",
      "https://user:pass@example.com/",
      "https://@example.com/",
      "ftp://example.com/",
      "file:///tmp/x",
      "javascript:void(0)",
      "/relative",
      "example.com",
      "https://",
      "https://example.com:notaport/",
    ]) {
      expect(safeDocumentLink(rejected)).toBeNull();
    }
    expect(safeDocumentLink(`https://example.com/${"a".repeat(2048)}`)).toBeNull();
    expect(safeDocumentLink(`https://example.com/${"a".repeat(2020)}`)).not.toBeNull();
  });

  it("chooses readable highlight text colours for light and dark highlights", () => {
    expect(readableTextColor({ red: 255, green: 255, blue: 255 })).toBe("rgb(0, 0, 0)");
    expect(readableTextColor({ red: 200, green: 200, blue: 200 })).toBe("rgb(0, 0, 0)");
    expect(readableTextColor({ red: 255, green: 255, blue: 0 })).toBe("rgb(0, 0, 0)");
    expect(readableTextColor({ red: 0, green: 0, blue: 0 })).toBe("rgb(255, 255, 255)");
    expect(readableTextColor({ red: 90, green: 30, blue: 160 })).toBe("rgb(255, 255, 255)");
  });

  it("clamps every heading level into the three reading levels", () => {
    expect(headingLevel(1)).toBe(1);
    expect(headingLevel(2)).toBe(2);
    expect(headingLevel(3)).toBe(3);
    expect(headingLevel(6)).toBe(3);
    expect(headingLevel(0)).toBeNull();
    expect(headingLevel(null)).toBeNull();
    expect(headingLevel(undefined)).toBeNull();
    expect(headingLevel(Number.NaN)).toBeNull();
  });

});
