import { describe, expect, it, vi } from "vitest";

import {
  PREFERENCE_KEY,
  RuntimePackService,
  readRuntimePreference,
  resolveRuntimeSelection,
  type RuntimePack,
  type RuntimePackInventory,
} from "./runtimePacks";

class MemoryStorage implements Storage {
  private readonly values = new Map<string, string>();

  get length() { return this.values.size; }
  clear() { this.values.clear(); }
  getItem(key: string) { return this.values.get(key) ?? null; }
  key(index: number) { return [...this.values.keys()][index] ?? null; }
  removeItem(key: string) { this.values.delete(key); }
  setItem(key: string, value: string) { this.values.set(key, value); }
}

function pack(id: string, backend: "cpu" | "cuda", status: "ready" | "invalid"): RuntimePack {
  return {
    id,
    backend: status === "ready" ? backend : null,
    status,
    runtimeVersion: status === "ready" ? "0.1.0" : null,
    llamaCppCommit: status === "ready" ? "test-commit" : null,
    abiMajor: status === "ready" ? 1 : null,
    abiMinor: status === "ready" ? 0 : null,
    devices: status === "ready" ? [{ index: 0, id: `${backend}:0`, name: backend, vendor: "Test" }] : [],
    error: status === "invalid" ? "probe failed" : null,
  };
}

function inventory(packs: RuntimePack[]): RuntimePackInventory {
  return { packs, fallbackPackId: "cpu-dev" };
}

describe("runtime pack selection", () => {
  it("lists installed packs through the fixed command", async () => {
    const invoke = vi.fn(async () => inventory([]));
    await new RuntimePackService({ invoke }).list();
    expect(invoke).toHaveBeenCalledWith("list_runtime_packs");
  });

  it("rejects a stored preference that is not ready", () => {
    const value = inventory([pack("cuda-broken", "cuda", "invalid"), pack("cpu-dev", "cpu", "ready")]);

    expect(resolveRuntimeSelection(value, { packId: "cuda-broken", backend: "cuda" }))
      .toEqual({ packId: "cpu-dev", backend: "cpu", deviceIndex: 0 });
  });

  it("leaves an invalid stored preference intact while using fallback", () => {
    const storage = new MemoryStorage();
    storage.setItem(PREFERENCE_KEY, JSON.stringify({ packId: "missing", backend: "cuda" }));
    const value = inventory([pack("cpu-dev", "cpu", "ready")]);

    expect(resolveRuntimeSelection(value, readRuntimePreference(storage)))
      .toEqual({ packId: "cpu-dev", backend: "cpu", deviceIndex: 0 });
    expect(storage.getItem(PREFERENCE_KEY)).toContain("missing");
  });

  it("ignores malformed stored preferences", () => {
    const storage = new MemoryStorage();
    storage.setItem(PREFERENCE_KEY, "not-json");
    expect(readRuntimePreference(storage)).toBeNull();
  });
});
