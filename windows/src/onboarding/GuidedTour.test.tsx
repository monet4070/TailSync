import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { GuidedTour } from "./GuidedTour";
import { endTour, endTourPermanently, moveTour, readTour, readTourSuppressed, saveTour, startTour, TOUR_KEY, TOUR_SUPPRESSED_KEY } from "./tourState";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("../hooks/useI18n", () => ({ useI18n: () => ({ locale: "zh-CN" }) }));

beforeEach(() => {
  localStorage.clear();
  invoke.mockReset().mockResolvedValue(undefined);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    left: 20, top: 70, right: 500, bottom: 120, width: 480, height: 50, x: 20, y: 70, toJSON: () => ({}),
  });
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); history.replaceState(null, "", "/"); });

it("highlights the real controls and advances without reopening the current window", async () => {
  saveTour({ active: true, index: 0 });
  const navigate = vi.fn(); const operate = vi.fn();
  render(<div className="app"><button data-tour="connection-mode" onClick={operate}>实际控件</button><div data-tour="pairing" /><GuidedTour window="connections" navigate={navigate} /></div>);
  expect(screen.getByRole("heading", { name: "先选择「自动」" })).toHaveFocus();
  fireEvent.click(screen.getByText("实际控件"));
  expect(operate).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "下一步" }));
  expect(await screen.findByRole("heading", { name: "点击「允许配对」" })).toBeInTheDocument();
  expect(navigate).not.toHaveBeenCalled();
});

it("hands off to history and rolls back when the destination cannot open", async () => {
  saveTour({ active: true, index: 3 });
  const navigate = vi.fn().mockRejectedValueOnce(new Error("window unavailable")).mockResolvedValue(undefined);
  render(<div className="app"><div data-tour="remote-pairing" /><GuidedTour window="connections" navigate={navigate} /></div>);
  fireEvent.click(screen.getByRole("button", { name: "下一步" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("操作未完成");
  expect(readTour()).toEqual({ active: true, index: 3 });
  fireEvent.click(screen.getByRole("button", { name: "下一步" }));
  await waitFor(() => expect(screen.queryByTestId("guided-tour")).not.toBeInTheDocument());
  expect(navigate).toHaveBeenLastCalledWith("history");
  expect(readTour().index).toBe(4);
});

it.each(["dialog", "listbox", "menu"])("lets a real %s take focus and restores the tour after it closes", async role => {
  saveTour({ active: true, index: 1 });
  const view = render(<div className="app"><div data-tour="pairing" /><GuidedTour window="connections" /></div>);
  view.rerender(<div className="app"><div data-tour="pairing" /><GuidedTour window="connections" /><div role={role}>操作选项</div></div>);
  await waitFor(() => expect(screen.queryByTestId("guided-tour")).not.toBeInTheDocument());
  view.rerender(<div className="app"><div data-tour="pairing" /><GuidedTour window="connections" /></div>);
  expect(await screen.findByTestId("guided-tour")).toBeInTheDocument();
});

it("continues when a mode-specific target is unavailable", async () => {
  saveTour({ active: true, index: 3 });
  const navigate = vi.fn().mockResolvedValue(undefined);
  render(<GuidedTour window="connections" navigate={navigate} />);
  expect(screen.getByText(/正在定位演示控件/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "下一步" }));
  await waitFor(() => expect(navigate).toHaveBeenCalledWith("history"));
});

it("starts a fresh installation at step one despite stale WebView progress", async () => {
  saveTour({ active: true, index: 10 });
  localStorage.setItem(TOUR_SUPPRESSED_KEY, "1");
  history.replaceState(null, "", "/connections.html?tour=start");
  const navigate = vi.fn().mockResolvedValue(undefined);
  render(<GuidedTour window="connections" navigate={navigate} />);
  expect(await screen.findByRole("heading", { name: "先选择「自动」" })).toBeInTheDocument();
  expect(navigate).not.toHaveBeenCalled();
  expect(location.search).toBe("");
  expect(readTour().index).toBe(0);
  expect(localStorage.getItem(TOUR_SUPPRESSED_KEY)).toBeNull();
});

it("receives progress changes from another window", () => {
  render(<GuidedTour window="history" />);
  expect(screen.queryByTestId("guided-tour")).not.toBeInTheDocument();
  act(() => {
    localStorage.setItem(TOUR_KEY, JSON.stringify({ active: true, index: 4 }));
    window.dispatchEvent(new StorageEvent("storage", { key: TOUR_KEY }));
  });
  expect(screen.getByRole("heading", { name: "从一次复制开始" })).toBeInTheDocument();
});

it("keeps the tour open on acknowledgement failure and allows retry", async () => {
  saveTour({ active: true, index: 14 });
  invoke.mockRejectedValueOnce(new Error("disk full")).mockResolvedValue(undefined);
  render(<GuidedTour window="settings" />);
  fireEvent.click(screen.getByRole("button", { name: "完成" }));
  expect(await screen.findByRole("alert")).toBeInTheDocument();
  expect(readTour().active).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "完成" }));
  await waitFor(() => expect(screen.queryByTestId("guided-tour")).not.toBeInTheDocument());
  expect(readTour().active).toBe(false);
});

it("discards corrupt or missing progress and restarts explicitly from step one", async () => {
  for (const value of ["broken", '{"active":true,"index":999}', '{"active":true,"index":-1}']) {
    localStorage.setItem(TOUR_KEY, value);
    expect(readTour()).toEqual({ active: false, index: 0 });
  }
  saveTour({ active: true, index: 8 });
  await startTour();
  expect(invoke).toHaveBeenCalledWith("open_onboarding_window");
  expect(readTour()).toEqual({ active: true, index: 0 });
  localStorage.clear();
  expect(readTour().active).toBe(false);
});

it("preserves progress on launch failure and does not navigate when storage fails", async () => {
  saveTour({ active: true, index: 8 });
  invoke.mockRejectedValue(new Error("window failed"));
  await expect(startTour()).rejects.toThrow("window failed");
  expect(readTour().index).toBe(8);
  const navigate = vi.fn();
  vi.spyOn(localStorage, "setItem").mockImplementation(() => { throw new Error("storage failed"); });
  await expect(moveTour(4, navigate)).rejects.toThrow("storage failed");
  expect(navigate).not.toHaveBeenCalled();
});

it("ends without closing the application's current window", async () => {
  saveTour({ active: true, index: 5 });
  await endTour();
  expect(invoke.mock.calls).toEqual([["dismiss_onboarding"]]);
  expect(readTour()).toEqual({ active: false, index: 5 });
});

it("can suppress the tour permanently from the card", async () => {
  saveTour({ active: true, index: 5 });
  await endTourPermanently();
  expect(invoke.mock.calls).toEqual([["suppress_onboarding"]]);
  expect(readTour()).toEqual({ active: false, index: 5 });
  expect(localStorage.getItem("tailsync-guided-tour-suppressed")).toBe("1");
});

it("uses native installation state to clear a suppression cache left by reinstall", async () => {
  localStorage.setItem(TOUR_SUPPRESSED_KEY, "1");
  invoke.mockResolvedValue(false);
  expect(await readTourSuppressed()).toBe(false);
  expect(localStorage.getItem(TOUR_SUPPRESSED_KEY)).toBeNull();
  await startTour();
  expect(invoke).toHaveBeenLastCalledWith("open_onboarding_window");
  expect(readTour().active).toBe(true);
});

it("cannot restart a permanently suppressed tour in the same installation", async () => {
  invoke.mockResolvedValue(true);
  await startTour();
  expect(invoke.mock.calls).toEqual([["onboarding_suppressed"]]);
  expect(readTour().active).toBe(false);
});

it("keeps the complete visible target highlighted and reserves layout space for the card", () => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    left: 20, top: 30, right: 500, bottom: 480, width: 480, height: 450, x: 20, y: 30, toJSON: () => ({}),
  });
  saveTour({ active: true, index: 4 });
  const view = render(<div className="app"><div data-tour="history-items" /><GuidedTour window="history" /></div>);
  const highlight = view.container.querySelector<HTMLElement>(".guided-tour-highlight")!;
  expect(highlight.style.top).toBe("25px");
  expect(highlight.style.height).toBe("460px");
  expect(view.container.querySelector(".app")).toHaveClass("has-guided-tour");
  expect(view.container.querySelector<HTMLElement>(".guided-tour-card")!.style.bottom).toBe("12px");
  act(() => saveTour({ active: false, index: 4 }));
  expect(view.container.querySelector(".app")).not.toHaveClass("has-guided-tour");
});

it("clips a scrolled section at its real scroll viewport", () => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return this.classList.contains("settings-content")
      ? { left: 10, top: 60, right: 520, bottom: 420, width: 510, height: 360, x: 10, y: 60, toJSON: () => ({}) }
      : { left: 20, top: -100, right: 500, bottom: 600, width: 480, height: 700, x: 20, y: -100, toJSON: () => ({}) };
  });
  saveTour({ active: true, index: 10 });
  const view = render(<div className="app"><div className="settings-content" style={{ overflowY: "auto" }}><div data-tour="general" /></div><GuidedTour window="settings" /></div>);
  const highlight = view.container.querySelector<HTMLElement>(".guided-tour-highlight")!;
  expect(highlight.style.top).toBe("60px");
  expect(highlight.style.height).toBe("360px");
});
