import type { LlmPhase } from "./nativeRuntime";
import type { RuntimeInstallState, RuntimePackStatus } from "./runtimePacks";

export type HomeReadinessKind =
  | "runtime-missing"
  | "runtime-checking"
  | "runtime-downloading"
  | "runtime-verifying"
  | "runtime-installing"
  | "runtime-installed"
  | "runtime-failed-network"
  | "runtime-failed-verification"
  | "runtime-failed-disk"
  | "runtime-failed-recovery"
  | "runtime-failed-unknown"
  | "model-missing"
  | "model-loading"
  | "ready";

export interface HomeReadinessInput {
  runtimePhase: LlmPhase;
  cpuPackStatus?: RuntimePackStatus;
  installState: RuntimeInstallState | null;
  distributionLoading: boolean;
  distributionError: string | null;
  runtimeRecovery: boolean;
}

export function classifyRuntimeFailure(message?: string | null): "network" | "verification" | "disk" | "unknown" {
  const normalized = message?.toLocaleLowerCase() ?? "";
  if (["network", "offline", "timeout", "dns", "connect", "http"].some((token) => normalized.includes(token))) return "network";
  if (["checksum", "sha-256", "sha256", "verify", "hash"].some((token) => normalized.includes(token))) return "verification";
  if (["disk", "space", "enospc"].some((token) => normalized.includes(token))) return "disk";
  return "unknown";
}

export function resolveHomeReadiness(input: HomeReadinessInput): HomeReadinessKind {
  const install = input.installState?.packId === "cpu" ? input.installState : null;
  if (install && ["downloading", "verifying", "installing"].includes(install.phase)) {
    return `runtime-${install.phase}` as HomeReadinessKind;
  }
  if ((install?.phase === "installed" && install.restartRequired === true)
    || input.cpuPackStatus === "replacement-pending") return "runtime-installed";
  if (install?.phase === "failed") return `runtime-failed-${classifyRuntimeFailure(install.error)}`;

  const cpuReady = input.cpuPackStatus === "ready";
  if (!cpuReady) {
    if (input.distributionLoading) return "runtime-checking";
    if (input.distributionError) return `runtime-failed-${classifyRuntimeFailure(input.distributionError)}`;
    if (input.cpuPackStatus === "repair-required" || input.cpuPackStatus === "unavailable") return "runtime-failed-recovery";
    if (input.runtimeRecovery && input.cpuPackStatus !== "not-installed") return "runtime-failed-recovery";
    return "runtime-missing";
  }

  if (input.runtimePhase === "loading") return "model-loading";
  if (input.runtimePhase === "ready" || input.runtimePhase === "streaming") return "ready";
  return "model-missing";
}

export function isCpuReady(kind: HomeReadinessKind): boolean {
  return kind === "model-missing" || kind === "model-loading" || kind === "ready";
}

export function readinessStatus(kind: HomeReadinessKind): { text: string; tone: "none" | "loading" | "pending" | "error" | "ready" } {
  if (kind === "runtime-checking") return { text: "런타임 확인 중", tone: "loading" };
  if (kind === "runtime-downloading") return { text: "런타임 다운로드 중", tone: "loading" };
  if (kind === "runtime-verifying") return { text: "런타임 검증 중", tone: "loading" };
  if (kind === "runtime-installing") return { text: "런타임 설치 중", tone: "loading" };
  if (kind === "runtime-installed") return { text: "재시작 필요", tone: "pending" };
  if (kind.startsWith("runtime-failed-")) {
    return { text: "런타임 필요", tone: kind === "runtime-failed-network" ? "none" : "error" };
  }
  if (kind === "runtime-missing") return { text: "런타임 필요", tone: "none" };
  if (kind === "model-loading") return { text: "모델 로딩 중", tone: "loading" };
  if (kind === "model-missing") return { text: "모델 없음", tone: "none" };
  return { text: "준비됨", tone: "ready" };
}
