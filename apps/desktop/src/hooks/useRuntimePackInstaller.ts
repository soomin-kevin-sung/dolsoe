import { useCallback, useEffect, useMemo, useState } from "react";

import {
  RuntimePackService,
  reduceRuntimeInstallState,
  type AvailableRuntimePack,
  type RuntimeInstallState,
} from "../services/runtimePacks";

const defaultService = new RuntimePackService();

export function useRuntimePackInstaller(service: RuntimePackService = defaultService) {
  const [availablePacks, setAvailablePacks] = useState<AvailableRuntimePack[]>([]);
  const [installState, setInstallState] = useState<RuntimeInstallState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setAvailablePacks(await service.listAvailable());
      setError(null);
    } catch (value) {
      setError(String(value));
    } finally {
      setLoading(false);
    }
  }, [service]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void service.subscribeInstallProgress((event) => {
      if (disposed) return;
      setInstallState((current) => reduceRuntimeInstallState(current, event));
      if (event.phase === "installed") void refresh();
    }).then((stop) => { if (disposed) stop(); else unlisten = stop; }).catch((value) => {
      if (!disposed) setError(String(value));
    });
    return () => { disposed = true; unlisten?.(); };
  }, [refresh, service]);

  const install = useCallback(async (packId: string) => {
    setError(null);
    try {
      await service.install(packId);
    } catch (value) {
      setError(String(value));
    }
  }, [service]);

  const cancel = useCallback(async () => {
    try {
      await service.cancelInstall();
    } catch (value) {
      setError(String(value));
    }
  }, [service]);

  const dismiss = useCallback(() => setInstallState(null), []);

  return useMemo(() => ({ availablePacks, installState, error, loading, refresh, install, cancel, dismiss }), [availablePacks, cancel, dismiss, error, install, installState, loading, refresh]);
}
