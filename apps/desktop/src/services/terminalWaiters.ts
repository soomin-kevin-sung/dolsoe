interface TerminalWait {
  promise: Promise<void>;
  cancel(): void;
}

export async function restartAfterTerminalPersistence(
  hasActiveGeneration: boolean,
  stopAndPersist: () => Promise<void>,
  restart: () => Promise<void>,
): Promise<void> {
  if (hasActiveGeneration) await stopAndPersist();
  await restart();
}

export class TerminalWaiters {
  private readonly waiters = new Set<(error?: Error) => void>();

  wait(timeoutMs = 10_000): TerminalWait {
    let finish = (_error?: Error) => undefined;
    const promise = new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.waiters.delete(finish);
        reject(new Error("terminal event timed out"));
      }, timeoutMs);
      finish = (error?: Error) => {
        clearTimeout(timeout);
        this.waiters.delete(finish);
        if (error) reject(error);
        else resolve();
      };
      this.waiters.add(finish);
    });
    return { promise, cancel: finish };
  }

  resolveAll(): void {
    for (const resolve of [...this.waiters]) resolve();
  }

  rejectAll(error: Error): void {
    for (const reject of [...this.waiters]) reject(error);
  }
}
