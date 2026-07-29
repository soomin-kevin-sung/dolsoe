import { describe, expect, it } from "vitest";

import { resolveHomeReadiness } from "./homeReadiness";

const base = {
  runtimePhase: "no-model" as const,
  runtimePacksInitialized: true,
  installState: null,
  distributionLoading: false,
  distributionError: null,
  runtimeRecovery: false,
};

describe("resolveHomeReadiness", () => {
  it("prioritizes an active CPU installation over runtime and model state", () => {
    expect(resolveHomeReadiness({
      ...base,
      runtimePhase: "ready",
      cpuPackStatus: "ready",
      installState: { packId: "cpu", phase: "verifying", downloadedBytes: 10, totalBytes: 10, error: null, progress: 100 },
    })).toBe("runtime-verifying");
  });

  it("routes missing CPU catalog and model states to distinct actions", () => {
    expect(resolveHomeReadiness({ ...base, distributionLoading: true })).toBe("runtime-checking");
    expect(resolveHomeReadiness({ ...base, distributionError: "network timeout" })).toBe("runtime-failed-network");
    expect(resolveHomeReadiness({ ...base, cpuPackStatus: "ready" })).toBe("model-missing");
    expect(resolveHomeReadiness({ ...base, cpuPackStatus: "ready", runtimePhase: "ready" })).toBe("ready");
  });

  it("keeps startup in checking state until the local runtime inventory is known", () => {
    expect(resolveHomeReadiness({
      ...base,
      runtimePacksInitialized: false,
      distributionError: "runtime distribution request failed",
    })).toBe("runtime-checking");
  });

  it("treats a missing CPU pack as onboarding even when the host reports recovery", () => {
    expect(resolveHomeReadiness({
      ...base,
      cpuPackStatus: "not-installed",
      runtimeRecovery: true,
    })).toBe("runtime-missing");
    expect(resolveHomeReadiness({
      ...base,
      cpuPackStatus: "repair-required",
      runtimeRecovery: true,
    })).toBe("runtime-failed-recovery");
  });

  it("requests restart only when the completed install replaced a loaded runtime", () => {
    const completed = { packId: "cpu", phase: "installed" as const, downloadedBytes: 0, totalBytes: 0, error: null, progress: 100 };
    expect(resolveHomeReadiness({
      ...base,
      cpuPackStatus: "ready",
      installState: { ...completed, restartRequired: false },
    })).toBe("model-missing");
    expect(resolveHomeReadiness({
      ...base,
      cpuPackStatus: "replacement-pending",
      installState: { ...completed, restartRequired: true },
    })).toBe("runtime-installed");
  });
});
