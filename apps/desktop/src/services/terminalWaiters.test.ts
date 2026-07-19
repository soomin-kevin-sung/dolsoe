import { afterEach, describe, expect, it, vi } from "vitest";

import { restartAfterTerminalPersistence, TerminalWaiters } from "./terminalWaiters";

afterEach(() => vi.useRealTimers());

describe("TerminalWaiters", () => {
  it("resolves every concurrent waiter", async () => {
    const waiters = new TerminalWaiters();
    const first = waiters.wait();
    const second = waiters.wait();

    waiters.resolveAll();

    await expect(first.promise).resolves.toBeUndefined();
    await expect(second.promise).resolves.toBeUndefined();
  });

  it("rejects instead of waiting forever", async () => {
    vi.useFakeTimers();
    const pending = new TerminalWaiters().wait(10);
    const rejected = expect(pending.promise).rejects.toThrow("terminal event timed out");

    await vi.advanceTimersByTimeAsync(10);

    await rejected;
  });

  it("rejects waiters when terminal persistence fails", async () => {
    const waiters = new TerminalWaiters();
    const pending = waiters.wait();

    waiters.rejectAll(new Error("database write failed"));

    await expect(pending.promise).rejects.toThrow("database write failed");
  });

  it("restarts only after active generation reaches terminal persistence", async () => {
    let finishStop = () => undefined;
    const order: string[] = [];
    const stop = vi.fn(() => new Promise<void>((resolve) => {
      finishStop = () => {
        order.push("persisted");
        resolve();
      };
    }));
    const restart = vi.fn(async () => { order.push("restart"); });

    const pending = restartAfterTerminalPersistence(true, stop, restart);
    await Promise.resolve();
    expect(restart).not.toHaveBeenCalled();
    finishStop();
    await pending;

    expect(order).toEqual(["persisted", "restart"]);
  });

  it("does not restart when terminal persistence fails", async () => {
    const restart = vi.fn(async () => undefined);

    await expect(restartAfterTerminalPersistence(
      true,
      async () => { throw new Error("database write failed"); },
      restart,
    )).rejects.toThrow("database write failed");

    expect(restart).not.toHaveBeenCalled();
  });
});
