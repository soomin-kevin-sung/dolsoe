interface TerminalWait {
  promise: Promise<void>;
  cancel(): void;
}

export class TerminalWaiters {
  private readonly waiters = new Set<() => void>();

  wait(timeoutMs = 10_000): TerminalWait {
    let finish = () => undefined;
    const promise = new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.waiters.delete(finish);
        reject(new Error("terminal event timed out"));
      }, timeoutMs);
      finish = () => {
        clearTimeout(timeout);
        this.waiters.delete(finish);
        resolve();
      };
      this.waiters.add(finish);
    });
    return { promise, cancel: finish };
  }

  resolveAll(): void {
    for (const resolve of [...this.waiters]) resolve();
  }
}
