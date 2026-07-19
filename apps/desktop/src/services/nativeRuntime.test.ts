import { describe, expect, it, vi } from "vitest";

import { NativeRuntimeService, type LlmEventDto, type NativeBindings } from "./nativeRuntime";

function bindings() {
  const unlisten = vi.fn();
  const invoke = vi.fn(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
    if (command === "llm_submit") return { requestHandle: "18446744073709551615" };
    if (command === "llm_get_metrics") return { generatedTokens: "0" };
    return { phase: "ready" };
  });
  const listen = vi.fn(async (_event: string, _handler: (event: { payload: unknown }) => void) => unlisten);
  const openGguf = vi.fn(async () => "D:\\models\\tiny.gguf");
  const value: NativeBindings = {
    invoke: async <T>(command: string, args?: Record<string, unknown>) => (
      args === undefined ? invoke(command) : invoke(command, args)
    ) as Promise<T>,
    listen: async <T>(event: string, handler: (event: { payload: T }) => void) => (
      listen(event, handler as (event: { payload: unknown }) => void)
    ),
    openGguf,
  };
  return { value, invoke, listen, openGguf, unlisten };
}

describe("NativeRuntimeService", () => {
  it("uses the fixed Tauri command contract", async () => {
    const fake = bindings();
    const service = new NativeRuntimeService(fake.value);
    const load = {
      runtimePackId: "cpu-dev",
      backend: "cpu" as const,
      deviceIndex: 0,
      modelPath: "D:\\models\\tiny.gguf",
      contextSize: 4096,
      batchSize: 512,
      physicalBatchSize: 128,
      threads: 8,
      useMmap: true,
    };
    const submit = { prompt: "안녕", maxNewTokens: 256, temperature: 0.8, topP: 0.95, seed: -1 };

    await service.getStatus();
    await service.loadModel(load);
    await service.unloadModel();
    await service.submit(submit);
    await service.cancel("18446744073709551615");
    await service.getMetrics();

    expect(fake.invoke.mock.calls).toEqual([
      ["llm_get_status"],
      ["llm_load_model", { request: load }],
      ["llm_unload_model"],
      ["llm_submit", { request: submit }],
      ["llm_cancel", { requestHandle: "18446744073709551615" }],
      ["llm_get_metrics"],
    ]);
  });

  it("subscribes to llm events and returns cleanup", async () => {
    const fake = bindings();
    const service = new NativeRuntimeService(fake.value);
    const listener = vi.fn();

    const cleanup = await service.subscribe(listener);
    const handler = fake.listen.mock.calls[0][1] as (event: { payload: LlmEventDto }) => void;
    handler({ payload: { kind: "done", requestHandle: "7", sequenceNumber: "2", bytes: [], errorCode: 0, metrics: null } });
    cleanup();

    expect(fake.listen.mock.calls[0][0]).toBe("llm://event");
    expect(listener).toHaveBeenCalledOnce();
    expect(fake.unlisten).toHaveBeenCalledOnce();
  });

  it("opens only one GGUF model path", async () => {
    const fake = bindings();
    const service = new NativeRuntimeService(fake.value);

    await expect(service.chooseModel()).resolves.toBe("D:\\models\\tiny.gguf");
    expect(fake.openGguf).toHaveBeenCalledOnce();
  });
});
