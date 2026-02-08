import React, { useState, useEffect } from 'react';
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
import { getModelHistory, getSystemUsage } from '@/utils/tauriCommands';

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

// Import model presets for auto-configuration
import { findPresetByName, DEFAULT_PRESET } from '@/config/modelPresets';

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
    repeat_penalty: 1.0,
    min_p: 0,
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
  const [availableVramGb, setAvailableVramGb] = useState(24.0); // Default until detected
  const [availableRamGb, setAvailableRamGb] = useState(64.0);   // Default until detected

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
  }, [isOpen, initialModelPath, modelPath]);

  // Fetch model history and hardware info when modal opens
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

    const fetchHardwareInfo = async () => {
      try {
        const usage = await getSystemUsage();
        if (usage.total_vram_gb && usage.total_vram_gb > 0) {
          setAvailableVramGb(usage.total_vram_gb);
        }
        if (usage.total_ram_gb && usage.total_ram_gb > 0) {
          setAvailableRamGb(usage.total_ram_gb);
        }
      } catch (error) {
        console.error('Failed to fetch hardware info:', error);
      }
    };

    fetchHistory();
    fetchHardwareInfo();
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

  // Auto-apply recommended sampling parameters when model info loads
  const generalName = modelInfo?.general_name;
  const recommendedParams = modelInfo?.recommended_params;
  useEffect(() => {
    if (!generalName && !recommendedParams) return;

    // First try GGUF embedded params, then preset lookup, then default
    let preset: Partial<SamplerConfig> | null = null;

    // Check for GGUF embedded params
    if (recommendedParams && Object.keys(recommendedParams).length > 0) {
      const { repetition_penalty, ...rest } = recommendedParams;
      preset = {
        ...rest,
        ...(repetition_penalty != null ? { repeat_penalty: repetition_penalty } : {}),
      };
    }

    // Fallback to preset lookup by model name
    if (!preset) {
      preset = findPresetByName(generalName || '');
    }

    // Final fallback to default
    if (!preset) {
      preset = DEFAULT_PRESET;
    }

    // Apply the preset
    setConfig(prev => ({
      ...prev,
      ...preset,
      model_path: prev.model_path,
    }));

    console.log('[ModelConfig] Auto-applied preset for:', generalName, preset);
  }, [generalName, recommendedParams]);

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
