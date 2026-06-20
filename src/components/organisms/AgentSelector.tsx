/* eslint-disable max-lines -- agent selector with full local model config */
import {
  Bot,
  Cpu,
  Cloud,
  Plus,
  Pencil,
  Trash2,
  X,
  ChevronLeft,
  ChevronDown,
  ChevronRight,
  Eye,
  FolderOpen,
  CheckCircle,
  Loader2,
} from 'lucide-react';
import type { MouseEvent } from 'react';
import { useState, useEffect, useMemo, useRef } from 'react';
import { toast } from 'react-hot-toast';

import { Card, CardContent, CardHeader, CardTitle } from '@/components/atoms/card';
import { ModelFileInput, ModelConfigSystemPrompt } from '@/components/molecules';
import type { SystemPromptMode } from '@/components/molecules/ModelConfigSystemPrompt';
import { AdvancedContextSection } from '@/components/organisms/model-config/AdvancedContextSection';
import { MemoryVisualization } from '@/components/organisms/model-config/MemoryVisualization';
import { ModelMetadataDisplay } from '@/components/organisms/model-config/ModelMetadataDisplay';
import { SamplingParametersSection } from '@/components/organisms/model-config/SamplingParametersSection';
import { TagPairsSection } from '@/components/organisms/model-config/TagPairsSection';
import { DEFAULT_PRESET, findPresetByName } from '@/config/modelPresets';
import { useAgentContext } from '@/contexts/AgentContext';
import { useModelContext } from '@/contexts/ModelContext';
import { useSystemResources } from '@/contexts/SystemResourcesContext';
import { useMemoryCalculation } from '@/hooks/useMemoryCalculation';
import { useModelPathValidation } from '@/hooks/useModelPathValidation';
import { useVramOptimizer } from '@/hooks/useVramOptimizer';
import type { Agent, SamplerConfig } from '@/types';
import { isTauriEnv } from '@/utils/tauri';
import { recordAppError, getModelHistory, pickFile } from '@/utils/tauriCommands';

const DEFAULT_CONTEXT_SIZE = 32768;
const DEFAULT_MAX_CONTEXT = 131072;

interface AgentSelectorProps {
  isOpen: boolean;
  onClose: () => void;
}

interface Provider {
  id: string;
  name: string;
  available: boolean;
  description?: string;
  models?: string[];
  default_base_url?: string;
}

type ApiKeyMap = Record<string, { api_key?: string; base_url?: string }>;

const CLI_PROVIDERS = new Set(['claude_code', 'codex', 'gemini_cli']);
const FALLBACK_CLI_PROVIDERS: Provider[] = [
  { id: 'claude_code', name: 'Claude Code', available: false },
  { id: 'codex', name: 'Codex CLI', available: false },
  { id: 'gemini_cli', name: 'Gemini CLI', available: false },
];
type ProviderMode = 'local' | 'remote' | 'cli';

function parseApiKeys(raw: unknown): ApiKeyMap {
  if (!raw) return {};
  if (typeof raw === 'object') return raw as ApiKeyMap;
  if (typeof raw !== 'string') return {};
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const result: ApiKeyMap = {};
    for (const [k, v] of Object.entries(parsed)) {
      if (typeof v === 'string') {
        result[k] = { api_key: v };
      } else if (v && typeof v === 'object') {
        const obj = v as Record<string, unknown>;
        result[k] = {
          api_key: typeof obj.api_key === 'string' ? obj.api_key : '',
          base_url: typeof obj.base_url === 'string' ? obj.base_url : '',
        };
      }
    }
    return result;
  } catch {
    return {};
  }
}

function providerIcon(providerId: string) {
  if (providerId === 'local') return <Cpu className="h-4 w-4 flex-shrink-0 text-emerald-400" />;
  return <Cloud className="h-4 w-4 flex-shrink-0 text-cyan-400" />;
}

function agentLabel(agent: Agent): string {
  if (agent.provider_id === 'local') {
    const name =
      agent.model_path
        ?.split(/[/\\]/)
        .pop()
        ?.replace(/\.gguf$/i, '') ?? '';
    return name || 'local model';
  }
  return `${agent.provider_id}${agent.provider_model ? ` / ${agent.provider_model}` : ''}`;
}

const BLANK_LOCAL_CONFIG: SamplerConfig = {
  sampler_type: DEFAULT_PRESET.sampler_type ?? 'Greedy',
  // eslint-disable-next-line @typescript-eslint/no-magic-numbers
  temperature: DEFAULT_PRESET.temperature ?? 0.8,
  // eslint-disable-next-line @typescript-eslint/no-magic-numbers
  top_p: DEFAULT_PRESET.top_p ?? 0.95,
  // eslint-disable-next-line @typescript-eslint/no-magic-numbers
  top_k: DEFAULT_PRESET.top_k ?? 40,
  repeat_penalty: DEFAULT_PRESET.repeat_penalty ?? 1.0,
  mirostat_tau: 5.0,
  mirostat_eta: 0.1,
  min_p: 0,
  flash_attention: true,
  cache_type_k: DEFAULT_PRESET.cache_type_k ?? 'f16',
  cache_type_v: DEFAULT_PRESET.cache_type_v ?? 'f16',
  gpu_layers: 32,
};

// eslint-disable-next-line max-lines-per-function, complexity
export const AgentSelector = ({ isOpen, onClose }: AgentSelectorProps) => {
  const {
    agents,
    loadAgents,
    createAgent,
    updateAgent,
    deleteAgent,
    agentStatuses,
    fetchAgentStatuses,
  } = useAgentContext();

  // ── Navigation ────────────────────────────────────────────────────────────
  const [view, setView] = useState<'list' | 'pick' | 'config'>('list');
  const [providerMode, setProviderMode] = useState<ProviderMode | null>(null);
  const [editingAgent, setEditingAgent] = useState<Agent | null>(null);

  // ── Shared form state (name, provider, model path) ────────────────────────
  const [agentName, setAgentName] = useState('');
  const [modelPath, setModelPath] = useState('');
  const [providerId, setProviderId] = useState('local');
  const [providerModel, setProviderModel] = useState('');
  const [systemPromptMode, setSystemPromptMode] = useState<SystemPromptMode>('system');
  const [customSystemPrompt, setCustomSystemPrompt] = useState('');

  // ── Local model config state (mirrors ModelConfigModal) ───────────────────
  const [localConfig, setLocalConfig] = useState<SamplerConfig>(BLANK_LOCAL_CONFIG);
  const [contextSize, setContextSize] = useState(DEFAULT_CONTEXT_SIZE);
  const [overheadGb, setOverheadGb] = useState(2.0);
  const [mmprojEnabled, setMmprojEnabled] = useState(false);
  const [mmprojPath, setMmprojPath] = useState('');
  const [isConfigExpanded, setIsConfigExpanded] = useState(true);
  const [isPickingModel, setIsPickingModel] = useState(false);
  const [modelHistory, setModelHistory] = useState<string[]>([]);
  const [showModelHistory, setShowModelHistory] = useState(false);
  const autoOptimizedForPath = useRef('');

  // ── Provider / API key state ──────────────────────────────────────────────
  const [saving, setSaving] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [apiKeyInputs, setApiKeyInputs] = useState<ApiKeyMap>({});
  const [savingProvider, setSavingProvider] = useState<string | null>(null);
  const nameInputRef = useRef<HTMLInputElement>(null);

  // ── Hooks (must be called unconditionally) ────────────────────────────────
  const { status: modelStatus } = useModelContext();
  const {
    totalVramGb: availableVramGb,
    totalRamGb: availableRamGb,
    unifiedMemory,
  } = useSystemResources();

  const {
    fileExists,
    isCheckingFile,
    directoryError,
    directorySuggestions,
    modelInfo,
    maxLayers,
    isTauri,
  } = useModelPathValidation({ modelPath, onPathChange: setModelPath });

  const generalName = modelInfo?.general_name;
  const recommendedParams = modelInfo?.recommended_params;

  const resolvedPreset = useMemo((): Partial<SamplerConfig> => {
    const specificPreset = findPresetByName(generalName || '');
    const namedPreset = specificPreset || DEFAULT_PRESET;
    if (recommendedParams && Object.keys(recommendedParams).length > 0) {
      const { repetition_penalty, ...rest } = recommendedParams;
      const ggufParams = {
        ...rest,
        ...(repetition_penalty != null ? { repeat_penalty: repetition_penalty } : {}),
      };
      return specificPreset
        ? { ...DEFAULT_PRESET, ...ggufParams, ...specificPreset }
        : { ...namedPreset, ...ggufParams };
    }
    return namedPreset;
  }, [generalName, recommendedParams]);

  const maxContextSize = useMemo(() => {
    if (!modelInfo?.context_length) return DEFAULT_MAX_CONTEXT;
    const parsed = parseInt(modelInfo.context_length.toString().replace(/,/g, ''));
    return isNaN(parsed) ? DEFAULT_MAX_CONTEXT : parsed;
  }, [modelInfo?.context_length]);

  const optimized = useVramOptimizer({
    modelMetadata: modelInfo,
    availableVramGb,
    maxLayers,
    cacheTypeK: resolvedPreset.cache_type_k || 'turbo2',
    cacheTypeV: resolvedPreset.cache_type_v || 'turbo3',
    presetContextSize: resolvedPreset.context_size,
    maxContextSize,
  });

  const memoryBreakdown = useMemoryCalculation({
    modelMetadata: modelInfo,
    gpuLayers: localConfig.gpu_layers || 0,
    contextSize,
    availableVramGb,
    availableRamGb,
    overheadGb,
    cacheTypeK: localConfig.cache_type_k || resolvedPreset.cache_type_k || 'turbo2',
    cacheTypeV: localConfig.cache_type_v || resolvedPreset.cache_type_v || 'turbo2',
  });

  // ── Load providers + model history ────────────────────────────────────────
  useEffect(() => {
    if (!isOpen) return;
    loadAgents().catch(() => {});
    fetchAgentStatuses().catch(() => {});
    const loadFormData = async () => {
      const history = await getModelHistory().catch(() => []);
      setModelHistory(history);
      if (isTauriEnv()) {
        const { invoke } = await import('@tauri-apps/api/core');
        const [configured, cli, config] = await Promise.all([
          invoke<{ providers?: Provider[] }>('list_configured_providers'),
          invoke<{ providers?: Provider[] }>('list_cli_providers').catch(() => ({ providers: [] })),
          invoke<{ provider_api_keys?: string }>('get_config').catch(
            () => ({}) as { provider_api_keys?: string },
          ),
        ]);
        const merged = new Map<string, Provider>();
        for (const p of configured.providers ?? []) merged.set(p.id, p);
        for (const p of cli.providers ?? []) merged.set(p.id, p);
        setProviders(Array.from(merged.values()));
        setApiKeyInputs(parseApiKeys(config.provider_api_keys));
        return;
      }
      const [configured, cli, keys] = await Promise.all([
        fetch('/api/providers/configured').then((r) => r.json()),
        fetch('/api/providers/cli-status')
          .then((r) => r.json())
          .catch(() => ({ providers: [] })),
        fetch('/api/config/provider-keys')
          .then((r) => r.json())
          .catch(() => ({})),
      ]);
      const merged = new Map<string, Provider>();
      for (const p of (configured.providers ?? []) as Provider[]) merged.set(p.id, p);
      for (const p of (cli.providers ?? []) as Provider[]) merged.set(p.id, p);
      setProviders(Array.from(merged.values()));
      setApiKeyInputs(parseApiKeys(keys));
    };
    loadFormData().catch(() => {});
  }, [isOpen, loadAgents, fetchAgentStatuses]);

  // ── Reset on close ────────────────────────────────────────────────────────
  useEffect(() => {
    if (!isOpen) {
      setView('list');
      setProviderMode(null);
      setEditingAgent(null);
      setAgentName('');
      setModelPath('');
      setProviderId('local');
      setProviderModel('');
      setSystemPromptMode('system');
      setCustomSystemPrompt('');
      setLocalConfig(BLANK_LOCAL_CONFIG);
      setContextSize(DEFAULT_CONTEXT_SIZE);
      setMmprojEnabled(false);
      setMmprojPath('');
      setIsConfigExpanded(true);
      setConfirmDeleteId(null);
      autoOptimizedForPath.current = '';
    }
  }, [isOpen]);

  // ── Keyboard ──────────────────────────────────────────────────────────────
  useEffect(() => {
    if (!isOpen) return;
    const handleKeyDown = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape') {
        if (view === 'config') {
          setView('pick');
          return;
        }
        if (view === 'pick') {
          setView('list');
          return;
        }
        onClose();
      }
    };
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, onClose, view]);

  useEffect(() => {
    if (!isOpen || view !== 'pick') return;
    nameInputRef.current?.focus();
  }, [isOpen, view]);

  // ── Auto-apply preset when model info loads ───────────────────────────────
  useEffect(() => {
    if (!modelInfo || providerMode !== 'local' || editingAgent) return;
    const specificPreset = findPresetByName(modelInfo.general_name || '');
    const recommended = modelInfo.recommended_params;
    const preset = specificPreset ?? DEFAULT_PRESET;
    let ggufParams: Partial<SamplerConfig> = {};
    if (recommended && Object.keys(recommended).length > 0) {
      const { repetition_penalty, ...rest } = recommended;
      ggufParams = {
        ...rest,
        ...(repetition_penalty != null ? { repeat_penalty: repetition_penalty } : {}),
      };
    }
    const merged = specificPreset
      ? { ...DEFAULT_PRESET, ...ggufParams, ...specificPreset }
      : { ...preset, ...ggufParams };
    const { context_size: presetCtx, ...samplerPreset } = merged as Partial<SamplerConfig> & {
      context_size?: number;
    };
    setLocalConfig((prev) => ({ ...prev, ...samplerPreset, model_path: prev.model_path }));
    if (presetCtx) setContextSize(presetCtx);
  }, [modelInfo, providerMode, editingAgent]);

  // ── Set context size to model max on metadata load ────────────────────────
  useEffect(() => {
    if (editingAgent || !modelInfo?.context_length) return;
    const max = parseInt(modelInfo.context_length.toString().replace(/,/g, ''));
    if (!isNaN(max)) setContextSize(max);
  }, [modelInfo, editingAgent]);

  // ── Auto-enable mmproj ────────────────────────────────────────────────────
  useEffect(() => {
    if (modelInfo?.mmproj_files?.length) {
      setMmprojEnabled(true);
      setMmprojPath(modelInfo.mmproj_files[0].path);
    } else {
      setMmprojEnabled(false);
      setMmprojPath('');
    }
  }, [modelInfo?.mmproj_files]);

  // ── Tag pairs from model ──────────────────────────────────────────────────
  useEffect(() => {
    if (!modelInfo?.detected_tag_pairs?.length) return;
    setLocalConfig((prev) => ({ ...prev, tag_pairs: modelInfo.detected_tag_pairs }));
  }, [modelInfo?.detected_tag_pairs]); // eslint-disable-line react-hooks/exhaustive-deps

  // ── VRAM auto-optimize ────────────────────────────────────────────────────
  useEffect(() => {
    if (optimized.ready && modelPath && autoOptimizedForPath.current !== modelPath) {
      autoOptimizedForPath.current = modelPath;
      setLocalConfig((prev) => ({ ...prev, gpu_layers: optimized.optimalGpuLayers }));
    }
  }, [optimized, modelPath]);

  // ── Navigation helpers ────────────────────────────────────────────────────
  const openCreate = () => {
    setEditingAgent(null);
    setAgentName('');
    setModelPath('');
    setProviderId('local');
    setProviderModel('');
    setSystemPromptMode('system');
    setCustomSystemPrompt('');
    setLocalConfig(BLANK_LOCAL_CONFIG);
    setContextSize(DEFAULT_CONTEXT_SIZE);
    setMmprojEnabled(false);
    setMmprojPath('');
    setIsConfigExpanded(true);
    autoOptimizedForPath.current = '';
    setProviderMode(null);
    setView('pick');
  };

  // eslint-disable-next-line complexity
  const openEdit = (agent: Agent) => {
    setEditingAgent(agent);
    setAgentName(agent.name);
    setModelPath(agent.model_path ?? '');
    setProviderId(agent.provider_id);
    setProviderModel(agent.provider_model ?? '');
    setSystemPromptMode(agent.system_prompt ? 'custom' : 'system');
    setCustomSystemPrompt(agent.system_prompt ?? '');
    setLocalConfig({
      ...BLANK_LOCAL_CONFIG,
      sampler_type: agent.sampler_type ?? BLANK_LOCAL_CONFIG.sampler_type,
      temperature: agent.temperature ?? BLANK_LOCAL_CONFIG.temperature,
      top_p: agent.top_p ?? BLANK_LOCAL_CONFIG.top_p,
      top_k: agent.top_k ?? BLANK_LOCAL_CONFIG.top_k,
      repeat_penalty: agent.repeat_penalty ?? BLANK_LOCAL_CONFIG.repeat_penalty,
      mirostat_tau: agent.mirostat_tau ?? BLANK_LOCAL_CONFIG.mirostat_tau,
      mirostat_eta: agent.mirostat_eta ?? BLANK_LOCAL_CONFIG.mirostat_eta,
      min_p: agent.min_p ?? BLANK_LOCAL_CONFIG.min_p,
      flash_attention: agent.flash_attention ?? true,
      cache_type_k: agent.cache_type_k ?? 'f16',
      cache_type_v: agent.cache_type_v ?? 'f16',
      // eslint-disable-next-line @typescript-eslint/no-magic-numbers
      gpu_layers: agent.main_gpu ?? 32,
      thinking_mode: agent.thinking_mode,
      typical_p: agent.typical_p,
      frequency_penalty: agent.frequency_penalty,
      presence_penalty: agent.presence_penalty,
      dry_multiplier: agent.dry_multiplier,
      seed: agent.seed,
      n_threads: agent.n_threads,
      rope_freq_base: agent.rope_freq_base,
      use_mlock: agent.use_mlock,
      use_mmap: agent.use_mmap,
      main_gpu: agent.main_gpu,
      split_mode: agent.split_mode,
    });
    setContextSize(agent.context_size ?? DEFAULT_CONTEXT_SIZE);
    setMmprojEnabled(false);
    setMmprojPath('');
    setIsConfigExpanded(true);
    autoOptimizedForPath.current = agent.model_path ?? '';
    let providerModeForAgent: ProviderMode;
    if (agent.provider_id === 'local') {
      providerModeForAgent = 'local';
    } else if (CLI_PROVIDERS.has(agent.provider_id)) {
      providerModeForAgent = 'cli';
    } else {
      providerModeForAgent = 'remote';
    }
    setProviderMode(providerModeForAgent);
    setView('config');
  };

  const handleSelectProvider = (mode: ProviderMode) => {
    setProviderMode(mode);
    if (mode === 'local') {
      setProviderId('local');
    } else {
      const available =
        mode === 'cli'
          ? providers.filter((p) => CLI_PROVIDERS.has(p.id))
          : providers.filter((p) => p.id !== 'local' && !CLI_PROVIDERS.has(p.id));
      const first = available[0];
      setProviderId(first?.id ?? (mode === 'cli' ? 'claude_code' : 'custom'));
      setProviderModel(first?.models?.[0] ?? 'default');
      setModelPath('');
    }
    setView('config');
  };

  // ── Derived validation ────────────────────────────────────────────────────
  const trimmedName = agentName.trim();
  const isDuplicateName =
    trimmedName.length > 0 &&
    agents.some(
      (a) => a.name.toLowerCase() === trimmedName.toLowerCase() && a.id !== editingAgent?.id,
    );
  const canSave =
    !saving &&
    trimmedName.length > 0 &&
    !isDuplicateName &&
    (providerMode === 'local'
      ? modelPath.trim().length > 0 && fileExists === true
      : providerModel.trim().length > 0);

  // ── Save ──────────────────────────────────────────────────────────────────
  // eslint-disable-next-line complexity
  const handleSave = async () => {
    if (!trimmedName) {
      toast.error('Name is required');
      return;
    }
    if (isDuplicateName) {
      toast.error('An agent with this name already exists');
      return;
    }
    if (!providerMode) {
      toast.error('Select a provider type');
      return;
    }
    if (providerMode === 'local') {
      if (!modelPath.trim()) {
        toast.error('Local agents need a GGUF model file');
        return;
      }
      if (fileExists !== true) {
        toast.error('The selected model file is not accessible');
        return;
      }
    } else {
      if (!providerModel.trim()) {
        toast.error('Remote agents need a model name');
        return;
      }
    }

    setSaving(true);
    try {
      const resolvedSystemPrompt =
        systemPromptMode === 'custom' && customSystemPrompt.trim()
          ? customSystemPrompt.trim()
          : undefined;

      let payload: Record<string, unknown> = {
        name: agentName.trim(),
        provider_id: providerId,
        ...(resolvedSystemPrompt ? { system_prompt: resolvedSystemPrompt } : {}),
      };

      if (providerMode === 'local') {
        const tagPairs = localConfig.tag_pairs || [];
        const execPair = tagPairs.find(
          (p) => p.category === 'tool' && p.name === 'exec' && p.enabled,
        );
        const respPair = tagPairs.find(
          (p) => p.category === 'tool' && p.name === 'response' && p.enabled,
        );
        payload = {
          ...payload,
          model_path: modelPath,
          context_size: contextSize,
          sampler_type: localConfig.sampler_type,
          temperature: localConfig.temperature,
          top_p: localConfig.top_p,
          top_k: localConfig.top_k,
          repeat_penalty: localConfig.repeat_penalty,
          mirostat_tau: localConfig.mirostat_tau,
          mirostat_eta: localConfig.mirostat_eta,
          min_p: localConfig.min_p,
          flash_attention: localConfig.flash_attention,
          cache_type_k: localConfig.cache_type_k,
          cache_type_v: localConfig.cache_type_v,
          main_gpu: localConfig.gpu_layers,
          ...(localConfig.thinking_mode !== undefined
            ? { thinking_mode: localConfig.thinking_mode }
            : {}),
          ...(localConfig.typical_p !== undefined ? { typical_p: localConfig.typical_p } : {}),
          ...(localConfig.frequency_penalty !== undefined
            ? { frequency_penalty: localConfig.frequency_penalty }
            : {}),
          ...(localConfig.presence_penalty !== undefined
            ? { presence_penalty: localConfig.presence_penalty }
            : {}),
          ...(localConfig.dry_multiplier !== undefined
            ? { dry_multiplier: localConfig.dry_multiplier }
            : {}),
          ...(localConfig.seed !== undefined ? { seed: localConfig.seed } : {}),
          ...(localConfig.n_threads !== undefined ? { n_threads: localConfig.n_threads } : {}),
          ...(localConfig.rope_freq_base !== undefined
            ? { rope_freq_base: localConfig.rope_freq_base }
            : {}),
          ...(localConfig.use_mlock !== undefined ? { use_mlock: localConfig.use_mlock } : {}),
          ...(localConfig.use_mmap !== undefined ? { use_mmap: localConfig.use_mmap } : {}),
          ...(localConfig.split_mode ? { split_mode: localConfig.split_mode } : {}),
          ...(mmprojEnabled && mmprojPath ? { mmproj_path: mmprojPath } : {}),
          ...(execPair
            ? { tool_tag_exec_open: execPair.open_tag, tool_tag_exec_close: execPair.close_tag }
            : {}),
          ...(respPair
            ? { tool_tag_output_open: respPair.open_tag, tool_tag_output_close: respPair.close_tag }
            : {}),
        };
      } else {
        payload = { ...payload, provider_model: providerModel };
      }

      const typedPayload = payload as Partial<Agent> & { name: string; provider_id: string };
      if (editingAgent) {
        await updateAgent(editingAgent.id, typedPayload);
        toast.success('Agent updated');
      } else {
        await createAgent(typedPayload);
        toast.success('Agent created');
      }
      setView('list');
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      toast.error(editingAgent ? 'Failed to update agent' : 'Failed to create agent');
      recordAppError({
        level: 'error',
        source: 'AgentSelector.save',
        message: editingAgent ? 'Failed to update agent' : 'Failed to create agent',
        details: msg,
      }).catch(() => {});
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async (id: string) => {
    if (confirmDeleteId !== id) {
      setConfirmDeleteId(id);
      return;
    }
    setDeletingId(id);
    try {
      await deleteAgent(id);
      setConfirmDeleteId(null);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      toast.error('Failed to delete agent');
      recordAppError({
        level: 'error',
        source: 'AgentSelector.delete',
        message: 'Failed to delete agent',
        details: msg,
      }).catch(() => {});
    } finally {
      setDeletingId(null);
    }
  };

  const handleLocalConfigChange = (
    field: keyof SamplerConfig,
    value: string | number | boolean | null,
  ) => {
    setLocalConfig((prev) => ({ ...prev, [field]: value }));
  };

  const handleProviderChange = (newProviderId: string) => {
    const provider = providers.find((p) => p.id === newProviderId);
    setProviderId(newProviderId);
    setProviderModel(
      newProviderId === 'local' ? '' : providerModel || provider?.models?.[0] || 'default',
    );
    if (newProviderId === 'local') setModelPath('');
  };

  const handleBrowseModel = async () => {
    if (isPickingModel) return;
    setIsPickingModel(true);
    try {
      const path = await pickFile();
      if (path) {
        setModelPath(path);
        autoOptimizedForPath.current = '';
      }
    } catch {
      toast.error('Failed to open file picker');
    } finally {
      setIsPickingModel(false);
    }
  };

  const saveApiKey = async (pid: string) => {
    setSavingProvider(pid);
    try {
      const input = apiKeyInputs[pid] || {};
      if (isTauriEnv()) {
        const { invoke } = await import('@tauri-apps/api/core');
        const config = await invoke<Record<string, unknown>>('get_config');
        const keys = parseApiKeys(config.provider_api_keys);
        keys[pid] = {
          ...keys[pid],
          api_key: input.api_key || '',
          ...(input.base_url ? { base_url: input.base_url } : {}),
        };
        await invoke('save_config', {
          config: { ...config, provider_api_keys: JSON.stringify(keys) },
        });
        const configured = await invoke<{ providers?: Provider[] }>('list_configured_providers');
        setProviders((current) => {
          const merged = new Map(current.map((p) => [p.id, p]));
          for (const p of configured.providers || []) merged.set(p.id, p);
          return Array.from(merged.values());
        });
        toast.success('Provider saved');
        return;
      }
      await fetch('/api/config/provider-keys', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          provider: pid,
          api_key: input.api_key || '',
          ...(input.base_url ? { base_url: input.base_url } : {}),
        }),
      });
      const r = await fetch('/api/providers/configured');
      const data = await r.json();
      setProviders((current) => {
        const merged = new Map(current.map((p) => [p.id, p]));
        for (const p of (data.providers || []) as Provider[]) merged.set(p.id, p);
        return Array.from(merged.values());
      });
      toast.success('Provider saved');
    } catch {
      toast.error('Failed to save provider');
    } finally {
      setSavingProvider(null);
    }
  };

  const cloudProviders = providers.filter((p) => p.id !== 'local' && !CLI_PROVIDERS.has(p.id));
  const loadedCliProviders = providers.filter((p) => CLI_PROVIDERS.has(p.id));
  const cliProviders = loadedCliProviders.length > 0 ? loadedCliProviders : FALLBACK_CLI_PROVIDERS;
  const selectedProvider = providers.find((p) => p.id === providerId);
  const remoteProviderSelectValue = cloudProviders.some((p) => p.id === providerId)
    ? providerId
    : 'custom';
  const needsApiKey =
    selectedProvider &&
    providerMode !== 'local' &&
    !CLI_PROVIDERS.has(selectedProvider.id) &&
    !selectedProvider.available;
  const customProviderInputValue = providerId === 'custom' ? '' : providerId;
  const selectedProviderStatus = selectedProvider?.available ? 'configured' : 'needs setup';
  const agentCountLabel = agents.length !== 1 ? 's' : '';

  if (!isOpen) return null;

  const handleBackdropClick = (e: MouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) onClose();
  };

  let headerBack: (() => void) | null = null;
  if (view === 'pick') headerBack = () => setView('list');
  else if (view === 'config') headerBack = () => setView('pick');

  let headerTitle: string;
  if (view === 'list') headerTitle = 'Agents';
  else if (view === 'pick') headerTitle = editingAgent ? 'Edit Agent' : 'New Agent';
  else if (providerMode === 'local') headerTitle = 'Local Model';
  else if (providerMode === 'remote') headerTitle = 'Remote Provider';
  else headerTitle = 'CLI Provider';

  // Wider modal for Local config step
  const modalWidth =
    view === 'config' && providerMode === 'local' ? 'w-[95vw] max-w-5xl' : 'w-[720px] max-w-[92vw]';

  // Give the agent list a stable height so deleting agents doesn't resize the modal —
  // the list scrolls inside the fixed body between the pinned header and footer. Other
  // views stay content-sized (capped by max-h-[90vh]).
  const modalHeight = view === 'list' ? 'h-[80vh]' : '';

  return (
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-static-element-interactions
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      role="dialog"
      aria-modal="true"
      onClick={handleBackdropClick}
    >
      {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions */}
      <div
        className={`rounded-lg border border-border bg-card shadow-2xl ${modalWidth} ${modalHeight} flex max-h-[90vh] flex-col`}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex flex-shrink-0 items-center justify-between border-b border-border px-5 py-4">
          <div className="flex items-center gap-2">
            {!!headerBack && (
              <button
                onClick={headerBack}
                className="rounded p-1 text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                aria-label="Back"
              >
                <ChevronLeft className="h-4 w-4" />
              </button>
            )}
            <Bot className="h-4 w-4 text-muted-foreground" />
            <h3 className="text-base font-medium text-foreground">{headerTitle}</h3>
          </div>
          <button
            onClick={onClose}
            className="rounded p-1 text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto">
          {/* ── LIST ── */}
          {view === 'list' && (
            <div className="space-y-2 p-5">
              {agents.length === 0 && (
                <div className="py-10 text-center text-sm text-muted-foreground">
                  No agents yet. Create one to get started.
                </div>
              )}
              {agents.length > 0 &&
                agents.map((agent) => {
                  const agentStatus = agentStatuses[agent.id]?.status ?? 'idle';
                  const isRunning = agentStatus === 'active' || agentStatus === 'generating';
                  let statusDotClass = 'bg-muted-foreground/30';
                  if (agentStatus === 'generating') statusDotClass = 'bg-amber-400 animate-pulse';
                  else if (isRunning) statusDotClass = 'bg-emerald-400';

                  const isConfirmDelete = confirmDeleteId === agent.id;
                  const deleteTitle = isConfirmDelete
                    ? 'Click again to confirm delete'
                    : 'Delete agent';

                  return (
                    <div
                      key={agent.id}
                      className={`rounded-lg border transition-colors ${isRunning ? 'border-primary/50 bg-primary/5' : 'border-border'}`}
                    >
                      <div className="flex items-center gap-3 p-3">
                        {providerIcon(agent.provider_id)}
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2">
                            {/* Status dot */}
                            <span
                              className={`h-2 w-2 flex-shrink-0 rounded-full ${statusDotClass}`}
                            />
                            <span className="truncate text-sm font-medium text-foreground">
                              {agent.name}
                            </span>
                            {!!(agentStatus === 'generating') && (
                              <span className="flex-shrink-0 rounded bg-amber-500/20 px-1.5 py-0.5 text-[10px] text-amber-400">
                                running
                              </span>
                            )}
                            {!!(agentStatus === 'active') && (
                              <span className="flex-shrink-0 rounded bg-primary/20 px-1.5 py-0.5 text-[10px] text-primary">
                                active
                              </span>
                            )}
                          </div>
                          <div className="mt-0.5 truncate text-xs text-muted-foreground">
                            {agentLabel(agent)}
                          </div>
                        </div>
                        <div className="flex flex-shrink-0 items-center gap-1">
                          <button
                            onClick={() => openEdit(agent)}
                            title="Edit agent"
                            className="rounded p-1.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                          >
                            <Pencil className="h-3.5 w-3.5" />
                          </button>
                          <button
                            onClick={() => handleDelete(agent.id)}
                            disabled={deletingId === agent.id}
                            title={deleteTitle}
                            className={`rounded p-1.5 transition-colors ${isConfirmDelete ? 'bg-red-500/20 text-red-400 hover:bg-red-500/30' : 'text-muted-foreground hover:bg-muted hover:text-red-400'}`}
                          >
                            <Trash2 className="h-3.5 w-3.5" />
                          </button>
                        </div>
                      </div>
                    </div>
                  );
                })}
            </div>
          )}

          {/* ── STEP 1: name + provider type ── */}
          {view === 'pick' && (
            <div className="space-y-4 p-5">
              <div className="space-y-1.5">
                {/* eslint-disable-next-line jsx-a11y/label-has-associated-control */}
                <label className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                  Name
                </label>
                <input
                  ref={nameInputRef}
                  type="text"
                  placeholder="e.g. Fast Local, Claude Sonnet..."
                  value={agentName}
                  onChange={(e) => setAgentName(e.target.value)}
                  className="w-full rounded-md border border-border bg-muted px-3 py-1.5 text-sm text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none"
                />
              </div>
              <div className="space-y-2">
                {/* eslint-disable-next-line jsx-a11y/label-has-associated-control */}
                <label className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                  Provider
                </label>
                <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
                  {(
                    [
                      {
                        mode: 'local' as ProviderMode,
                        Icon: Cpu,
                        iconClass: 'text-emerald-400',
                        title: 'Local',
                        desc: 'Run a GGUF model with llama.cpp.',
                      },
                      {
                        mode: 'remote' as ProviderMode,
                        Icon: Cloud,
                        iconClass: 'text-cyan-400',
                        title: 'Remote',
                        desc: 'Use an OpenAI-compatible API provider.',
                      },
                      {
                        mode: 'cli' as ProviderMode,
                        Icon: Bot,
                        iconClass: 'text-violet-400',
                        title: 'CLI',
                        desc: 'Use Claude Code, Codex, or Gemini CLI.',
                      },
                    ] as const
                  ).map(({ mode, Icon, iconClass, title, desc }) => (
                    <button
                      key={mode}
                      type="button"
                      onClick={() => handleSelectProvider(mode)}
                      className="rounded-md border border-border p-4 text-left transition-colors hover:border-primary/60 hover:bg-muted/50"
                    >
                      <div className="mb-1 flex items-center gap-2">
                        <Icon className={`h-4 w-4 ${iconClass}`} />
                        <span className="text-sm font-medium text-foreground">{title}</span>
                      </div>
                      <p className="text-xs leading-snug text-muted-foreground">{desc}</p>
                    </button>
                  ))}
                </div>
              </div>
            </div>
          )}

          {/* ── STEP 2: provider config ── */}
          {view === 'config' && (
            <div className="space-y-4 px-6 py-4">
              {/* Agent name inline */}
              <div className="space-y-1">
                <div className="flex items-center gap-2">
                  <span className="flex-shrink-0 text-xs text-muted-foreground">Agent:</span>
                  <input
                    type="text"
                    placeholder="Name..."
                    value={agentName}
                    onChange={(e) => setAgentName(e.target.value)}
                    className={`flex-1 rounded-md border bg-muted px-2 py-1 text-sm text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none ${isDuplicateName ? 'border-red-500/70' : 'border-border'}`}
                  />
                </div>
                {!!isDuplicateName && (
                  <p className="pl-12 text-xs text-red-400">
                    An agent with this name already exists.
                  </p>
                )}
              </div>

              {/* ── LOCAL ── */}
              {providerMode === 'local' && (
                <>
                  {/* Model File */}
                  <Card>
                    <CardHeader>
                      <CardTitle className="text-sm">Model File</CardTitle>
                    </CardHeader>
                    <CardContent className="space-y-3">
                      <ModelFileInput
                        modelPath={modelPath}
                        setModelPath={(p) => {
                          setModelPath(p);
                          autoOptimizedForPath.current = '';
                        }}
                        fileExists={fileExists}
                        isCheckingFile={isCheckingFile || isPickingModel}
                        directoryError={directoryError}
                        directorySuggestions={directorySuggestions}
                        modelHistory={modelHistory}
                        showHistory={showModelHistory}
                        setShowHistory={setShowModelHistory}
                        isTauri={isTauri}
                        handleBrowseFile={handleBrowseModel}
                      />

                      {!!isCheckingFile && (
                        <div className="flex items-center gap-2">
                          <Loader2 className="h-4 w-4 animate-spin" />
                          <p className="text-sm text-muted-foreground">Reading GGUF metadata...</p>
                        </div>
                      )}

                      {!!modelInfo && <ModelMetadataDisplay modelInfo={modelInfo} />}

                      {/* mmproj */}
                      {!!modelPath && fileExists === true && (
                        <div className="mt-2 space-y-2">
                          <label className="flex cursor-pointer select-none items-center gap-2 text-sm">
                            <input
                              type="checkbox"
                              checked={mmprojEnabled}
                              onChange={(e) => {
                                setMmprojEnabled(e.target.checked);
                                if (!e.target.checked) setMmprojPath('');
                                else if (modelInfo?.mmproj_files?.length) {
                                  setMmprojPath(modelInfo.mmproj_files[0].path);
                                }
                              }}
                              className="h-3.5 w-3.5"
                            />
                            <Eye className="h-4 w-4 text-muted-foreground" />
                            <span className="font-medium">Vision Projector (mmproj)</span>
                          </label>
                          {!!mmprojEnabled && (
                            <div className="space-y-1.5 pl-6">
                              <div className="relative">
                                <button
                                  type="button"
                                  onClick={async () => {
                                    const p = await pickFile();
                                    if (p) setMmprojPath(p);
                                  }}
                                  className={`flex w-full items-center gap-2 rounded-md border bg-background px-3 py-1.5 pr-8 text-left text-sm ${mmprojPath ? 'border-green-500/50' : 'border-input'} cursor-pointer transition-colors hover:bg-accent/50`}
                                >
                                  <FolderOpen className="h-3.5 w-3.5 flex-shrink-0 text-muted-foreground" />
                                  {!!mmprojPath && (
                                    <span className="truncate font-mono text-xs">{mmprojPath}</span>
                                  )}
                                  {!mmprojPath && (
                                    <span className="text-xs text-muted-foreground">
                                      Click to select mmproj .gguf file...
                                    </span>
                                  )}
                                </button>
                                {!!mmprojPath && (
                                  <div className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2">
                                    <CheckCircle className="h-3.5 w-3.5 text-green-500" />
                                  </div>
                                )}
                              </div>
                            </div>
                          )}
                        </div>
                      )}
                    </CardContent>
                  </Card>

                  {/* Model Configurations — only shown when file is valid */}
                  {!!modelPath && fileExists === true && (
                    <Card>
                      <CardHeader className="p-0">
                        <button
                          className={`flex w-full items-center justify-between bg-primary px-6 py-3 text-left text-white transition-opacity hover:opacity-90 ${isConfigExpanded ? 'rounded-t-lg' : 'rounded-lg'}`}
                          onClick={() => setIsConfigExpanded(!isConfigExpanded)}
                          type="button"
                        >
                          <CardTitle className="flex items-center gap-2 text-sm text-white">
                            {!!isConfigExpanded && (
                              <ChevronDown className="h-5 w-5 stroke-[3] text-white" />
                            )}
                            {!isConfigExpanded && (
                              <ChevronRight className="h-5 w-5 stroke-[3] text-white" />
                            )}
                            Model Configurations
                          </CardTitle>
                        </button>
                      </CardHeader>
                      {!!isConfigExpanded && (
                        <CardContent className="space-y-4 pt-6">
                          {!!modelInfo && (
                            <MemoryVisualization
                              memory={memoryBreakdown}
                              unifiedMemory={unifiedMemory}
                              overheadGb={overheadGb}
                              onOverheadChange={setOverheadGb}
                              gpuLayers={localConfig.gpu_layers || 0}
                              onGpuLayersChange={(layers) =>
                                handleLocalConfigChange('gpu_layers', layers)
                              }
                              maxLayers={maxLayers}
                              contextSize={contextSize}
                              onContextSizeChange={setContextSize}
                              maxContextSize={maxContextSize}
                              systemPromptTokens={modelStatus.system_prompt_tokens}
                              toolDefinitionsTokens={modelStatus.tool_definitions_tokens}
                            />
                          )}

                          <ModelConfigSystemPrompt
                            systemPromptMode={systemPromptMode}
                            setSystemPromptMode={setSystemPromptMode}
                            customSystemPrompt={customSystemPrompt}
                            setCustomSystemPrompt={setCustomSystemPrompt}
                          />

                          <AdvancedContextSection
                            config={localConfig}
                            onConfigChange={handleLocalConfigChange}
                            supportsThinking={
                              modelStatus.supports_thinking ??
                              Boolean(
                                modelInfo?.chat_template &&
                                (modelInfo.chat_template.includes('enable_thinking') ||
                                  modelInfo.chat_template.includes('clear_thinking')),
                              )
                            }
                          />

                          <SamplingParametersSection
                            config={localConfig}
                            onConfigChange={handleLocalConfigChange}
                          />

                          <TagPairsSection
                            tagPairs={localConfig.tag_pairs || []}
                            detectedTagPairs={modelInfo?.detected_tag_pairs}
                            onTagPairsChange={(pairs) =>
                              setLocalConfig((prev) => ({ ...prev, tag_pairs: pairs }))
                            }
                          />
                        </CardContent>
                      )}
                    </Card>
                  )}
                </>
              )}

              {/* ── REMOTE ── */}
              {providerMode === 'remote' && (
                <div className="space-y-3">
                  <div className="space-y-1.5">
                    {/* eslint-disable-next-line jsx-a11y/label-has-associated-control */}
                    <label className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                      Provider
                    </label>
                    <select
                      value={remoteProviderSelectValue}
                      onChange={(e) => handleProviderChange(e.target.value)}
                      className="w-full rounded-md border border-border bg-muted px-3 py-1.5 text-sm text-foreground focus:border-primary focus:outline-none"
                    >
                      {cloudProviders.map((p) => {
                        const configuredSuffix = p.available ? '' : ' (not configured)';
                        return (
                          <option key={p.id} value={p.id}>
                            {p.name}
                            {configuredSuffix}
                          </option>
                        );
                      })}
                      <option value="custom">Custom</option>
                    </select>
                    {remoteProviderSelectValue === 'custom' && (
                      <input
                        type="text"
                        placeholder="Provider ID (e.g. openai, groq)"
                        value={customProviderInputValue}
                        onChange={(e) => setProviderId(e.target.value)}
                        className="w-full rounded-md border border-border bg-muted px-3 py-1.5 font-mono text-sm text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none"
                      />
                    )}
                  </div>
                  {!!selectedProvider && (
                    <div
                      className={`rounded-md border px-3 py-2 text-xs ${selectedProvider.available ? 'border-emerald-500/30 bg-emerald-500/10 text-emerald-300' : 'border-border bg-muted/30 text-muted-foreground'}`}
                    >
                      <div className="flex items-center gap-2">
                        {providerIcon(selectedProvider.id)}
                        <span className="font-medium">{selectedProvider.name}</span>
                        <span className="ml-auto">{selectedProviderStatus}</span>
                      </div>
                    </div>
                  )}
                  {!!needsApiKey && (
                    <div className="space-y-2 rounded-md border border-border p-3">
                      <div className="space-y-1">
                        {/* eslint-disable-next-line jsx-a11y/label-has-associated-control */}
                        <label className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                          API Key
                        </label>
                        <input
                          type="password"
                          placeholder="sk-..."
                          value={apiKeyInputs[providerId]?.api_key || ''}
                          onChange={(e) =>
                            setApiKeyInputs((prev) => ({
                              ...prev,
                              [providerId]: { ...prev[providerId], api_key: e.target.value },
                            }))
                          }
                          className="w-full rounded-md border border-border bg-muted px-3 py-1.5 font-mono text-xs text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none"
                        />
                      </div>
                      <div className="space-y-1">
                        {/* eslint-disable-next-line jsx-a11y/label-has-associated-control */}
                        <label className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                          Base URL
                        </label>
                        <input
                          type="text"
                          placeholder={
                            selectedProvider?.default_base_url || 'https://api.example.com/v1'
                          }
                          value={apiKeyInputs[providerId]?.base_url || ''}
                          onChange={(e) =>
                            setApiKeyInputs((prev) => ({
                              ...prev,
                              [providerId]: { ...prev[providerId], base_url: e.target.value },
                            }))
                          }
                          className="w-full rounded-md border border-border bg-muted px-3 py-1.5 font-mono text-xs text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none"
                        />
                      </div>
                      <button
                        type="button"
                        onClick={() => saveApiKey(providerId)}
                        disabled={savingProvider === providerId}
                        className="inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
                      >
                        {savingProvider === providerId && (
                          <Loader2 className="h-3 w-3 animate-spin" />
                        )}
                        Save Provider
                      </button>
                    </div>
                  )}
                  <div className="space-y-1.5">
                    {/* eslint-disable-next-line jsx-a11y/label-has-associated-control */}
                    <label className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                      Model
                    </label>
                    {!!selectedProvider?.models?.length && (
                      <select
                        value={providerModel || selectedProvider.models[0]}
                        onChange={(e) => setProviderModel(e.target.value)}
                        className="w-full rounded-md border border-border bg-muted px-3 py-1.5 font-mono text-sm text-foreground focus:border-primary focus:outline-none"
                      >
                        {selectedProvider.models.map((m) => (
                          <option key={m} value={m}>
                            {m}
                          </option>
                        ))}
                      </select>
                    )}
                    <input
                      type="text"
                      placeholder="Or type a model name..."
                      value={providerModel}
                      onChange={(e) => setProviderModel(e.target.value)}
                      className="w-full rounded-md border border-border bg-muted px-3 py-1.5 font-mono text-sm text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none"
                    />
                  </div>
                  <ModelConfigSystemPrompt
                    systemPromptMode={systemPromptMode}
                    setSystemPromptMode={setSystemPromptMode}
                    customSystemPrompt={customSystemPrompt}
                    setCustomSystemPrompt={setCustomSystemPrompt}
                  />
                </div>
              )}

              {/* ── CLI ── */}
              {providerMode === 'cli' && (
                <div className="space-y-3">
                  <div className="space-y-1.5">
                    {/* eslint-disable-next-line jsx-a11y/label-has-associated-control */}
                    <label className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                      CLI Provider
                    </label>
                    <select
                      value={providerId}
                      onChange={(e) => handleProviderChange(e.target.value)}
                      className="w-full rounded-md border border-border bg-muted px-3 py-1.5 text-sm text-foreground focus:border-primary focus:outline-none"
                    >
                      {cliProviders.map((p) => {
                        const detectedSuffix = p.available ? '' : ' (not detected)';
                        return (
                          <option key={p.id} value={p.id}>
                            {p.name}
                            {detectedSuffix}
                          </option>
                        );
                      })}
                    </select>
                  </div>
                  <ModelConfigSystemPrompt
                    systemPromptMode={systemPromptMode}
                    setSystemPromptMode={setSystemPromptMode}
                    customSystemPrompt={customSystemPrompt}
                    setCustomSystemPrompt={setCustomSystemPrompt}
                  />
                </div>
              )}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex flex-shrink-0 items-center justify-between border-t border-border px-5 py-4">
          {view === 'list' && (
            <>
              <span className="text-xs text-muted-foreground">
                {agents.length} agent{agentCountLabel}
              </span>
              <button
                onClick={openCreate}
                className="flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90"
              >
                <Plus className="h-3.5 w-3.5" /> New Agent
              </button>
            </>
          )}
          {view === 'pick' && (
            <button
              onClick={() => setView('list')}
              className="ml-auto rounded-md px-3 py-1.5 text-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            >
              Cancel
            </button>
          )}
          {view === 'config' && (
            <>
              <button
                onClick={() => setView('pick')}
                className="rounded-md px-3 py-1.5 text-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              >
                Back
              </button>
              {(() => {
                let saveLabel = editingAgent ? 'Save Changes' : 'Create Agent';
                if (saving) saveLabel = 'Saving...';
                return (
                  <button
                    onClick={handleSave}
                    disabled={!canSave}
                    className="rounded-md bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
                  >
                    {saveLabel}
                  </button>
                );
              })()}
            </>
          )}
        </div>
      </div>
    </div>
  );
};
