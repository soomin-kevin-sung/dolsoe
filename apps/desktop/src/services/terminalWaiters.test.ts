import { afterEach, describe, expect, it, vi } from "vitest";

import { TerminalWaiters } from "./terminalWaiters";

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
});
