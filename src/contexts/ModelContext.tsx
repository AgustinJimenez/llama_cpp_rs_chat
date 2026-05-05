import { createContext, useContext, useCallback, useMemo, useState, type ReactNode } from 'react';
import { toast } from 'react-hot-toast';
import { useTranslation } from 'react-i18next';

const TOAST_DELAY_MS = 1500;

import { useModel, type LoadingAction } from '../hooks/useModel';
import type { SamplerConfig, ToolTags } from '../types';
import { logToastError } from '../utils/toastLogger';

/** Per-provider parameter values stored as JSON in localStorage */
export type ProviderParamsMap = Record<string, Record<string, unknown>>;

interface ModelStatus {
  loaded: boolean;
  loading_progress?: number;
  generating?: boolean;
  active_conversation_id?: string;
  status_message?: string;
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

// eslint-disable-next-line @typescript-eslint/ban-types
export type ActiveProvider = 'local' | 'claude_code' | 'codex' | (string & {});

interface ModelContextValue {
  status: ModelStatus;
  isLoading: boolean;
  loadingAction: LoadingAction;
  hasStatusError: boolean;
  /** Clean display name derived from model_path (e.g. "gemma-3-12b-it-Q8_0") */
  modelName: string;
  loadModel: (
    modelPath: string,
    config?: SamplerConfig,
  ) => Promise<{ success: boolean; message: string }>;
  unloadModel: () => Promise<void>;
  forceUnload: () => Promise<void>;
  refreshStatus: () => Promise<void>;
  /** Active provider: 'local' (llama.cpp) or CLI-backed provider */
  activeProvider: ActiveProvider;
  /** Active remote/provider model selection */
  activeProviderModel: string;
  /** Switch to a remote provider with a specific model */
  setRemoteProvider: (provider: string, model: string) => void;
  /** Switch back to local provider */
  setLocalProvider: () => void;
  /** Per-provider parameter overrides (temperature, thinking, etc.) */
  providerParams: ProviderParamsMap;
  /** Update params for a specific provider */
  setProviderParamsFor: (providerId: string, params: Record<string, unknown>) => void;
  /** Get params for the currently active provider */
  activeProviderParams: Record<string, unknown>;
}

const ModelContext = createContext<ModelContextValue | null>(null);

export const ModelProvider = ({ children }: { children: ReactNode }) => {
  const { t } = useTranslation();
  const {
    status,
    isLoading,
    loadingAction,
    hasStatusError,
    loadModel: loadModelRaw,
    unloadModel: unloadModelRaw,
    hardUnload,
    refreshStatus,
  } = useModel();

  const modelName = status.model_path
    ? (status.model_path.split(/[/\\]/).pop() || '').replace(/\.gguf$/i, '')
    : '';

  const loadModel = useCallback(
    async (modelPath: string, config?: SamplerConfig) => {
      const result = await loadModelRaw(modelPath, config);
      if (result.success) {
        toast.success(t('toast.modelLoaded'));
        if (result.visionFailed) {
          setTimeout(() => {
            toast(t('toast.visionFailed'), { icon: '\u26A0\uFE0F', duration: 6000 });
          }, TOAST_DELAY_MS);
        }
      } else {
        const display = t('toast.modelLoadFailed', { message: result.message });
        logToastError('ModelContext.loadModel', display);
        toast.error(display, { duration: 5000 });
      }
      return result;
    },
    [loadModelRaw, t],
  );

  const unloadModel = useCallback(async () => {
    // For remote providers, just clear the provider state (no local model to unload)
    const currentProvider = localStorage.getItem('activeProvider') || 'local';
    if (currentProvider !== 'local') {
      setActiveProvider('local');
      localStorage.setItem('activeProvider', 'local');
      toast.success(t('toast.providerDisconnected'));
      return;
    }
    const result = await unloadModelRaw();
    if (result.success) {
      toast.success(t('toast.modelUnloaded'));
    } else {
      const display = t('toast.modelUnloadFailed', { message: result.message });
      logToastError('ModelContext.unloadModel', display);
      toast.error(display, { duration: 5000 });
    }
  }, [unloadModelRaw, t]);

  const forceUnload = useCallback(async () => {
    await hardUnload();
    toast(t('toast.forceUnloaded'), { icon: '🧹' });
  }, [hardUnload, t]);

  // Provider state — persisted in localStorage
  const [activeProvider, setActiveProvider] = useState<ActiveProvider>(
    () => (localStorage.getItem('activeProvider') as ActiveProvider) || 'local',
  );
  const [activeProviderModel, setActiveProviderModel] = useState(() => {
    const provider = (localStorage.getItem('activeProvider') as ActiveProvider) || 'local';
    const saved =
      localStorage.getItem('activeProviderModel') || localStorage.getItem('activeClaudeModel');
    if (saved) return saved;
    return provider === 'codex' ? 'gpt-5' : 'sonnet';
  });

  // Provider params — persisted in localStorage per provider
  const [providerParams, setProviderParams] = useState<ProviderParamsMap>(() => {
    try {
      const raw = localStorage.getItem('providerParams');
      return raw ? (JSON.parse(raw) as ProviderParamsMap) : {};
    } catch {
      return {};
    }
  });

  const setProviderParamsFor = useCallback(
    (providerId: string, params: Record<string, unknown>) => {
      setProviderParams((prev) => {
        const next = { ...prev, [providerId]: params };
        localStorage.setItem('providerParams', JSON.stringify(next));
        return next;
      });
    },
    [],
  );

  const activeProviderParams = useMemo(
    () => providerParams[activeProvider] ?? {},
    [providerParams, activeProvider],
  );

  const setRemoteProvider = useCallback((provider: string, model: string) => {
    setActiveProvider(provider as ActiveProvider);
    setActiveProviderModel(model);
    localStorage.setItem('activeProvider', provider);
    localStorage.setItem('activeProviderModel', model);
  }, []);

  const setLocalProvider = useCallback(() => {
    setActiveProvider('local');
    localStorage.setItem('activeProvider', 'local');
  }, []);

  const value = useMemo<ModelContextValue>(
    () => ({
      status,
      isLoading,
      loadingAction,
      hasStatusError,
      modelName,
      loadModel,
      unloadModel,
      forceUnload,
      refreshStatus,
      activeProvider,
      activeProviderModel,
      setRemoteProvider,
      setLocalProvider,
      providerParams,
      setProviderParamsFor,
      activeProviderParams,
    }),
    [
      status,
      isLoading,
      loadingAction,
      hasStatusError,
      modelName,
      loadModel,
      unloadModel,
      forceUnload,
      refreshStatus,
      activeProvider,
      activeProviderModel,
      setRemoteProvider,
      setLocalProvider,
      providerParams,
      setProviderParamsFor,
      activeProviderParams,
    ],
  );

  return <ModelContext.Provider value={value}>{children}</ModelContext.Provider>;
};

// eslint-disable-next-line react-refresh/only-export-components
export function useModelContext() {
  const ctx = useContext(ModelContext);
  if (!ctx) throw new Error('useModelContext must be used within ModelProvider');
  return ctx;
}
