import { createContext, useContext, useCallback, useMemo, useState, type ReactNode } from 'react';
import { toast } from 'react-hot-toast';

const TOAST_DELAY_MS = 1500;

import { useModel, type LoadingAction } from '../hooks/useModel';
import type { SamplerConfig, ToolTags } from '../types';
import { logToastError } from '../utils/toastLogger';

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
}

const ModelContext = createContext<ModelContextValue | null>(null);

export const ModelProvider = ({ children }: { children: ReactNode }) => {
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
        toast.success('Model loaded successfully!');
        if (result.visionFailed) {
          setTimeout(() => {
            toast(
              'Vision projector failed to initialize. The model may not support vision, or the mmproj file is incompatible.',
              {
                icon: '\u26A0\uFE0F',
                duration: 6000,
              },
            );
          }, TOAST_DELAY_MS);
        }
      } else {
        const display = `Failed to load model: ${result.message}`;
        logToastError('ModelContext.loadModel', display);
        toast.error(display, { duration: 5000 });
      }
      return result;
    },
    [loadModelRaw],
  );

  const unloadModel = useCallback(async () => {
    const result = await unloadModelRaw();
    if (result.success) {
      toast.success('Model unloaded successfully');
    } else {
      const display = `Failed to unload model: ${result.message}`;
      logToastError('ModelContext.unloadModel', display);
      toast.error(display, { duration: 5000 });
    }
  }, [unloadModelRaw]);

  const forceUnload = useCallback(async () => {
    await hardUnload();
    toast('Force-unloaded backend to free memory', { icon: '🧹' });
  }, [hardUnload]);

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

  const setRemoteProvider = useCallback((provider: string, model: string) => {
    setActiveProvider(provider as ActiveProvider);
    setActiveProviderModel(model);
    localStorage.setItem('activeProvider', provider);
    localStorage.setItem('activeProviderModel', model);
    const providerNames: Record<string, string> = {
      claude_code: 'Claude Code',
      codex: 'Codex CLI',
      groq: 'Groq',
      gemini: 'Gemini',
      sambanova: 'SambaNova',
      cerebras: 'Cerebras',
      openrouter: 'OpenRouter',
      together: 'Together AI',
      deepseek: 'DeepSeek',
      custom_openai: 'Custom OpenAI',
    };
    const providerName = providerNames[provider] || provider;
    toast.success(`Switched to ${providerName} (${model})`);
  }, []);

  const setLocalProvider = useCallback(() => {
    setActiveProvider('local');
    localStorage.setItem('activeProvider', 'local');
    toast.success('Switched to Local Model');
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
