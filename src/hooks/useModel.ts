import { useState, useEffect, useCallback, useRef } from 'react';

const DEFAULT_MEMORY_USAGE_MB = 512;
const LOADING_POLL_INTERVAL_MS = 200;
const STATUS_POLL_INTERVAL_MS = 5000;

import type { SamplerConfig, ToolTags } from '../types';
import {
  getModelStatus,
  loadModel as loadModelCmd,
  unloadModel as unloadModelCmd,
  hardUnload as hardUnloadCmd,
  saveConfig,
} from '../utils/tauriCommands';

interface ModelStatus {
  loaded: boolean;
  loading?: boolean;
  loading_progress?: number;
  generating?: boolean;
  model_path: string | null;
  last_used: string | null;
  memory_usage_mb: number | null;
  has_vision?: boolean;
  tool_tags?: ToolTags;
  gpu_layers?: number;
  block_count?: number;
  system_prompt_tokens?: number;
  tool_definitions_tokens?: number;
  context_size?: number;
}

interface ModelResponse {
  success: boolean;
  message: string;
  status?: ModelStatus;
}

export type LoadingAction = 'loading' | 'unloading' | null;

// eslint-disable-next-line max-lines-per-function
export const useModel = () => {
  const [status, setStatus] = useState<ModelStatus>({
    loaded: false,
    model_path: null,
    last_used: null,
    memory_usage_mb: null,
  });
  const [isLoading, setIsLoading] = useState(false);
  const [loadingAction, setLoadingAction] = useState<LoadingAction>(null);
  const [error, setError] = useState<string | null>(null);
  const [hasStatusError, setHasStatusError] = useState(false);

  const hardUnload = useCallback(async () => {
    try {
      await hardUnloadCmd();
      setStatus({
        loaded: false,
        model_path: null,
        last_used: null,
        memory_usage_mb: null,
      });
    } catch (err) {
      console.warn('Hard unload failed', err);
    }
  }, []);

  const fetchStatus = useCallback(async () => {
    try {
      const data = (await getModelStatus()) as ModelStatus;
      setStatus(data);
      setError(null);
      setHasStatusError(false);
      // Sync loading state from server (e.g. after browser refresh during load)
      if (data.loading && !data.loaded) {
        setIsLoading(true);
        setLoadingAction('loading');
      }
    } catch (err) {
      setError('Network error while fetching model status');
      console.error('Model status fetch error:', err);
      setHasStatusError(true);
    }
  }, []);

  const loadModel = useCallback(
    async (modelPath: string, config?: SamplerConfig) => {
      setIsLoading(true);
      setLoadingAction('loading');
      setError(null);

      const refreshStatusSafe = async () => {
        try {
          await fetchStatus();
        } catch {
          // ignore secondary failures to keep UX responsive
        }
      };

      try {
        // First update the configuration if provided
        if (config) {
          await saveConfig({
            ...config,
            model_path: modelPath,
          });
        }

        // Then load the model (pass gpu_layers and mmproj_path from config if available)
        const data: ModelResponse = await loadModelCmd(
          modelPath,
          config?.gpu_layers,
          config?.mmproj_path,
        );

        if (data.success) {
          // If backend returns no status or an incorrect unloaded status, synthesize a "loaded" status
          const nowSeconds = Math.floor(Date.now() / 1000).toString();
          const coercedStatus = data.status ?? {
            loaded: true,
            model_path: modelPath,
            last_used: nowSeconds,
            memory_usage_mb: 512,
          };

          if (!coercedStatus.loaded) {
            setStatus({
              ...coercedStatus,
              loaded: true,
              model_path: modelPath,
              last_used: nowSeconds,
              memory_usage_mb: coercedStatus.memory_usage_mb ?? DEFAULT_MEMORY_USAGE_MB,
            });
          } else {
            setStatus(coercedStatus);
          }
          setError(null);
          setHasStatusError(false);
          // Detect if mmproj was requested but vision init failed
          const visionFailed = !!(config?.mmproj_path && data.status?.has_vision === false);
          return { success: true, message: data.message, visionFailed };
        }
        setError(data.message);
        setHasStatusError(true);
        // Refresh status even when loading fails to ensure UI is accurate
        await refreshStatusSafe();
        return { success: false, message: data.message };
      } catch (err) {
        const errorMessage = err instanceof Error ? err.message : 'Unknown error occurred';
        setError(errorMessage);
        setHasStatusError(true);
        // Refresh status on error to ensure UI is accurate
        await refreshStatusSafe();
        return { success: false, message: errorMessage };
      } finally {
        setIsLoading(false);
        setLoadingAction(null);
      }
    },
    [fetchStatus],
  );

  const unloadModel = useCallback(async () => {
    setIsLoading(true);
    setLoadingAction('unloading');
    setError(null);

    try {
      const data: ModelResponse = await unloadModelCmd();

      if (data.success && data.status) {
        setStatus(data.status);
        setError(null);
        setHasStatusError(false);
        return { success: true, message: data.message };
      }
      setError(data.message);
      setHasStatusError(true);
      return { success: false, message: data.message };
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Unknown error occurred';
      setError(errorMessage);
      setHasStatusError(true);
      // Refresh status on error to ensure UI is accurate
      await fetchStatus();
      return { success: false, message: errorMessage };
    } finally {
      setIsLoading(false);
      setLoadingAction(null);
    }
  }, [fetchStatus]);

  // Fetch status on mount
  useEffect(() => {
    fetchStatus();
  }, [fetchStatus]);

  // Poll status while loading to get progress updates and detect completion
  useEffect(() => {
    if (!isLoading || loadingAction !== 'loading') return;
    const interval = setInterval(async () => {
      try {
        const data = (await getModelStatus()) as ModelStatus;
        setStatus(data);
        if (data.loaded || (!data.loading && !isLoading)) {
          setIsLoading(false);
          setLoadingAction(null);
        }
      } catch {
        // Keep polling on error
      }
    }, LOADING_POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [isLoading, loadingAction]);

  // Poll status periodically to detect active generation (for sidebar indicator).
  // Slower interval (5s) to avoid hammering the API.
  // Only update state when something actually changed to avoid unnecessary re-renders
  // that close menus, reset scroll, etc.
  const lastStatusJson = useRef('');
  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const data = (await getModelStatus()) as ModelStatus;
        const json = JSON.stringify(data);
        if (json !== lastStatusJson.current) {
          lastStatusJson.current = json;
          setStatus(data);
        }
      } catch {
        // ignore
      }
    }, STATUS_POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, []);

  return {
    status,
    isLoading,
    loadingAction,
    error,
    hasStatusError,
    loadModel,
    unloadModel,
    hardUnload,
    refreshStatus: fetchStatus,
  };
};
