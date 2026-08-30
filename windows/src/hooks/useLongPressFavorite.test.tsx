import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  LONG_PRESS_CHARGE_MS,
  LONG_PRESS_GRACE_MS,
  useLongPressFavorite,
} from "./useLongPressFavorite";

function Harness({
  onComplete,
  onClick = () => {},
  onContextAction = () => {},
  isFavorite = false,
}: {
  onComplete: () => void;
  onClick?: () => void;
  onContextAction?: () => void;
  isFavorite?: boolean;
}) {
  const gesture = useLongPressFavorite(onComplete, true, isFavorite);
  return (
    <div
      data-testid="row"
      data-charging={gesture.isCharging ? "true" : "false"}
      data-triggered={gesture.isTriggered ? "true" : "false"}
      data-triggered-action={gesture.triggeredAction ?? "none"}
      onPointerDown={gesture.onPointerDown}
      onPointerMove={gesture.onPointerMove}
      onPointerUp={gesture.onPointerUp}
      onPointerCancel={gesture.onPointerCancel}
      onClick={() => {
        if (!gesture.suppressClick()) onClick();
      }}
      onContextMenu={(event) => {
        event.preventDefault();
        const completedActivePress = gesture.suppressContextMenu();
        gesture.cancel();
        if (!completedActivePress) onContextAction();
      }}
    />
  );
}

describe("useLongPressFavorite", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("waits through the grace period, charges, then commits once", () => {
    const onComplete = vi.fn();
    render(<Harness onComplete={onComplete} />);
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { button: 0, pointerId: 1, clientX: 20, clientY: 20 });
    act(() => vi.advanceTimersByTime(LONG_PRESS_GRACE_MS - 1));
    expect(row).toHaveAttribute("data-charging", "false");

    act(() => vi.advanceTimersByTime(1));
    expect(row).toHaveAttribute("data-charging", "true");
    act(() => vi.advanceTimersByTime(LONG_PRESS_CHARGE_MS - 1));
    expect(onComplete).not.toHaveBeenCalled();

    act(() => vi.advanceTimersByTime(1));
    expect(onComplete).toHaveBeenCalledOnce();
    expect(row).toHaveAttribute("data-triggered", "true");
    expect(row).toHaveAttribute("data-triggered-action", "favorite");

    fireEvent.pointerUp(row, { pointerId: 1 });
    expect(onComplete).toHaveBeenCalledOnce();
  });

  it("retains the unfavorite direction for the macOS-style release animation", () => {
    const onComplete = vi.fn();
    render(<Harness onComplete={onComplete} isFavorite />);
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { button: 0, pointerId: 7, clientX: 20, clientY: 20 });
    act(() => vi.advanceTimersByTime(LONG_PRESS_GRACE_MS + LONG_PRESS_CHARGE_MS));

    expect(onComplete).toHaveBeenCalledOnce();
    expect(row).toHaveAttribute("data-triggered-action", "unfavorite");
  });

  it("ends the transient completion stamp without re-enabling the same click", () => {
    const onComplete = vi.fn();
    const onClick = vi.fn();
    render(<Harness onComplete={onComplete} onClick={onClick} />);
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { button: 0, pointerId: 6, clientX: 20, clientY: 20 });
    act(() => vi.advanceTimersByTime(LONG_PRESS_GRACE_MS + LONG_PRESS_CHARGE_MS));
    expect(row).toHaveAttribute("data-triggered", "true");

    act(() => vi.advanceTimersByTime(549));
    expect(row).toHaveAttribute("data-triggered", "true");
    act(() => vi.advanceTimersByTime(1));
    expect(row).toHaveAttribute("data-triggered", "false");

    fireEvent.pointerUp(row, { pointerId: 6 });
    fireEvent.click(row);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("cancels when the pointer moves before completion", () => {
    const onComplete = vi.fn();
    render(<Harness onComplete={onComplete} />);
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { button: 0, pointerId: 2, clientX: 20, clientY: 20 });
    fireEvent.pointerMove(row, { pointerId: 2, clientX: 30, clientY: 20 });
    act(() => vi.advanceTimersByTime(LONG_PRESS_GRACE_MS + LONG_PRESS_CHARGE_MS));

    expect(onComplete).not.toHaveBeenCalled();
    expect(row).toHaveAttribute("data-charging", "false");
  });

  it("suppresses the click produced by a completed long press", () => {
    const onComplete = vi.fn();
    const onClick = vi.fn();
    render(<Harness onComplete={onComplete} onClick={onClick} />);
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { button: 0, pointerId: 3, clientX: 20, clientY: 20 });
    act(() => vi.advanceTimersByTime(LONG_PRESS_GRACE_MS + LONG_PRESS_CHARGE_MS));
    fireEvent.pointerUp(row, { pointerId: 3 });
    fireEvent.click(row);

    expect(onComplete).toHaveBeenCalledOnce();
    expect(onClick).not.toHaveBeenCalled();

    fireEvent.pointerDown(row, { button: 0, pointerId: 4, clientX: 20, clientY: 20 });
    fireEvent.pointerUp(row, { pointerId: 4 });
    fireEvent.click(row);
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("does not turn a completed active long press into a context action", () => {
    const onComplete = vi.fn();
    const onContextAction = vi.fn();
    render(
      <Harness
        onComplete={onComplete}
        onContextAction={onContextAction}
      />,
    );
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { button: 0, pointerId: 5, clientX: 20, clientY: 20 });
    act(() => vi.advanceTimersByTime(LONG_PRESS_GRACE_MS + LONG_PRESS_CHARGE_MS));
    fireEvent.contextMenu(row);

    expect(onComplete).toHaveBeenCalledOnce();
    expect(onContextAction).not.toHaveBeenCalled();

    fireEvent.contextMenu(row);
    expect(onContextAction).toHaveBeenCalledOnce();
  });
});
