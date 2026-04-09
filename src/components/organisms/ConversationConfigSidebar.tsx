import { useState, useEffect, useCallback, useRef } from 'react';
import { X, Save } from 'lucide-react';
import { toast } from 'react-hot-toast';
import { ContextSizeSection } from './model-config/ContextSizeSection';
import { GpuLayersSection } from './model-config/GpuLayersSection';
import { AdvancedContextSection } from './model-config/AdvancedContextSection';
import { SamplingParametersSection } from './model-config/SamplingParametersSection';
import { VramBar } from './model-config/MemoryVisualization';
import { getConversationConfig, saveConversationConfig, getConfig, getModelInfo } from '../../utils/tauriCommands';
import { useModelContext } from '../../contexts/ModelContext';
import { useSystemResources } from '../../contexts/SystemResourcesContext';
import { useMemoryCalculation } from '../../hooks/useMemoryCalculation';
import type { SamplerConfig, ModelMetadata } from '../../types';

const CONTEXT_RELOAD_FIELDS: (keyof SamplerConfig)[] = [
  'context_size', 'flash_attention', 'cache_type_k', 'cache_type_v', 'n_batch',
];

interface ConversationConfigSidebarProps {
  isOpen: boolean;
  onClose: () => void;
  conversationId: string | null;
  currentModelPath?: string;
  onReloadModel: (modelPath: string, config: SamplerConfig) => void;
}

// eslint-disable-next-line max-lines-per-function
export function ConversationConfigSidebar({
  isOpen,
  onClose,
  conversationId,
  currentModelPath,
  onReloadModel,
}: ConversationConfigSidebarProps) {
  const { status } = useModelContext();
  const [localConfig, setLocalConfig] = useState<SamplerConfig | null>(null);
  const [contextSize, setContextSize] = useState(32768);
  const [isSaving, setIsSaving] = useState(false);
  const originalConfigRef = useRef<SamplerConfig | null>(null);
  const [modelMetadata, setModelMetadata] = useState<ModelMetadata | null>(null);
  const { totalVramGb, totalRamGb } = useSystemResources();

  // Fetch model metadata for VRAM estimation
  useEffect(() => {
    if (!isOpen || !currentModelPath) { setModelMetadata(null); return; }
    getModelInfo(currentModelPath).then(info => {
      setModelMetadata(info as unknown as ModelMetadata);
    }).catch(() => setModelMetadata(null));
  }, [isOpen, currentModelPath]);

  const memory = useMemoryCalculation({
    modelMetadata,
    gpuLayers: localConfig?.gpu_layers ?? status.gpu_layers ?? 0,
    contextSize,
    availableVramGb: totalVramGb || 24,
    availableRamGb: totalRamGb || 64,
    cacheTypeK: localConfig?.cache_type_k ?? 'f16',
    cacheTypeV: localConfig?.cache_type_v ?? 'f16',
  });

  useEffect(() => {
    if (!isOpen || !conversationId) {
      setLocalConfig(null);
      originalConfigRef.current = null;
      return;
    }
    // Load both the per-conversation config and the global config.
    // Global config reflects what the Load Model modal saved (hardware + sampling).
    // Per-conversation config has user-customized sampling overrides.
    // Hardware fields (gpu_layers, context_size, cache types) always come from global
    // since they must reflect the currently loaded model, not a stale snapshot.
    Promise.all([
      getConversationConfig(conversationId).catch(() => null),
      getConfig().catch(() => null),
    ]).then(([convConfig, globalConfig]) => {
      // Extract only sampling fields from per-conversation config
      const samplingOverrides = convConfig ? {
        sampler_type: convConfig.sampler_type,
        temperature: convConfig.temperature,
        top_p: convConfig.top_p,
        top_k: convConfig.top_k,
        min_p: convConfig.min_p,
        repeat_penalty: convConfig.repeat_penalty,
        presence_penalty: convConfig.presence_penalty,
        frequency_penalty: convConfig.frequency_penalty,
        dry_multiplier: convConfig.dry_multiplier,
        dry_base: convConfig.dry_base,
        dry_allowed_length: convConfig.dry_allowed_length,
        dry_penalty_last_n: convConfig.dry_penalty_last_n,
        top_n_sigma: convConfig.top_n_sigma,
        seed: convConfig.seed,
      } : {};
      // Global config provides hardware values, conversation overrides sampling
      const merged = { ...globalConfig, ...samplingOverrides } as SamplerConfig;
      // Override gpu_layers from model status (absolute source of truth)
      if (status.gpu_layers != null) {
        merged.gpu_layers = status.gpu_layers;
      }
      if (status.block_count && merged.gpu_layers && merged.gpu_layers > status.block_count) {
        merged.gpu_layers = status.block_count;
      }
      setLocalConfig(merged);
      setContextSize(merged.context_size ?? globalConfig?.context_size ?? 32768);
      originalConfigRef.current = merged;
    }).catch(() => toast.error('Failed to load conversation config'));
  }, [isOpen, conversationId, status.block_count]);

  const handleConfigChange = useCallback(
    (field: keyof SamplerConfig, value: string | number | boolean) => {
      setLocalConfig((prev) => (prev ? { ...prev, [field]: value } : prev));
    },
    []
  );

  const handleSave = async () => {
    if (!localConfig || !conversationId) return;
    setIsSaving(true);
    const finalConfig = { ...localConfig, context_size: contextSize };
    try {
      await saveConversationConfig(conversationId, finalConfig);
      const original = originalConfigRef.current;
      const gpuChanged = original && original.gpu_layers !== finalConfig.gpu_layers;
      const contextChanged = original && CONTEXT_RELOAD_FIELDS.some((f) => original[f] !== finalConfig[f]);

      if (gpuChanged) {
        onReloadModel(currentModelPath || finalConfig.model_path || '', finalConfig);
      } else if (contextChanged) {
        toast.success('Context settings updated — takes effect on next message');
      } else {
        toast.success('Config saved');
      }
      originalConfigRef.current = finalConfig;
      setLocalConfig(finalConfig);
      onClose();
    } catch {
      toast.error('Failed to save config');
    } finally {
      setIsSaving(false);
    }
  };

  const showContent = conversationId && localConfig;

  return (
    <>
      {isOpen ? <div
          className="fixed inset-0 bg-black/50 z-40 md:hidden"
          role="button"
          tabIndex={0}
          onClick={onClose}
          onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') onClose(); }}
        /> : null}

      <div
        className={`fixed top-0 right-0 h-full bg-card border-l border-border z-50 transition-transform duration-300 flex flex-col ${
          isOpen ? 'translate-x-0' : 'translate-x-full'
        }`}
        style={{ width: '360px' }}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
          <h2 className="text-lg font-semibold">Conversation Config</h2>
          <button onClick={onClose} className="p-2 hover:bg-muted rounded-lg transition-colors" title="Close sidebar">
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-4 space-y-6">
          {!conversationId && <p className="text-sm text-muted-foreground">No conversation selected</p>}
          {conversationId && !localConfig ? <p className="text-sm text-muted-foreground">Loading...</p> : null}

          {showContent ? <>
              <ContextSizeSection contextSize={contextSize} setContextSize={setContextSize} modelInfo={{ context_length: '131072' } as ModelMetadata} />
              <GpuLayersSection
                gpuLayers={localConfig.gpu_layers ?? status.gpu_layers ?? 0}
                onGpuLayersChange={(n) => handleConfigChange('gpu_layers', n)}
                maxLayers={status.block_count ?? 100}
              />
              <AdvancedContextSection config={localConfig} onConfigChange={handleConfigChange} />
              {modelMetadata && <VramBar vram={memory.vram} />}
              <div className="border-t border-border" />
              <SamplingParametersSection config={localConfig} onConfigChange={handleConfigChange} />
            </> : null}
        </div>

        {showContent ? <div className="shrink-0 px-4 py-3 border-t border-border">
            <button
              onClick={handleSave}
              disabled={isSaving}
              className="w-full flex items-center justify-center gap-2 px-4 py-2 bg-primary text-primary-foreground rounded-md hover:bg-primary/90 transition-colors disabled:opacity-50 text-sm font-medium"
            >
              <Save className="h-4 w-4" />
              {isSaving ? 'Saving...' : 'Save Config'}
            </button>
          </div> : null}
      </div>
    </>
  );
}
