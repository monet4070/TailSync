import { describe, expect, it } from "vitest";
import { LatestRequest, SerialTaskQueue } from "./asyncControl";

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("LatestRequest", () => {
  it("marks earlier results stale when a newer request begins", () => {
    const requests = new LatestRequest();
    const first = requests.begin();
    const second = requests.begin();

    expect(requests.isCurrent(first)).toBe(false);
    expect(requests.isCurrent(second)).toBe(true);
  });

  it("can invalidate an outstanding result during teardown", () => {
    const requests = new LatestRequest();
    const current = requests.begin();

    requests.invalidate();

    expect(requests.isCurrent(current)).toBe(false);
  });
});

describe("SerialTaskQueue", () => {
  it("starts the next task only after the previous task settles", async () => {
    const queue = new SerialTaskQueue();
    const firstGate = deferred<void>();
    const events: string[] = [];

    const first = queue.enqueue(async () => {
      events.push("first:start");
      await firstGate.promise;
      events.push("first:end");
    });
    const second = queue.enqueue(async () => {
      events.push("second:start");
    });

    await Promise.resolve();
    expect(events).toEqual(["first:start"]);

    firstGate.resolve();
    await Promise.all([first, second]);
    expect(events).toEqual(["first:start", "first:end", "second:start"]);
  });

  it("continues processing after a task fails", async () => {
    const queue = new SerialTaskQueue();
    const failure = queue.enqueue(async () => {
      throw new Error("save failed");
    });
    const recovery = queue.enqueue(async () => "saved");

    await expect(failure).rejects.toThrow("save failed");
    await expect(recovery).resolves.toBe("saved");
  });
});
