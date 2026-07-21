import { describe, expect, it, vi } from "vitest";

import {
  PREFERENCE_KEY,
  RuntimePackService,
  applyRuntimeSelection,
  canInstallRuntimePack,
  reduceRuntimeInstallState,
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

function pack(id: string, backend: "cpu" | "cuda", status: "ready" | "repair-required"): RuntimePack {
  return {
    id,
    backend,
    status,
    runtimeVersion: status === "ready" ? "0.1.0" : null,
    llamaCppCommit: status === "ready" ? "test-commit" : null,
    abiMajor: status === "ready" ? 1 : null,
    abiMinor: status === "ready" ? 0 : null,
    devices: status === "ready" ? [{ index: 0, id: `${backend}:0`, name: backend, vendor: "Test" }] : [],
    error: status === "repair-required" ? "probe failed" : null,
  };
}

function inventory(packs: RuntimePack[]): RuntimePackInventory {
  return { packs, fallbackPackId: "cpu" };
}

describe("runtime pack selection", () => {
  it("unloads, saves, and reloads the current model in order", async () => {
    const calls: string[] = [];
    await applyRuntimeSelection({ packId: "cuda", backend: "cuda", deviceIndex: 0 }, {
      modelPath: "D:\\models\\tiny.gguf",
      unload: async () => { calls.push("unload"); },
      persist: () => { calls.push("persist"); },
      load: async () => { calls.push("load"); },
    });
    expect(calls).toEqual(["unload", "persist", "load"]);
  });

  it("only saves a selection when no model is loaded", async () => {
    const calls: string[] = [];
    await applyRuntimeSelection({ packId: "cpu-dev", backend: "cpu", deviceIndex: 0 }, {
      modelPath: null,
      unload: async () => { calls.push("unload"); },
      persist: () => { calls.push("persist"); },
      load: async () => { calls.push("load"); },
    });
    expect(calls).toEqual(["persist"]);
  });

  it("lists installed packs through the fixed command", async () => {
    const invoke = vi.fn(async () => inventory([]));
    await new RuntimePackService({ invoke: invoke as never }).list();
    expect(invoke).toHaveBeenCalledWith("list_runtime_packs");
  });

  it("lists, installs, and cancels release packs through fixed commands", async () => {
    const invoke = vi.fn(async (command: string) => command === "list_available_runtime_packs"
      ? [{ id: "cuda", backend: "cuda", releaseVersion: "1", sizeBytes: 1024, llamaCppRelease: "b10068", llamaCppCommit: "commit", installed: false }]
      : undefined);
    const service = new RuntimePackService({ invoke: invoke as never });

    expect(await service.listAvailable()).toHaveLength(1);
    await service.install("cpu");
    await service.cancelInstall();

    expect(invoke).toHaveBeenNthCalledWith(1, "list_available_runtime_packs");
    expect(invoke).toHaveBeenNthCalledWith(2, "install_runtime_pack", { packId: "cpu" });
    expect(invoke).toHaveBeenNthCalledWith(3, "cancel_runtime_pack_install");
  });

  it("reduces runtime installation events without leaking an old error", () => {
    const downloading = reduceRuntimeInstallState(null, {
      packId: "cuda-1", phase: "downloading", downloadedBytes: 25, totalBytes: 100, error: null,
    });
    expect(downloading?.progress).toBe(25);
    const verifying = reduceRuntimeInstallState(downloading, {
      packId: "cuda-1", phase: "verifying", downloadedBytes: 100, totalBytes: 100, error: null,
    });
    expect(verifying).toMatchObject({ phase: "verifying", progress: 100, error: null });
    const failed = reduceRuntimeInstallState(verifying, {
      packId: "cuda-1", phase: "failed", downloadedBytes: 0, totalBytes: 0, error: "network",
    });
    expect(failed?.error).toBe("network");
    const restarted = reduceRuntimeInstallState(failed, {
      packId: "cuda-1", phase: "downloading", downloadedBytes: 0, totalBytes: 100, error: null,
    });
    expect(restarted?.error).toBeNull();
  });

  it("allows retry after a terminal install state but not while another install is active", () => {
    const failed = reduceRuntimeInstallState(null, {
      packId: "cuda-1", phase: "failed", downloadedBytes: 0, totalBytes: 0, error: "network",
    });
    const downloading = reduceRuntimeInstallState(null, {
      packId: "cuda-1", phase: "downloading", downloadedBytes: 1, totalBytes: 100, error: null,
    });

    expect(canInstallRuntimePack("cuda-1", false, failed)).toBe(true);
    expect(canInstallRuntimePack("cuda-1", false, downloading)).toBe(false);
    expect(canInstallRuntimePack("cuda-1", true, null)).toBe(false);
  });

  it("rejects a stored preference that is not ready", () => {
    const value = inventory([pack("cuda", "cuda", "repair-required"), pack("cpu", "cpu", "ready")]);

    expect(resolveRuntimeSelection(value, { packId: "cuda-broken", backend: "cuda" }))
      .toEqual({ packId: "cpu", backend: "cpu", deviceIndex: 0 });
  });

  it("leaves an invalid stored preference intact while using fallback", () => {
    const storage = new MemoryStorage();
    storage.setItem(PREFERENCE_KEY, JSON.stringify({ packId: "missing", backend: "cuda" }));
    const value = inventory([pack("cpu", "cpu", "ready")]);

    expect(resolveRuntimeSelection(value, readRuntimePreference(storage)))
      .toEqual({ packId: "cpu", backend: "cpu", deviceIndex: 0 });
    expect(storage.getItem(PREFERENCE_KEY)).toContain("missing");
  });

  it("ignores malformed stored preferences", () => {
    const storage = new MemoryStorage();
    storage.setItem(PREFERENCE_KEY, "not-json");
    expect(readRuntimePreference(storage)).toBeNull();
  });

  it("reads and persists Rust-owned backend selection", async () => {
    const invoke = vi.fn(async (command: string) => command === "get_runtime_selection"
      ? { schemaVersion: 1, activeBackend: "cpu", pendingActivation: null, lastActivationError: null }
      : undefined);
    const service = new RuntimePackService({ invoke: invoke as never });

    expect((await service.getSelection()).activeBackend).toBe("cpu");
    await service.requestActivation("cuda");
    await service.setActive("cpu");

    expect(invoke).toHaveBeenNthCalledWith(2, "request_runtime_activation", { backend: "cuda" });
    expect(invoke).toHaveBeenNthCalledWith(3, "set_active_runtime_backend", { backend: "cpu" });
  });
});
