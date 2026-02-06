import React, { useState, useEffect, useRef } from 'react';
import { Brain, Loader2, ChevronDown, ChevronRight } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogDescription,
  DialogTitle,
} from '../../atoms/dialog';
import { Button } from '../../atoms/button';
import { Card, CardContent, CardHeader, CardTitle } from '../../atoms/card';
import type { SamplerConfig } from '@/types';
import { toast } from 'react-hot-toast';

// Import extracted components
import { ModelFileInput, ModelConfigSystemPrompt } from '../../molecules';
import { ModelMetadataDisplay } from './ModelMetadataDisplay';
import { ContextSizeSection } from './ContextSizeSection';
import { GpuLayersSection } from './GpuLayersSection';
import { SamplingParametersSection } from './SamplingParametersSection';
import { MemoryVisualization } from './MemoryVisualization';

// Import hooks
import { useMemoryCalculation } from '@/hooks/useMemoryCalculation';
import { useModelPathValidation } from '@/hooks/useModelPathValidation';

interface ModelConfigModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSave: (config: SamplerConfig) => void;
  isLoading?: boolean;
  initialModelPath?: string;
}

// eslint-disable-next-line max-lines-per-function, complexity
export const ModelConfigModal: React.FC<ModelConfigModalProps> = ({
  isOpen,
  onClose,
  onSave,
  isLoading = false,
  initialModelPath
}) => {
  const [config, setConfig] = useState<SamplerConfig>({
    sampler_type: 'Greedy',
    temperature: 0.7,
    top_p: 0.95,
    top_k: 20,
    mirostat_tau: 5.0,
    mirostat_eta: 0.1,
    model_path: '',
    gpu_layers: 32,  // Default for RTX 4090
  });

  const [contextSize, setContextSize] = useState(32768);
  const [modelPath, setModelPath] = useState('');
  const [isMetadataExpanded, setIsMetadataExpanded] = useState(false);
  const [isConfigExpanded, setIsConfigExpanded] = useState(true);
  const [modelHistory, setModelHistory] = useState<string[]>([]);
  const [showHistory, setShowHistory] = useState(false);
  const [systemPromptMode, setSystemPromptMode] = useState<'model' | 'system' | 'custom'>('system');
  const [customSystemPrompt, setCustomSystemPrompt] = useState('You are a helpful AI assistant.');
  const [availableVramGb, _setAvailableVramGb] = useState(22.0); // Default: RTX 4090
  const [availableRamGb, _setAvailableRamGb] = useState(32.0);   // Default: 32GB

  // Track which file path we've already applied parameters for (to prevent duplicate toasts)
  const lastAppliedPathRef = useRef<string | null>(null);

  // Use model path validation hook for file checking and metadata fetching
  const {
    fileExists,
    isCheckingFile,
    directoryError,
    directorySuggestions,
    modelInfo,
    maxLayers,
    isTauri,
  } = useModelPathValidation({
    modelPath,
    onPathChange: setModelPath,
  });

  // Calculate memory breakdown in real-time
  const memoryBreakdown = useMemoryCalculation({
    modelMetadata: modelInfo,
    gpuLayers: config.gpu_layers || 0,
    contextSize: contextSize,
    availableVramGb,
    availableRamGb,
  });

  // Initialize model path from config when modal opens
  useEffect(() => {
    if (isOpen && initialModelPath && !modelPath) {
      setModelPath(initialModelPath);
    }
    // Reset the last applied path ref when modal opens so parameters can be reapplied
    if (isOpen) {
      console.log('[ModelConfig] Modal opened, resetting lastAppliedPathRef');
      lastAppliedPathRef.current = null;
    }
  }, [isOpen, initialModelPath, modelPath]);

  // Fetch model history when modal opens
  useEffect(() => {
    const fetchHistory = async () => {
      try {
        const response = await fetch('/api/model/history');
        if (response.ok) {
          const history = await response.json();
          setModelHistory(history);
        }
      } catch (error) {
        console.error('Failed to fetch model history:', error);
      }
    };

    if (isOpen) {
      fetchHistory();
    }
  }, [isOpen]);

  useEffect(() => {
    if (modelPath) {
      setConfig(prev => ({
        ...prev,
        model_path: modelPath
      }));
    }
  }, [modelPath]);

  // Set context size to model's max when metadata is loaded
  useEffect(() => {
    if (modelInfo?.context_length) {
      const maxContext = parseInt(modelInfo.context_length.toString().replace(/,/g, ''));
      if (!isNaN(maxContext)) {
        setContextSize(maxContext);
      }
    }
  }, [modelInfo]);

  // Apply recommended sampling parameters from GGUF metadata or architecture-based defaults
  useEffect(() => {
    console.log('[ModelConfig] useEffect triggered for parameter application');
    console.log('[ModelConfig] modelInfo.file_path:', modelInfo?.file_path);
    console.log('[ModelConfig] lastAppliedPathRef.current:', lastAppliedPathRef.current);

    if (!modelInfo?.file_path) {
      console.log('[ModelConfig] No file_path, exiting early');
      return;
    }

    // Skip if we've already applied parameters for this file path
    if (lastAppliedPathRef.current === modelInfo.file_path) {
      console.log('[ModelConfig] Already applied parameters for this path, skipping');
      return;
    }

    console.log('[ModelConfig] Proceeding to apply parameters for:', modelInfo.file_path);

    const metadata = modelInfo.gguf_metadata || {};
    const arch = modelInfo.architecture?.toLowerCase() || '';

    // Helper to get metadata value with fallbacks
    const getMetadataValue = (keys: string[]): number | undefined => {
      for (const key of keys) {
        const value = metadata[key];
        if (typeof value === 'number' && !isNaN(value)) {
          return value;
        }
      }
      return undefined;
    };

    // Extract sampling parameters from metadata
    // Try architecture-specific keys first, then generic keys
    const temperature = getMetadataValue([
      'general.sampling.temp',         // Common format (e.g., GLM4, Qwen)
      'general.sampling.temperature',
      `${arch}.temperature`,
      'temperature',
      'sampling.temperature',
      'recommended.temperature'
    ]);

    const topP = getMetadataValue([
      'general.sampling.top_p',        // Common format
      `${arch}.top_p`,
      'top_p',
      'sampling.top_p',
      'recommended.top_p'
    ]);

    const topK = getMetadataValue([
      'general.sampling.top_k',        // Common format
      `${arch}.top_k`,
      'top_k',
      'sampling.top_k',
      'recommended.top_k'
    ]);

    // Architecture-based defaults (if no metadata values found)
    // These are community-recommended values for different model families
    // NOTE: GGUF files don't store sampling configs - we use official documentation
    const getArchitectureDefaults = () => {
      const archLower = arch.toLowerCase();
      const nameLower = (modelInfo.name || '').toLowerCase();

      // Devstral models (Mistral's coding-specific model)
      // Uses very low temperature (0.15) for deterministic code generation
      if (nameLower.includes('devstral')) {
        return {
          temperature: 0.15,
          top_p: 0.95,
          top_k: 64,
          min_p: 0.01,
          forceOverride: true
        };
      }

      // Ministral 3 models - variant-specific configs
      if (nameLower.includes('ministral')) {
        if (nameLower.includes('reasoning')) {
          // Reasoning variant: higher temp for diverse reasoning outputs
          return { temperature: 0.7, top_p: 0.95, top_k: 40, forceOverride: true };
        } else if (nameLower.includes('instruct')) {
          // Instruct variant: very low temp for production use
          return { temperature: 0.1, top_p: 0.95, top_k: 40, forceOverride: true };
        }
      }

      // Qwen3 models - variant-specific configs
      if (archLower.includes('qwen')) {
        if (nameLower.includes('thinking')) {
          // Thinking mode: temp=0.6, DO NOT use greedy decoding
          return { temperature: 0.6, top_p: 0.95, top_k: 20, min_p: 0, forceOverride: true };
        } else if (nameLower.includes('coder') || nameLower.includes('instruct')) {
          // Instruct/Coder: standard Qwen3 settings
          return { temperature: 0.7, top_p: 0.8, top_k: 20, min_p: 0 };
        }
        // Generic Qwen
        return { temperature: 0.7, top_p: 0.8, top_k: 20, min_p: 0 };
      }

      // Gemma 3 models - official Google recommendations
      if (archLower.includes('gemma')) {
        return {
          temperature: 1.0,
          top_p: 0.95,
          top_k: 64,
          min_p: 0,
          forceOverride: true
        };
      }

      // Granite 4 models (IBM)
      if (archLower.includes('granite')) {
        return {
          temperature: 0.6,
          top_p: 0.9,
          top_k: 50,
          min_p: 0.01
        };
      }

      // DeepSeek2 models (GLM-4.7-Flash)
      // CRITICAL: These values MUST override metadata to prevent infinite loops
      if (archLower.includes('deepseek2')) {
        return {
          temperature: 0.7,
          top_p: 0.95,
          top_k: 50,
          min_p: 0.01,
          context_size: 16384,  // Recommended 16K, NOT the metadata's 202K
          forceOverride: true
        };
      }

      // Llama models (Llama 2, 3, etc.)
      if (archLower.includes('llama')) {
        return { temperature: 0.7, top_p: 0.9, top_k: 40 };
      }

      // Mistral models (generic fallback)
      if (archLower.includes('mistral')) {
        return { temperature: 0.7, top_p: 0.95, top_k: 50 };
      }

      // Phi models
      if (archLower.includes('phi')) {
        return { temperature: 0.7, top_p: 0.95, top_k: 40 };
      }

      // Default fallback
      return null;
    };

    const archDefaults = getArchitectureDefaults();

    // Calculate updates first
    const updates: Partial<SamplerConfig> = {};

    // For deepseek2, FORCE architecture defaults to override problematic metadata values
    // Otherwise, prefer metadata values and fall back to architecture defaults
    const forceOverride = (archDefaults as any)?.forceOverride === true;
    const finalTemp = forceOverride ? (archDefaults?.temperature ?? temperature) : (temperature ?? archDefaults?.temperature);
    const finalTopP = forceOverride ? (archDefaults?.top_p ?? topP) : (topP ?? archDefaults?.top_p);
    const finalTopK = forceOverride ? (archDefaults?.top_k ?? topK) : (topK ?? archDefaults?.top_k);
    const finalMinP = (archDefaults as any)?.min_p;
    const finalContextSize = (archDefaults as any)?.context_size;

    if (finalTemp !== undefined && finalTemp >= 0 && finalTemp <= 2) {
      updates.temperature = finalTemp;
      const source = forceOverride ? '(architecture default - OVERRIDING metadata)' : (temperature !== undefined ? '(from metadata)' : '(architecture default)');
      console.log('[ModelConfig] Applying temperature:', finalTemp, source);
    }

    if (finalTopP !== undefined && finalTopP >= 0 && finalTopP <= 1) {
      updates.top_p = finalTopP;
      const source = forceOverride ? '(architecture default - OVERRIDING metadata)' : (topP !== undefined ? '(from metadata)' : '(architecture default)');
      console.log('[ModelConfig] Applying top_p:', finalTopP, source);
    }

    if (finalTopK !== undefined && finalTopK >= 0) {
      updates.top_k = finalTopK;
      const source = forceOverride ? '(architecture default - OVERRIDING metadata)' : (topK !== undefined ? '(from metadata)' : '(architecture default)');
      console.log('[ModelConfig] Applying top_k:', finalTopK, source);
    }

    // Apply min_p if specified (critical for deepseek2)
    if (finalMinP !== undefined && finalMinP >= 0 && finalMinP <= 1) {
      updates.min_p = finalMinP;
      console.log('[ModelConfig] Applying min_p:', finalMinP, '(architecture default - CRITICAL for deepseek2)');
    }

    // Apply context_size if specified (overrides metadata for deepseek2)
    if (finalContextSize !== undefined && finalContextSize > 0) {
      updates.context_size = finalContextSize;
      console.log('[ModelConfig] Applying context_size:', finalContextSize, '(architecture default - recommended for deepseek2)');
    }

    // Infer appropriate sampler type based on which parameters are present
    // This provides intelligent defaults for the sampling strategy
    const inferSamplerType = (): string | undefined => {
      const hasTemp = finalTemp !== undefined && finalTemp > 0;
      const hasTopP = finalTopP !== undefined && finalTopP < 1.0;
      const hasTopK = finalTopK !== undefined && finalTopK > 0;

      // If all three sampling parameters are present, use ChainFull for comprehensive sampling
      if (hasTemp && hasTopP && hasTopK) {
        console.log('[ModelConfig] Inferred sampler type: ChainFull (temp + top_p + top_k)');
        return 'ChainFull';
      }

      // If temperature and top_p are present, use ChainTempTopP
      if (hasTemp && hasTopP) {
        console.log('[ModelConfig] Inferred sampler type: ChainTempTopP (temp + top_p)');
        return 'ChainTempTopP';
      }

      // If temperature and top_k are present, use ChainTempTopK
      if (hasTemp && hasTopK) {
        console.log('[ModelConfig] Inferred sampler type: ChainTempTopK (temp + top_k)');
        return 'ChainTempTopK';
      }

      // If only temperature is present, use Temperature sampling
      if (hasTemp) {
        console.log('[ModelConfig] Inferred sampler type: Temperature (temp only)');
        return 'Temperature';
      }

      // If temperature is 0 or not present, use Greedy (deterministic)
      if (finalTemp !== undefined && finalTemp === 0) {
        console.log('[ModelConfig] Inferred sampler type: Greedy (temp = 0)');
        return 'Greedy';
      }

      // No inference possible
      return undefined;
    };

    const inferredSamplerType = inferSamplerType();
    if (inferredSamplerType) {
      updates.sampler_type = inferredSamplerType;
    }

    // Only update and notify if we found parameters
    if (Object.keys(updates).length > 0) {
      const hasMetadata = temperature !== undefined || topP !== undefined || topK !== undefined;
      const paramCount = Object.keys(updates).filter(k => k !== 'sampler_type').length;
      const hasSamplerType = updates.sampler_type !== undefined;

      let message = hasMetadata
        ? `Applied ${paramCount} parameter(s) from model metadata`
        : `Applied ${arch} architecture defaults`;

      if (hasSamplerType) {
        message += ` + ${updates.sampler_type} sampler`;
      }

      console.log('[ModelConfig] Showing toast with message:', message);
      console.log('[ModelConfig] Marking path as applied:', modelInfo.file_path);

      // Mark this file path as already processed
      lastAppliedPathRef.current = modelInfo.file_path;

      // Apply the updates to state
      setConfig(prev => ({ ...prev, ...updates }));

      // Show toast AFTER state update, not during
      toast.success(message, { duration: 3000 });
    } else {
      console.log('[ModelConfig] No updates to apply');
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [modelInfo?.file_path]); // Only re-run when the model file path changes

  const handleInputChange = (field: keyof SamplerConfig, value: string | number) => {
    setConfig(prev => ({
      ...prev,
      [field]: value
    }));
  };

  const handleSave = () => {
    if (!modelPath.trim()) {
      toast.error('Please select a model file or enter a model path.');
      return;
    }

    if (fileExists === false) {
      toast.error('The specified file does not exist or is not accessible.');
      return;
    }

    if (config.temperature < 0 || config.temperature > 2) {
      toast.error('Temperature must be between 0.0 and 2.0');
      return;
    }
    if (config.top_p < 0 || config.top_p > 1) {
      toast.error('Top P must be between 0.0 and 1.0');
      return;
    }
    if ((config.top_k as number) < 0) {
      toast.error('Top K must be non-negative');
      return;
    }
    if (contextSize <= 0) {
      toast.error('Context size must be positive');
      return;
    }

    // Log what we're trying to load for debugging
    console.log('Attempting to load model:', modelPath);
    console.log('Model path type:', typeof modelPath);
    console.log('Full model path being saved:', modelPath);
    console.log('File exists:', fileExists);

    // Determine system prompt based on mode
    // 'model' = null (use GGUF default), 'system' = '__AGENTIC__' (use universal prompt), 'custom' = user's prompt
    let systemPrompt: string | null = null;
    if (systemPromptMode === 'model') {
      // Use null to let backend use model's default from chat template
      systemPrompt = null;
    } else if (systemPromptMode === 'system') {
      // Use special marker to tell backend to use universal agentic prompt
      systemPrompt = '__AGENTIC__';
    } else {
      // Use custom prompt
      systemPrompt = customSystemPrompt;
    }

    const finalConfig = {
      ...config,
      model_path: modelPath,
      context_size: contextSize,
      system_prompt: systemPrompt ?? undefined,  // undefined = model default, '__AGENTIC__' = agentic mode, string = custom
    };
    console.log('[DEBUG] Saving config with system_prompt:', systemPrompt, 'mode:', systemPromptMode);
    onSave(finalConfig);
  };

  const handleBrowseFile = async () => {
    if (!isTauri) {
      alert('File picker is only available in desktop mode. Please enter the full file path manually.');
      return;
    }

    try {
      console.log('Opening file dialog...');
      const selected = await open({
        multiple: false,
        filters: [{
          name: 'GGUF Model Files',
          extensions: ['gguf']
        }]
      });

      if (selected) {
        const filePath = Array.isArray(selected) ? selected[0] : selected;
        console.log('Selected file path:', filePath);
        // Just set the path - the useModelPathValidation hook will handle
        // file existence checking and metadata fetching automatically
        setModelPath(filePath);
      }
    } catch (error) {
      console.error('Error opening file dialog:', error);
      alert(`Failed to open file dialog: ${error instanceof Error ? error.message : String(error)}`);
    }
  };


  const getModelFileName = () => {
    if (!modelPath) return 'No model selected';
    const fileName = modelPath.split('/').pop() || modelPath;
    return fileName;
  };

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="w-[95vw] max-w-7xl h-[90vh] max-h-[90vh] flex flex-col p-0">
        <DialogHeader className="px-6 pt-6">
          <DialogTitle className="flex items-center gap-2">
            <Brain className="h-5 w-5" />
            Load Model
          </DialogTitle>
          <DialogDescription className="text-sm text-muted-foreground">
            {modelPath ? `Model: ${getModelFileName()}` : 'Select a model file to load'}
          </DialogDescription>
        </DialogHeader>

        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-6">
          {/* Model File Selection */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm">Model File Selection</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              <ModelFileInput
                modelPath={modelPath}
                setModelPath={setModelPath}
                fileExists={fileExists}
                isCheckingFile={isCheckingFile}
                directoryError={directoryError}
                directorySuggestions={directorySuggestions}
                modelHistory={modelHistory}
                showHistory={showHistory}
                setShowHistory={setShowHistory}
                isTauri={isTauri}
                handleBrowseFile={handleBrowseFile}
              />

              {isCheckingFile && (
                <Card className="mt-3">
                  <CardContent className="pt-4">
                    <div className="flex items-center gap-2">
                      <Loader2 className="h-4 w-4 animate-spin" />
                      <p className="text-sm text-muted-foreground">Reading GGUF metadata...</p>
                    </div>
                  </CardContent>
                </Card>
              )}

              {modelInfo && (
                <ModelMetadataDisplay
                  modelInfo={modelInfo}
                  isExpanded={isMetadataExpanded}
                  setIsExpanded={setIsMetadataExpanded}
                />
              )}
            </CardContent>
          </Card>

          {/* Configuration Options - Only show when model is valid */}
          {modelPath && fileExists === true && (
            <Card>
              <CardHeader className="p-0">
                <button
                  className={`flex items-center justify-between w-full text-left bg-primary text-white px-6 py-3 hover:opacity-90 transition-opacity ${
                    isConfigExpanded ? 'rounded-t-lg' : 'rounded-lg'
                  }`}
                  onClick={() => setIsConfigExpanded(!isConfigExpanded)}
                  type="button"
                  data-testid="config-expand-button"
                >
                  <CardTitle className="text-sm flex items-center gap-2 text-white">
                    {isConfigExpanded ? <ChevronDown className="h-5 w-5 text-white stroke-[3]" /> : <ChevronRight className="h-5 w-5 text-white stroke-[3]" />}
                    Model Configurations
                  </CardTitle>
                </button>
              </CardHeader>
              {isConfigExpanded && (
                <CardContent className="space-y-4 pt-6">
                  <ContextSizeSection
                    contextSize={contextSize}
                    setContextSize={setContextSize}
                    modelInfo={modelInfo}
                  />

                  <ModelConfigSystemPrompt
                    systemPromptMode={systemPromptMode}
                    setSystemPromptMode={setSystemPromptMode}
                    customSystemPrompt={customSystemPrompt}
                    setCustomSystemPrompt={setCustomSystemPrompt}
                    modelInfo={modelInfo}
                  />

                  <GpuLayersSection
                    gpuLayers={config.gpu_layers || 0}
                    onGpuLayersChange={(layers) => handleInputChange('gpu_layers', layers)}
                    maxLayers={maxLayers}
                  />

                  {/* Memory Visualization - Real-time VRAM/RAM usage */}
                  {modelInfo && (
                    <MemoryVisualization memory={memoryBreakdown} />
                  )}

                  <SamplingParametersSection
                    config={config}
                    onConfigChange={handleInputChange}
                  />
                </CardContent>
              )}
            </Card>
          )}

          {isLoading && (
            <div className="pb-4">
              <div className="flex items-center justify-center gap-2 py-4">
                <Loader2 className="h-5 w-5 animate-spin" />
                <span className="text-sm font-medium">Loading model...</span>
              </div>
            </div>
          )}
        </div>

        <DialogFooter className="px-6 py-4 border-t">
          <Button variant="outline" onClick={onClose} disabled={isLoading}>
            Cancel
          </Button>
          <Button data-testid="load-model-button" onClick={handleSave} disabled={!modelPath.trim() || isCheckingFile || isLoading}>
            {isLoading ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin mr-2" />
                Loading...
              </>
            ) : isCheckingFile ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin mr-2" />
                Reading file...
              </>
            ) : 'Load Model'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
