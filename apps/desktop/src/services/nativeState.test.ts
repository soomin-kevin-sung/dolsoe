import { describe, expect, it } from "vitest";

import { applyNativeEvent, createNativeState, nativeReducer, TokenDecoders } from "./nativeState";
import type { LlmEventDto } from "./nativeRuntime";

function event(kind: LlmEventDto["kind"], handle: string, bytes: number[] = []): LlmEventDto {
  return { kind, requestHandle: handle, sequenceNumber: "1", bytes, errorCode: 0, metrics: null };
}

function readyState() {
  return nativeReducer(createNativeState(), {
    type: "status",
    status: {
      phase: "ready",
      runtimePackId: "cpu-dev",
      modelPath: "D:\\models\\tiny.gguf",
      modelName: "tiny.gguf",
      backend: "CPU",
      loadingProgress: 1,
      activeRequestHandle: null,
      lastError: null,
    },
  });
}

describe("nativeState", () => {
  it("reassembles UTF-8 token bytes split inside Korean characters", () => {
    const bytes = [...new TextEncoder().encode("한글")];
    const decoders = new TokenDecoders();
    let state = nativeReducer(readyState(), { type: "submit-started", prompt: "한국어로 답해줘" });

    state = applyNativeEvent(state, event("token", "7", bytes.slice(0, 2)), decoders);
    state = applyNativeEvent(state, event("token", "7", bytes.slice(2, 4)), decoders);
    state = applyNativeEvent(state, event("token", "7", bytes.slice(4)), decoders);
    state = applyNativeEvent(state, event("done", "7"), decoders);

    expect(state.messages[state.messages.length - 1]?.content).toBe("한글");
    expect(state.messages[state.messages.length - 1]?.status).toBe("complete");
    expect(state.activeRequestHandle).toBeNull();
    expect(state.phase).toBe("ready");
  });

  it("ignores stale handles and repeated terminal events", () => {
    const decoders = new TokenDecoders();
    let state = nativeReducer(readyState(), { type: "submit-started", prompt: "질문" });
    state = nativeReducer(state, { type: "submit-accepted", requestHandle: "9" });
    state = applyNativeEvent(state, event("token", "8", [65]), decoders);
    state = applyNativeEvent(state, event("done", "9"), decoders);
    const completed = state;
    state = applyNativeEvent(state, event("error", "9", [66]), decoders);

    expect(completed.messages[completed.messages.length - 1]?.content).toBe("");
    expect(state).toEqual(completed);
  });

  it("marks a cancelled response and flushes its decoder", () => {
    const decoders = new TokenDecoders();
    let state = nativeReducer(readyState(), { type: "submit-started", prompt: "질문" });
    state = applyNativeEvent(state, event("token", "11", [65]), decoders);
    state = applyNativeEvent(state, event("cancelled", "11"), decoders);

    expect(state.messages[state.messages.length - 1]?.content).toBe("A");
    expect(state.messages[state.messages.length - 1]?.status).toBe("cancelled");
    expect(state.phase).toBe("ready");
  });

  it("settles a submit command failure instead of leaving a streaming state", () => {
    let state = nativeReducer(readyState(), { type: "submit-started", prompt: "질문" });

    state = nativeReducer(state, { type: "submit-failed", error: "runtime is busy" });

    expect(state.phase).toBe("error");
    expect(state.pendingSubmit).toBe(false);
    expect(state.messages[state.messages.length - 1]?.status).toBe("error");
    expect(state.error).toBe("runtime is busy");
  });
});
