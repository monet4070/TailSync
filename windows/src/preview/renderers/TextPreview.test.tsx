import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PREVIEW_TEXT_FONT_SIZE_KEY } from "../previewPreferences";
import { TextPreview } from "./TextPreview";

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
});
