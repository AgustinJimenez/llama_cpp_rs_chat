import React, { useState, useEffect, useMemo, useRef } from 'react';
import { Loader2, ChevronDown, ChevronRight } from 'lucide-react';
import { pickFile } from '@/utils/tauriCommands';
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
import { getModelHistory, getConfig } from '@/utils/tauriCommands';

// Import extracted components
import { ModelFileInput, ModelConfigSystemPrompt } from '../../molecules';
import { ModelMetadataDisplay } from './ModelMetadataDisplay';
import { SamplingParametersSection } from './SamplingParametersSection';
import { AdvancedContextSection } from './AdvancedContextSection';
import { TagPairsSection } from './TagPairsSection';

import { MemoryVisualization } from './MemoryVisualization';

// Import hooks
import { useMemoryCalculation } from '@/hooks/useMemoryCalculation';
import { useVramOptimizer } from '@/hooks/useVramOptimizer';
import { useModelPathValidation } from '@/hooks/useModelPathValidation';
import { useSystemResources } from '@/contexts/SystemResourcesContext';

// Import model presets for auto-configuration
import { findPresetByName, DEFAULT_PRESET } from '@/config/modelPresets';

interface ModelConfigModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSave: (config: SamplerConfig) => void;
  initialModelPath?: string;
}

// eslint-disable-next-line max-lines-per-function, complexity
export const ModelConfigModal: React.FC<ModelConfigModalProps> = ({
  isOpen,
  onClose,
  onSave,
  initialModelPath
}) => {
  const [config, setConfig] = useState<SamplerConfig>({
    sampler_type: 'Greedy',
    temperature: 0.7,
    top_p: 0.95,
    top_k: 20,
    mirostat_tau: 5.0,
    mirostat_eta: 0.1,
    repeat_penalty: 1.0,
    min_p: 0,
    model_path: '',
    gpu_layers: 32,  // Default for RTX 4090
  });

  const [contextSize, setContextSize] = useState(32768);
  const [modelPath, setModelPath] = useState('');
  const [isPicking, setIsPicking] = useState(false);
  const [isConfigExpanded, setIsConfigExpanded] = useState(true);
  const [modelHistory, setModelHistory] = useState<string[]>([]);
  const [showHistory, setShowHistory] = useState(false);
  const [systemPromptMode, setSystemPromptMode] = useState<'system' | 'custom'>('system');
  const [customSystemPrompt, setCustomSystemPrompt] = useState('You are a helpful AI assistant.');
  const savedConfigLoaded = useRef(false);

  const [overheadGb, setOverheadGb] = useState(1.5);

  // Use global system resources (fetched at app startup)
  const { totalVramGb: availableVramGb, totalRamGb: availableRamGb } = useSystemResources();

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

  // Derive model name and recommended params early (used by optimizer and preset effect)
  const generalName = modelInfo?.general_name;
  const recommendedParams = modelInfo?.recommended_params;

  // Resolve preset synchronously for optimizer (avoids race with useEffect-based preset application)
  // If a model-specific preset exists, it takes priority over GGUF embedded params (it's hand-tuned).
  // GGUF params only fill in gaps the preset doesn't cover, or serve as the base when no preset exists.
  const resolvedPreset = useMemo((): Partial<SamplerConfig> => {
    const specificPreset = findPresetByName(generalName || '');
    const namedPreset = specificPreset || DEFAULT_PRESET;
    if (recommendedParams && Object.keys(recommendedParams).length > 0) {
      const { repetition_penalty, ...rest } = recommendedParams;
      const ggufParams = {
        ...rest,
        ...(repetition_penalty != null ? { repeat_penalty: repetition_penalty } : {}),
      };
      // Specific preset wins over GGUF; GGUF wins over DEFAULT
      return specificPreset
        ? { ...DEFAULT_PRESET, ...ggufParams, ...specificPreset }
        : { ...namedPreset, ...ggufParams };
    }
    return namedPreset;
  }, [generalName, recommendedParams]);

  const maxContextSize = useMemo(() => {
    if (!modelInfo?.context_length) return 131072;
    const parsed = parseInt(modelInfo.context_length.toString().replace(/,/g, ''));
    return isNaN(parsed) ? 131072 : parsed;
  }, [modelInfo?.context_length]);

  // Auto-calculate optimal gpu_layers and context_size for available VRAM
  const optimized = useVramOptimizer({
    modelMetadata: modelInfo,
    availableVramGb,
    maxLayers,
    cacheTypeK: resolvedPreset.cache_type_k || 'f16',
    cacheTypeV: resolvedPreset.cache_type_v || 'f16',
    presetContextSize: resolvedPreset.context_size,
    maxContextSize,
  });

  // Calculate memory breakdown in real-time
  const memoryBreakdown = useMemoryCalculation({
    modelMetadata: modelInfo,
    gpuLayers: config.gpu_layers || 0,
    contextSize: contextSize,
    availableVramGb,
    availableRamGb,
    overheadGb,
    cacheTypeK: config.cache_type_k || resolvedPreset.cache_type_k || 'f16',
    cacheTypeV: config.cache_type_v || resolvedPreset.cache_type_v || 'f16',
  });

  // Initialize model path from config when modal opens
  useEffect(() => {
    if (isOpen && initialModelPath && !modelPath) {
      setModelPath(initialModelPath);
    }
  }, [isOpen, initialModelPath, modelPath]);

  // Fetch model history and saved config when modal opens
  useEffect(() => {
    if (!isOpen) return;

    const fetchHistory = async () => {
      try {
        const history = await getModelHistory();
        setModelHistory(history);
      } catch (error) {
        console.error('Failed to fetch model history:', error);
      }
    };

    const fetchSavedConfig = async () => {
      try {
        const saved = await getConfig();
        if (saved.tag_pairs?.length) {
          setConfig(prev => ({ ...prev, tag_pairs: saved.tag_pairs }));
          console.log('[ModelConfig] Loaded saved tag_pairs from DB:', saved.tag_pairs.length, 'pairs');
        }
      } catch (error) {
        console.error('Failed to fetch saved config:', error);
      }
      savedConfigLoaded.current = true;
    };

    fetchHistory();
    fetchSavedConfig();
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
  // If a preset specifies a smaller context_size, the preset useEffect below will override this
  useEffect(() => {
    if (modelInfo?.context_length) {
      const maxContext = parseInt(modelInfo.context_length.toString().replace(/,/g, ''));
      if (!isNaN(maxContext)) {
        // Set to model's max — VRAM optimizer will adjust to fit available memory
        setContextSize(maxContext);
      }
    }
  }, [modelInfo]);

  // Auto-apply recommended sampling parameters when model info loads
  useEffect(() => {
    if (!generalName && !recommendedParams) return;

    // Specific preset = hand-tuned for this model (takes priority over GGUF sampling params)
    const specificPreset = findPresetByName(generalName || '');
    const namedPreset = specificPreset || DEFAULT_PRESET;

    // GGUF embedded params (from general.sampling.* keys in the GGUF file)
    let ggufParams: Partial<SamplerConfig> = {};
    if (recommendedParams && Object.keys(recommendedParams).length > 0) {
      const { repetition_penalty, ...rest } = recommendedParams;
      ggufParams = {
        ...rest,
        ...(repetition_penalty != null ? { repeat_penalty: repetition_penalty } : {}),
      };
    }

    // Merge: specific preset wins over GGUF; GGUF wins over DEFAULT
    const merged = specificPreset
      ? { ...DEFAULT_PRESET, ...ggufParams, ...specificPreset }
      : { ...namedPreset, ...ggufParams };

    // Apply the preset (including context_size if specified)
    const { context_size: presetContextSize, ...samplerPreset } = merged as Partial<SamplerConfig> & { context_size?: number };
    setConfig(prev => ({
      ...prev,
      ...samplerPreset,
      model_path: prev.model_path,
    }));
    if (presetContextSize) {
      setContextSize(presetContextSize);
    }

    console.log('[ModelConfig] Auto-applied preset for:', generalName, merged);
  }, [generalName, recommendedParams]);

  // Auto-populate tag_pairs from detected_tag_pairs only when config has none yet
  // Wait for saved config to load first to avoid overwriting DB-saved pairs
  useEffect(() => {
    if (!modelInfo?.detected_tag_pairs?.length) return;
    if (!savedConfigLoaded.current) {
      // Saved config not loaded yet — schedule a retry
      const timer = setTimeout(() => {
        if (!savedConfigLoaded.current) return; // still not ready, give up
        setConfig(prev => {
          if (prev.tag_pairs?.length) return prev; // DB had saved pairs
          return { ...prev, tag_pairs: modelInfo.detected_tag_pairs };
        });
      }, 300);
      return () => clearTimeout(timer);
    }
    // Saved config already loaded — only populate if empty
    setConfig(prev => {
      if (prev.tag_pairs?.length) return prev;
      console.log('[ModelConfig] Auto-populated tag pairs:', modelInfo.detected_tag_pairs!.length, 'pairs');
      return { ...prev, tag_pairs: modelInfo.detected_tag_pairs };
    });
  }, [modelInfo?.detected_tag_pairs]); // eslint-disable-line react-hooks/exhaustive-deps

  // Auto-apply VRAM-optimized gpu_layers and context_size once per model
  const autoOptimizedForPath = useRef('');
  useEffect(() => {
    if (optimized.ready && modelPath && autoOptimizedForPath.current !== modelPath) {
      autoOptimizedForPath.current = modelPath;
      setConfig(prev => ({ ...prev, gpu_layers: optimized.optimalGpuLayers }));
      setContextSize(optimized.optimalContextSize);
      console.log('[ModelConfig] VRAM auto-optimized:', {
        gpuLayers: optimized.optimalGpuLayers,
        contextSize: optimized.optimalContextSize,
        kvAttentionLayers: optimized.kvAttentionLayers,
      });
    }
  }, [optimized, modelPath]);

  const handleInputChange = (field: keyof SamplerConfig, value: string | number | boolean) => {
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
    // 'system' = '__AGENTIC__' (use universal agentic prompt), 'custom' = user's prompt
    const systemPrompt = systemPromptMode === 'system'
      ? '__AGENTIC__'
      : customSystemPrompt;

    // Derive tool_tag_* fields from tag_pairs for backward compatibility
    const tagPairs = config.tag_pairs || [];
    const execPair = tagPairs.find(p => p.category === 'tool' && p.name === 'exec' && p.enabled);
    const respPair = tagPairs.find(p => p.category === 'tool' && p.name === 'response' && p.enabled);

    const finalConfig = {
      ...config,
      model_path: modelPath,
      context_size: contextSize,
      system_prompt: systemPrompt,
      ...(execPair ? { tool_tag_exec_open: execPair.open_tag, tool_tag_exec_close: execPair.close_tag } : {}),
      ...(respPair ? { tool_tag_output_open: respPair.open_tag, tool_tag_output_close: respPair.close_tag } : {}),
    };
    console.log('[DEBUG] Saving config with system_prompt:', systemPrompt, 'mode:', systemPromptMode);

    // Warn if model's native tool format was not detected (fell back to default SYSTEM.EXEC)
    const detectedTags = modelInfo?.detected_tool_tags;
    if (detectedTags && detectedTags.exec_open === '<||SYSTEM.EXEC>') {
      setTimeout(() => {
        toast('Tool call format not detected for this model. Using default format — this may affect agentic tasks.', {
          icon: '\u26A0\uFE0F',
          duration: 6000,
        });
      }, 1500); // Delay so it appears after the "loaded successfully" toast
    }

    onSave(finalConfig);
  };

  const handleBrowseFile = async () => {
    if (isPicking) return;
    setIsPicking(true);
    try {
      const filePath = await pickFile();
      if (filePath) {
        setModelPath(filePath);
      }
    } catch (error) {
      console.error('Error opening file dialog:', error);
    } finally {
      setIsPicking(false);
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

              {isCheckingFile ? <Card className="mt-3">
                  <CardContent className="pt-4">
                    <div className="flex items-center gap-2">
                      <Loader2 className="h-4 w-4 animate-spin" />
                      <p className="text-sm text-muted-foreground">Reading GGUF metadata...</p>
                    </div>
                  </CardContent>
                </Card> : null}

              {modelInfo ? <ModelMetadataDisplay
                  modelInfo={modelInfo}
                /> : null}
            </CardContent>
          </Card>

          {/* Configuration Options - Only show when model is valid */}
          {modelPath && fileExists === true ? <Card>
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
              {isConfigExpanded ? <CardContent className="space-y-4 pt-6">
                  {/* Memory Visualization - Real-time VRAM/RAM usage */}
                  {modelInfo ? <MemoryVisualization
                      memory={memoryBreakdown}
                      overheadGb={overheadGb}
                      onOverheadChange={setOverheadGb}
                      gpuLayers={config.gpu_layers || 0}
                      onGpuLayersChange={(layers) => handleInputChange('gpu_layers', layers)}
                      maxLayers={maxLayers}
                      contextSize={contextSize}
                      onContextSizeChange={setContextSize}
                      maxContextSize={maxContextSize}
                    /> : null}

                  <ModelConfigSystemPrompt
                    systemPromptMode={systemPromptMode}
                    setSystemPromptMode={setSystemPromptMode}
                    customSystemPrompt={customSystemPrompt}
                    setCustomSystemPrompt={setCustomSystemPrompt}
                  />

                  <AdvancedContextSection
                    config={config}
                    onConfigChange={handleInputChange}
                  />

                  <SamplingParametersSection
                    config={config}
                    onConfigChange={handleInputChange}
                  />

                  <TagPairsSection
                    tagPairs={config.tag_pairs || []}
                    detectedTagPairs={modelInfo?.detected_tag_pairs}
                    onTagPairsChange={(pairs) => setConfig(prev => ({ ...prev, tag_pairs: pairs }))}
                  />


                </CardContent> : null}
            </Card> : null}

        </div>

        <DialogFooter className="px-6 py-4 border-t">
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button data-testid="load-model-button" onClick={handleSave} disabled={!modelPath.trim() || isCheckingFile}>
            {isCheckingFile ? (
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
