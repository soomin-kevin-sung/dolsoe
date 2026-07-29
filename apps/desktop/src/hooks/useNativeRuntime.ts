import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  NativeRuntimeService,
  type AgentActivityEventDto,
  type LlmEventDto,
  type LoadModelRequest,
  type SubmitRequest,
} from "../services/nativeRuntime";
import { applyNativeEvent, createNativeState, nativeReducer, TokenDecoders } from "../services/nativeState";
import {
  RuntimePackService,
  selectionForBackend,
  type RuntimeBackend,
  type RuntimePack,
  type RuntimeSelection,
} from "../services/runtimePacks";

export interface NativeOptions {
  contextSize: number;
  batchSize: number;
  physicalBatchSize: number;
  threads: number;
  useMmap: boolean;
  maxNewTokens: number;
  temperature: number;
  topK: number;
  topP: number;
  minP: number;
  repeatLastN: number;
  repeatPenalty: number;
  frequencyPenalty: number;
  presencePenalty: number;
  stopSequences: string[];
  seed: number;
}

export const defaultNativeOptions: NativeOptions = {
  contextSize: 4096,
  batchSize: 512,
  physicalBatchSize: 128,
  threads: Math.min(8, Math.max(1, navigator.hardwareConcurrency || 4)),
  useMmap: true,
  maxNewTokens: 256,
  temperature: 0.8,
  topK: 40,
  topP: 0.95,
  minP: 0.05,
  repeatLastN: 64,
  repeatPenalty: 1.1,
  frequencyPenalty: 0,
  presencePenalty: 0,
  stopSequences: [],
  seed: -1,
};

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function useNativeRuntime(
  onEvent?: (event: LlmEventDto) => void,
  onAgentActivity?: (event: AgentActivityEventDto) => void,
) {
  const service = useMemo(() => new NativeRuntimeService(), []);
  const runtimePackService = useMemo(() => new RuntimePackService(), []);
  const decoders = useRef(new TokenDecoders());
  const cancelWhenAccepted = useRef(false);
  const eventObserver = useRef(onEvent);
  const activityObserver = useRef(onAgentActivity);
  const [state, setState] = useState(createNativeState);
  const [options, setOptions] = useState(defaultNativeOptions);
  const [runtimePacks, setRuntimePacks] = useState<RuntimePack[]>([]);
  const [runtimePackError, setRuntimePackError] = useState<string | null>(null);
  const [appliedRuntime, setAppliedRuntime] = useState<RuntimeSelection | null>(null);
  const [pendingRuntime, setPendingRuntime] = useState<RuntimeSelection | null>(null);

  useEffect(() => {
    eventObserver.current = onEvent;
  }, [onEvent]);

  useEffect(() => {
    activityObserver.current = onAgentActivity;
  }, [onAgentActivity]);

  useEffect(() => {
    let disposed = false;
    let cleanupEvents: (() => void) | undefined;
    let cleanupActivity: (() => void) | undefined;
    let cleanupHost: (() => void) | undefined;
    void (async () => {
      try {
        cleanupEvents = await service.subscribe((event) => {
          if (disposed) return;
          eventObserver.current?.(event);
          setState((current) => applyNativeEvent(current, event, decoders.current));
          if (["done", "cancelled", "error"].includes(event.kind)) {
            void service.getMetrics()
              .then((metrics) => { if (!disposed) setState((current) => nativeReducer(current, { type: "metrics", metrics })); })
              .catch(() => undefined);
          }
        });
        cleanupActivity = await service.subscribeAgentActivity((event) => {
          if (!disposed) activityObserver.current?.(event);
        });
        cleanupHost = await service.subscribeHostReady((status) => {
          if (!disposed) setState((current) => nativeReducer(current, { type: "status", status }));
        });
        const status = await service.getStatus();
        if (!disposed) setState((current) => nativeReducer(current, { type: "status", status }));
      } catch (error) {
        if (!disposed) setState((current) => nativeReducer(current, { type: "load-failed", error: errorText(error) }));
      }
    })();
    return () => {
      disposed = true;
      cleanupEvents?.();
      cleanupActivity?.();
      cleanupHost?.();
      decoders.current.clear();
    };
  }, [service]);

  const refreshRuntimePacks = useCallback(async () => {
    try {
      const [inventory, selectionState] = await Promise.all([
        runtimePackService.list(),
        runtimePackService.getSelection(),
      ]);
      const selection = selectionForBackend(inventory, selectionState.activeBackend)
        ?? selectionForBackend(inventory, "cpu");
      setRuntimePacks(inventory.packs);
      setAppliedRuntime(selection);
      setPendingRuntime(selection);
      setRuntimePackError(null);
    } catch (error) {
      setRuntimePackError(errorText(error));
    }
  }, [runtimePackService]);

  useEffect(() => {
    void refreshRuntimePacks();
  }, [refreshRuntimePacks]);

  const loadModel = useCallback((modelPath: string, selection: RuntimeSelection, loadOptions: NativeOptions) => {
    const request: LoadModelRequest = {
      runtimePackId: selection.packId,
      backend: selection.backend,
      deviceIndex: selection.deviceIndex,
      modelPath,
      contextSize: loadOptions.contextSize,
      batchSize: loadOptions.batchSize,
      physicalBatchSize: loadOptions.physicalBatchSize,
      threads: loadOptions.threads,
      useMmap: loadOptions.useMmap,
    };
    return service.loadModel(request);
  }, [service]);

  const loadPath = useCallback(async (modelPath: string, selection = appliedRuntime, loadOptions = options) => {
    setState((current) => nativeReducer(current, { type: "load-started", modelPath }));
    if (!selection) {
      setState((current) => nativeReducer(current, { type: "load-failed", error: "사용 가능한 런타임 팩이 없습니다." }));
      return false;
    }
    try {
      const status = await loadModel(modelPath, selection, loadOptions);
      setState((current) => nativeReducer(current, { type: "status", status }));
      return true;
    } catch (error) {
      setState((current) => nativeReducer(current, { type: "load-failed", error: errorText(error) }));
      return false;
    }
  }, [appliedRuntime, loadModel, options]);

  const chooseModelPath = useCallback(() => service.chooseModel(), [service]);

  const chooseModel = useCallback(async () => {
    const modelPath = await chooseModelPath();
    if (modelPath) await loadPath(modelPath);
  }, [chooseModelPath, loadPath]);

  const submit = useCallback(async (
    conversationId: string,
    agentRunId: string,
    agentStepId: string,
    prompt: string,
    messages: SubmitRequest["messages"] = [{ role: "user", content: prompt }],
  ) => {
    cancelWhenAccepted.current = false;
    setState((current) => nativeReducer(current, { type: "submit-started", prompt }));
    const request: SubmitRequest = {
      conversationId,
      agentRunId,
      agentStepId,
      prompt,
      messages,
      maxNewTokens: options.maxNewTokens,
      temperature: options.temperature,
      topK: options.topK,
      topP: options.topP,
      minP: options.minP,
      repeatLastN: options.repeatLastN,
      repeatPenalty: options.repeatPenalty,
      frequencyPenalty: options.frequencyPenalty,
      presencePenalty: options.presencePenalty,
      stopSequences: options.stopSequences,
      seed: options.seed,
    };
    try {
      const response = await service.submit(request);
      setState((current) => nativeReducer(current, { type: "submit-accepted", requestHandle: response.requestHandle }));
      if (cancelWhenAccepted.current) await service.cancel(response.requestHandle);
      return response;
    } catch (error) {
      setState((current) => nativeReducer(current, { type: "submit-failed", error: errorText(error) }));
      throw error;
    }
  }, [options, service]);

  const stop = useCallback(async () => {
    if (state.activeRequestHandle) {
      try {
        await service.cancel(state.activeRequestHandle);
      } catch (error) {
        setState((current) => nativeReducer(current, { type: "submit-failed", error: errorText(error) }));
        throw error;
      }
    } else if (state.pendingSubmit) {
      cancelWhenAccepted.current = true;
    }
  }, [service, state.activeRequestHandle, state.pendingSubmit]);

  const reportError = useCallback((error: unknown) => {
    setState((current) => nativeReducer(current, { type: "load-failed", error: errorText(error) }));
  }, []);

  const reset = useCallback(async () => {
    if (state.activeRequestHandle) await service.cancel(state.activeRequestHandle).catch(() => undefined);
    decoders.current.clear();
    setState((current) => nativeReducer(current, { type: "reset" }));
  }, [service, state.activeRequestHandle]);

  const unload = useCallback(async () => {
    try {
      const status = await service.unloadModel();
      decoders.current.clear();
      setState((current) => nativeReducer(current, { type: "status", status }));
    } catch (error) {
      setState((current) => nativeReducer(current, { type: "load-failed", error: errorText(error) }));
    }
  }, [service]);

  const reload = useCallback(async () => {
    if (state.modelPath) await loadPath(state.modelPath);
  }, [loadPath, state.modelPath]);

  const setPendingBackend = useCallback((backend: RuntimeBackend) => {
    const pack = runtimePacks.find((candidate) => candidate.status === "ready"
      && candidate.backend === backend
      && candidate.devices.length > 0);
    const device = pack?.devices[0];
    setPendingRuntime(pack && device
      ? { packId: pack.id, backend, deviceIndex: device.index }
      : null);
  }, [runtimePacks]);

  const applyConfiguration = useCallback(async (
    nextOptions: NativeOptions,
    backend: RuntimeBackend,
    targetModelPath: string | null = state.modelPath,
  ) => {
    const targetSelection = selectionForBackend({ packs: runtimePacks, fallbackPackId: null }, backend);
    if (!targetSelection) {
      setState((current) => nativeReducer(current, { type: "load-failed", error: `${backend} 백엔드를 사용할 수 없습니다.` }));
      return false;
    }

    const previousSelection = appliedRuntime;
    const previousOptions = options;
    const previousModelPath = state.modelPath;

    if (!targetModelPath) {
      try {
        await runtimePackService.setActive(targetSelection.backend);
        setAppliedRuntime(targetSelection);
        setPendingRuntime(targetSelection);
        setOptions(nextOptions);
        return true;
      } catch (error) {
        setState((current) => nativeReducer(current, { type: "load-failed", error: errorText(error) }));
        return false;
      }
    }

    setState((current) => nativeReducer(current, { type: "load-started", modelPath: targetModelPath }));
    try {
      if (previousModelPath) {
        await service.unloadModel();
        decoders.current.clear();
      }
      const status = await loadModel(targetModelPath, targetSelection, nextOptions);
      await runtimePackService.setActive(targetSelection.backend);
      setAppliedRuntime(targetSelection);
      setPendingRuntime(targetSelection);
      setOptions(nextOptions);
      setState((current) => nativeReducer(current, { type: "status", status }));
      return true;
    } catch (error) {
      if (previousSelection && previousModelPath) {
        try {
          const rollbackStatus = await loadModel(previousModelPath, previousSelection, previousOptions);
          await runtimePackService.setActive(previousSelection.backend);
          setAppliedRuntime(previousSelection);
          setPendingRuntime(previousSelection);
          setOptions(previousOptions);
          setState((current) => nativeReducer(current, { type: "status", status: rollbackStatus }));
          return false;
        } catch (rollbackError) {
          setState((current) => nativeReducer(current, {
            type: "load-failed",
            error: `${errorText(error)} 이전 설정 복구도 실패했습니다: ${errorText(rollbackError)}`,
          }));
          return false;
        }
      }
      setState((current) => nativeReducer(current, { type: "load-failed", error: errorText(error) }));
      return false;
    }
  }, [appliedRuntime, loadModel, options, runtimePackService, runtimePacks, service, state.modelPath]);

  const restartApp = useCallback(() => runtimePackService.restart(), [runtimePackService]);

  return {
    state,
    options,
    setOptions,
    runtimePacks,
    runtimePackError,
    appliedRuntime,
    pendingRuntime,
    refreshRuntimePacks,
    setPendingBackend,
    applyConfiguration,
    restartApp,
    reportError,
    chooseModelPath,
    chooseModel,
    submit,
    stop,
    reset,
    unload,
    reload,
  };
}
