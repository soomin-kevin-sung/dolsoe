import { invoke } from "@tauri-apps/api/core";

export type RuntimeBackend = "cpu" | "cuda" | "vulkan";
export type RuntimePackStatus = "ready" | "invalid";

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

interface RuntimePackBindings {
  invoke(command: "list_runtime_packs"): Promise<RuntimePackInventory>;
}

const tauriRuntimePackBindings: RuntimePackBindings = { invoke };

export const PREFERENCE_KEY = "local-llm-wiki.runtime-pack";

function isBackend(value: unknown): value is RuntimeBackend {
  return value === "cpu" || value === "cuda" || value === "vulkan";
}

export function readRuntimePreference(storage: Storage): RuntimePreference | null {
  const raw = storage.getItem(PREFERENCE_KEY);
  if (!raw) return null;
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

interface RuntimeTransition {
  modelPath: string | null;
  unload(): Promise<void>;
  persist(selection: RuntimeSelection): void;
  load(modelPath: string, selection: RuntimeSelection): Promise<void>;
}

export async function applyRuntimeSelection(
  selection: RuntimeSelection,
  transition: RuntimeTransition,
): Promise<void> {
  if (!transition.modelPath) {
    transition.persist(selection);
    return;
  }
  await transition.unload();
  transition.persist(selection);
  await transition.load(transition.modelPath, selection);
}

export class RuntimePackService {
  constructor(private readonly bindings: RuntimePackBindings = tauriRuntimePackBindings) {}

  list(): Promise<RuntimePackInventory> {
    return this.bindings.invoke("list_runtime_packs");
  }
}
