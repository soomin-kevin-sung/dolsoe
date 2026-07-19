import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { NativeRuntimeService, type LlmEventDto, type LoadModelRequest, type SubmitRequest } from "../services/nativeRuntime";
import { applyNativeEvent, createNativeState, nativeReducer, TokenDecoders } from "../services/nativeState";

export interface NativeOptions {
  contextSize: number;
  batchSize: number;
  physicalBatchSize: number;
  threads: number;
  maxNewTokens: number;
  temperature: number;
  topP: number;
  seed: number;
}

export const defaultNativeOptions: NativeOptions = {
  contextSize: 4096,
  batchSize: 512,
  physicalBatchSize: 128,
  threads: Math.min(8, Math.max(1, navigator.hardwareConcurrency || 4)),
  maxNewTokens: 256,
  temperature: 0.8,
  topP: 0.95,
  seed: -1,
};

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function useNativeRuntime(onEvent?: (event: LlmEventDto) => void) {
  const service = useMemo(() => new NativeRuntimeService(), []);
  const decoders = useRef(new TokenDecoders());
  const cancelWhenAccepted = useRef(false);
  const eventObserver = useRef(onEvent);
  const [state, setState] = useState(createNativeState);
  const [options, setOptions] = useState(defaultNativeOptions);

  useEffect(() => {
    eventObserver.current = onEvent;
  }, [onEvent]);

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void (async () => {
      try {
        cleanup = await service.subscribe((event) => {
          if (disposed) return;
          eventObserver.current?.(event);
          setState((current) => applyNativeEvent(current, event, decoders.current));
          if (["done", "cancelled", "error"].includes(event.kind)) {
            void service.getMetrics()
              .then((metrics) => { if (!disposed) setState((current) => nativeReducer(current, { type: "metrics", metrics })); })
              .catch(() => undefined);
          }
        });
        const status = await service.getStatus();
        if (!disposed) setState((current) => nativeReducer(current, { type: "status", status }));
      } catch (error) {
        if (!disposed) setState((current) => nativeReducer(current, { type: "load-failed", error: errorText(error) }));
      }
    })();
    return () => {
      disposed = true;
      cleanup?.();
      decoders.current.clear();
    };
  }, [service]);

  const loadPath = useCallback(async (modelPath: string) => {
    setState((current) => nativeReducer(current, { type: "load-started", modelPath }));
    const request: LoadModelRequest = {
      runtimePackId: "cpu-dev",
      backend: "cpu",
      deviceIndex: 0,
      modelPath,
      contextSize: options.contextSize,
      batchSize: options.batchSize,
      physicalBatchSize: options.physicalBatchSize,
      threads: options.threads,
      useMmap: true,
    };
    try {
      const status = await service.loadModel(request);
      setState((current) => nativeReducer(current, { type: "status", status }));
    } catch (error) {
      setState((current) => nativeReducer(current, { type: "load-failed", error: errorText(error) }));
    }
  }, [options, service]);

  const chooseModel = useCallback(async () => {
    const modelPath = await service.chooseModel();
    if (modelPath) await loadPath(modelPath);
  }, [loadPath, service]);

  const submit = useCallback(async (prompt: string) => {
    cancelWhenAccepted.current = false;
    setState((current) => nativeReducer(current, { type: "submit-started", prompt }));
    const request: SubmitRequest = {
      prompt,
      maxNewTokens: options.maxNewTokens,
      temperature: options.temperature,
      topP: options.topP,
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
      await service.cancel(state.activeRequestHandle).catch((error) => {
        setState((current) => nativeReducer(current, { type: "submit-failed", error: errorText(error) }));
      });
    } else if (state.pendingSubmit) {
      cancelWhenAccepted.current = true;
    }
  }, [service, state.activeRequestHandle, state.pendingSubmit]);

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

  return { state, options, setOptions, chooseModel, submit, stop, reset, unload, reload };
}
