import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PREVIEW_TEXT_FONT_SIZE_KEY } from "../previewPreferences";
import { TextPreview } from "./TextPreview";
import {
  TEXT_PREVIEW_MAX_LINE_NUMBER_ROWS,
  TEXT_PREVIEW_RENDER_MAX_CHARS,
} from "./textPreviewPolicy";

const t = (key: string) => key;

describe("TextPreview", () => {
  it("renders selectable 18px text and remembers user zoom", () => {
    render(<TextPreview data={new TextEncoder().encode("Readable text")} name="note.txt" t={t} />);
    const text = screen.getByText("Readable text");
    expect(text).toHaveStyle({ fontSize: "18px" });

    const scroller = text.closest(".preview-source-scroll");
    expect(scroller).not.toBeNull();
    const wheel = new WheelEvent("wheel", {
      bubbles: true,
      cancelable: true,
      ctrlKey: true,
      deltaY: -100,
    });
    fireEvent(scroller!, wheel);

    expect(text).toHaveStyle({ fontSize: "20px" });
    expect(wheel.defaultPrevented).toBe(true);
    expect(localStorage.getItem(PREVIEW_TEXT_FONT_SIZE_KEY)).toBe("20");
  });

  it("applies language-aware token classes and exposes the language", () => {
    const { container } = render(
      <TextPreview
        data={new TextEncoder().encode('const answer: number = 42;\nconsole.log("ready");')}
        name="main.ts"
        forceCode
        t={t}
      />,
    );

    expect(screen.getByText("typescript")).toBeInTheDocument();
    expect(container.querySelector(".hljs-keyword")).not.toBeNull();
    expect(container.querySelector(".hljs-number")).not.toBeNull();
    expect(container.querySelector(".hljs-string")).not.toBeNull();
  });

  it("lets users render clipboard text as sanitized Markdown", () => {
    render(<TextPreview data={new TextEncoder().encode("# Heading")} name="note.txt" t={t} />);

    fireEvent.click(screen.getByTitle("history.preview.markdownMode"));
    expect(screen.getByRole("heading", { name: "Heading" })).toBeInTheDocument();
  });

  it("bounds expensive code rendering while retaining complete copy statistics", () => {
    const source = `const value = 1;\n${"x".repeat(TEXT_PREVIEW_RENDER_MAX_CHARS + 100)}`;
    const { container } = render(
      <TextPreview data={new TextEncoder().encode(source)} name="large.ts" forceCode t={t} />,
    );

    expect(screen.getByTestId("preview-truncated")).toBeInTheDocument();
    const renderedCode = container.querySelector(".preview-code code");
    expect(renderedCode).not.toBeNull();
    expect(renderedCode!.textContent!.length).toBeLessThanOrEqual(TEXT_PREVIEW_RENDER_MAX_CHARS);
    expect(screen.getByText(`${source.length} history.preview.characters`)).toBeInTheDocument();
  });

  it("does not create an unbounded line-number DOM for newline-heavy text", () => {
    const source = Array.from(
      { length: TEXT_PREVIEW_MAX_LINE_NUMBER_ROWS + 2 },
      () => "x",
    ).join("\n");
    const { container } = render(
      <TextPreview data={new TextEncoder().encode(source)} name="many-lines.ts" forceCode t={t} />,
    );

    expect(screen.getByTestId("preview-line-numbers-disabled")).toBeInTheDocument();
    expect(container.querySelector(".preview-code-lines")).toBeNull();
    expect(screen.getByText(`${TEXT_PREVIEW_MAX_LINE_NUMBER_ROWS + 2} history.preview.lines`))
      .toBeInTheDocument();
  });
});
