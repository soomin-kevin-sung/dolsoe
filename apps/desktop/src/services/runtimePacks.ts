import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type RuntimeBackend = "cpu" | "cuda" | "vulkan";
export type RuntimePackStatus = "ready" | "not-installed" | "replacement-pending" | "repair-required" | "unavailable";

export interface RuntimeDevice {
  index: number;
  id: string;
  name: string;
  vendor: string;
}

export interface RuntimePack {
  id: string;
  backend: RuntimeBackend | null;
  status: RuntimePackStatus;
  runtimeVersion: string | null;
  llamaCppCommit: string | null;
  abiMajor: number | null;
  abiMinor: number | null;
  devices: RuntimeDevice[];
  error: string | null;
}

export interface RuntimePackInventory {
  packs: RuntimePack[];
  fallbackPackId: string | null;
}

export interface RuntimePreference {
  packId: string;
  backend: RuntimeBackend;
}

export interface RuntimeSelection extends RuntimePreference {
  deviceIndex: number;
}

export interface RuntimeSelectionState {
  schemaVersion: number;
  activeBackend: RuntimeBackend;
  pendingActivation: RuntimeBackend | null;
  lastActivationError: string | null;
}

export type RuntimeInstallPhase = "downloading" | "verifying" | "installing" | "installed" | "cancelled" | "failed";

export interface AvailableRuntimePack {
  id: string;
  backend: RuntimeBackend;
  releaseVersion: string;
  sizeBytes: number;
  llamaCppRelease: string;
  llamaCppCommit: string;
  installed: boolean;
}

export interface RuntimeInstallProgressEvent {
  packId: string;
  phase: RuntimeInstallPhase;
  downloadedBytes: number;
  totalBytes: number;
  error: string | null;
  restartRequired?: boolean | null;
}

export interface RuntimeInstallState extends RuntimeInstallProgressEvent {
  progress: number;
}

interface RuntimePackBindings {
  invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
  listen?<T>(event: string, handler: (event: { payload: T }) => void): Promise<UnlistenFn>;
}

const tauriRuntimePackBindings: RuntimePackBindings = { invoke, listen };

export const PREFERENCE_KEY = "dolsoe.runtime-pack";
export const LEGACY_PREFERENCE_KEY = "local-llm-wiki.runtime-pack";

export function formatBytes(value: number): string {
  return value >= 1024 ** 3
    ? `${(value / 1024 ** 3).toFixed(1)} GB`
    : `${Math.max(1, Math.round(value / 1024 ** 2))} MB`;
}

function isBackend(value: unknown): value is RuntimeBackend {
  return value === "cpu" || value === "cuda" || value === "vulkan";
}

export function readRuntimePreference(storage: Storage): RuntimePreference | null {
  const raw = storage.getItem(PREFERENCE_KEY) ?? storage.getItem(LEGACY_PREFERENCE_KEY);
  if (!raw) return null;
  if (!storage.getItem(PREFERENCE_KEY)) storage.setItem(PREFERENCE_KEY, raw);
  try {
    const value = JSON.parse(raw) as Partial<RuntimePreference>;
    return typeof value.packId === "string" && isBackend(value.backend)
      ? { packId: value.packId, backend: value.backend }
      : null;
  } catch {
    return null;
  }
}

export function writeRuntimePreference(storage: Storage, value: RuntimeSelection): void {
  storage.setItem(PREFERENCE_KEY, JSON.stringify({ packId: value.packId, backend: value.backend }));
}

function selectionFor(pack: RuntimePack | undefined, backend?: RuntimeBackend): RuntimeSelection | null {
  if (!pack || pack.status !== "ready" || !pack.backend || (backend && pack.backend !== backend)) return null;
  const device = pack.devices[0];
  return device ? { packId: pack.id, backend: pack.backend, deviceIndex: device.index } : null;
}

export function resolveRuntimeSelection(
  inventory: RuntimePackInventory,
  preference: RuntimePreference | null,
): RuntimeSelection | null {
  if (preference) {
    const selected = selectionFor(
      inventory.packs.find((pack) => pack.id === preference.packId),
      preference.backend,
    );
    if (selected) return selected;
  }
  return selectionFor(inventory.packs.find((pack) => pack.id === inventory.fallbackPackId));
}

export function selectionForBackend(
  inventory: RuntimePackInventory,
  backend: RuntimeBackend,
): RuntimeSelection | null {
  return selectionFor(inventory.packs.find((pack) => pack.id === backend), backend);
}

interface RuntimeTransition {
  modelPath: string | null;
  unload(): Promise<void>;
  persist(selection: RuntimeSelection): void | Promise<void>;
  load(modelPath: string, selection: RuntimeSelection): Promise<void>;
}

export async function applyRuntimeSelection(
  selection: RuntimeSelection,
  transition: RuntimeTransition,
): Promise<void> {
  if (!transition.modelPath) {
    await transition.persist(selection);
    return;
  }
  await transition.unload();
  await transition.persist(selection);
  await transition.load(transition.modelPath, selection);
}

export function reduceRuntimeInstallState(
  _current: RuntimeInstallState | null,
  event: RuntimeInstallProgressEvent,
): RuntimeInstallState {
  const progress = event.totalBytes > 0
    ? Math.min(100, Math.max(0, Math.round((event.downloadedBytes / event.totalBytes) * 100)))
    : event.phase === "installed" ? 100 : 0;
  return { ...event, progress };
}

export function canInstallRuntimePack(
  packId: string,
  installed: boolean,
  state: RuntimeInstallState | null,
): boolean {
  if (installed) return false;
  if (!state) return true;
  if (["downloading", "verifying", "installing"].includes(state.phase)) return false;
  if (state.packId !== packId) return true;
  return state.phase === "failed" || state.phase === "cancelled";
}

export class RuntimePackService {
  constructor(private readonly bindings: RuntimePackBindings = tauriRuntimePackBindings) {}

  list(): Promise<RuntimePackInventory> {
    return this.bindings.invoke("list_runtime_packs") as Promise<RuntimePackInventory>;
  }

  listAvailable(): Promise<AvailableRuntimePack[]> {
    return this.bindings.invoke("list_available_runtime_packs") as Promise<AvailableRuntimePack[]>;
  }

  install(packId: string): Promise<void> {
    return this.bindings.invoke("install_runtime_pack", { packId }) as Promise<void>;
  }

  cancelInstall(): Promise<void> {
    return this.bindings.invoke("cancel_runtime_pack_install") as Promise<void>;
  }

  getSelection(): Promise<RuntimeSelectionState> {
    return this.bindings.invoke("get_runtime_selection") as Promise<RuntimeSelectionState>;
  }

  requestActivation(backend: RuntimeBackend): Promise<void> {
    return this.bindings.invoke("request_runtime_activation", { backend }) as Promise<void>;
  }

  setActive(backend: RuntimeBackend): Promise<void> {
    return this.bindings.invoke("set_active_runtime_backend", { backend }) as Promise<void>;
  }

  restart(): Promise<void> {
    return this.bindings.invoke("restart_runtime_app") as Promise<void>;
  }

  subscribeInstallProgress(handler: (event: RuntimeInstallProgressEvent) => void): Promise<UnlistenFn> {
    if (!this.bindings.listen) throw new Error("runtime pack event listener is unavailable");
    return this.bindings.listen<RuntimeInstallProgressEvent>(
      "runtime-pack-install-progress",
      (event) => handler(event.payload),
    );
  }
}
