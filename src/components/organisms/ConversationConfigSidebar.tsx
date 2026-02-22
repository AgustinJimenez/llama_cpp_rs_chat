import { useState, useEffect, useCallback, useRef } from 'react';
import { X, Save } from 'lucide-react';
import { toast } from 'react-hot-toast';
import { ContextSizeSection } from './model-config/ContextSizeSection';
import { GpuLayersSection } from './model-config/GpuLayersSection';
import { AdvancedContextSection } from './model-config/AdvancedContextSection';
import { SamplingParametersSection } from './model-config/SamplingParametersSection';
import { getConversationConfig, saveConversationConfig } from '../../utils/tauriCommands';
import type { SamplerConfig } from '../../types';

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
  const [localConfig, setLocalConfig] = useState<SamplerConfig | null>(null);
  const [contextSize, setContextSize] = useState(32768);
  const [isSaving, setIsSaving] = useState(false);
  const originalConfigRef = useRef<SamplerConfig | null>(null);

  useEffect(() => {
    if (!isOpen || !conversationId) {
      setLocalConfig(null);
      originalConfigRef.current = null;
      return;
    }
    getConversationConfig(conversationId)
      .then((config) => {
        setLocalConfig(config);
        setContextSize(config.context_size ?? 32768);
        originalConfigRef.current = config;
      })
      .catch(() => toast.error('Failed to load conversation config'));
  }, [isOpen, conversationId]);

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
    } catch {
      toast.error('Failed to save config');
    } finally {
      setIsSaving(false);
    }
  };

  const showContent = conversationId && localConfig;

  return (
    <>
      {isOpen && (
        <div
          className="fixed inset-0 bg-black/50 z-40 md:hidden"
          role="button"
          tabIndex={0}
          onClick={onClose}
          onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') onClose(); }}
        />
      )}

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
          {conversationId && !localConfig && <p className="text-sm text-muted-foreground">Loading...</p>}

          {showContent && (
            <>
              <ContextSizeSection contextSize={contextSize} setContextSize={setContextSize} modelInfo={null} />
              <GpuLayersSection
                gpuLayers={localConfig.gpu_layers ?? 0}
                onGpuLayersChange={(n) => handleConfigChange('gpu_layers', n)}
                maxLayers={100}
              />
              <AdvancedContextSection config={localConfig} onConfigChange={handleConfigChange} />
              <div className="border-t border-border" />
              <SamplingParametersSection config={localConfig} onConfigChange={handleConfigChange} />
            </>
          )}
        </div>

        {showContent && (
          <div className="shrink-0 px-4 py-3 border-t border-border">
            <button
              onClick={handleSave}
              disabled={isSaving}
              className="w-full flex items-center justify-center gap-2 px-4 py-2 bg-primary text-primary-foreground rounded-md hover:bg-primary/90 transition-colors disabled:opacity-50 text-sm font-medium"
            >
              <Save className="h-4 w-4" />
              {isSaving ? 'Saving...' : 'Save Config'}
            </button>
          </div>
        )}
      </div>
    </>
  );
}
