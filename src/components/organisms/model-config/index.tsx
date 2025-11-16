import React, { useState, useEffect } from 'react';
import { Brain, Loader2, ChevronDown, ChevronRight } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../../atoms/dialog';
import { Button } from '../../atoms/button';
import { Card, CardContent, CardHeader, CardTitle } from '../../atoms/card';
import type { SamplerConfig, ModelMetadata } from '@/types';

// Import extracted components
import { ModelFileInput, ModelConfigSystemPrompt } from '../../molecules';
import { ModelMetadataDisplay } from './ModelMetadataDisplay';
import { ContextSizeSection } from './ContextSizeSection';
import { GpuLayersSection } from './GpuLayersSection';
import { SamplingParametersSection } from './SamplingParametersSection';
import { PresetsSection } from './PresetsSection';
import { MemoryVisualization } from './MemoryVisualization';

// Import hooks
import { useMemoryCalculation } from '@/hooks/useMemoryCalculation';

interface ModelConfigModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSave: (config: SamplerConfig) => void;
  isLoading?: boolean;
  initialModelPath?: string;
}

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
  const [modelInfo, setModelInfo] = useState<ModelMetadata | null>(null);
  const [isLoadingInfo, setIsLoadingInfo] = useState(false);
  const [isMetadataExpanded, setIsMetadataExpanded] = useState(false);
  const [isConfigExpanded, setIsConfigExpanded] = useState(true);
  const [fileExists, setFileExists] = useState<boolean | null>(null);
  const [isCheckingFile, setIsCheckingFile] = useState(false);
  const [directorySuggestions, setDirectorySuggestions] = useState<string[]>([]);
  const [directoryError, setDirectoryError] = useState<string | null>(null);
  const [modelHistory, setModelHistory] = useState<string[]>([]);
  const [showHistory, setShowHistory] = useState(false);
  const [maxLayers, setMaxLayers] = useState(99);  // Dynamic max layers based on model
  const [systemPromptMode, setSystemPromptMode] = useState<'default' | 'custom'>('default');
  const [customSystemPrompt, setCustomSystemPrompt] = useState('You are a helpful AI assistant.');
  const [availableVramGb, _setAvailableVramGb] = useState(22.0); // Default: RTX 4090
  const [availableRamGb, _setAvailableRamGb] = useState(32.0);   // Default: 32GB

  // Check if we're in Tauri environment
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

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
  }, [isOpen, initialModelPath]);

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

  // Helper function to save model path to history
  const saveToHistory = async (path: string) => {
    try {
      await fetch('/api/model/history', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ model_path: path }),
      });
    } catch (error) {
      console.error('Failed to save model path to history:', error);
    }
  };

  // Debounced file existence check
  useEffect(() => {
    if (!modelPath.trim()) {
      setFileExists(null);
      return;
    }

    const checkFileExists = async () => {
      setIsCheckingFile(true);
      try {
        if (isTauri) {
          // For Tauri, we can use the filesystem API
          const { invoke } = await import('@tauri-apps/api/core');
          try {
            // Try to get metadata - if it succeeds, file exists
            const metadata = await invoke<ModelMetadata>('get_model_metadata', { modelPath: modelPath });
            setFileExists(true);

            // Save to history when file is validated
            await saveToHistory(modelPath);

            // Automatically set all model metadata
            console.log('[DEBUG] Tauri metadata received:', metadata);
            console.log('[DEBUG] GGUF metadata:', metadata.gguf_metadata);
            setModelInfo(metadata);

            // Update max layers if available
            if (metadata.estimated_layers) {
              setMaxLayers(metadata.estimated_layers);
            }
          } catch (error) {
            setFileExists(false);
            setModelInfo(null);
          }
        } else {
          // For web, make a GET request to check if file exists on server
          try {
            // Trim whitespace from path before encoding
            const trimmedPath = modelPath.trim();
            const encodedPath = encodeURIComponent(trimmedPath);
            console.log('[DEBUG] Checking file existence for:', trimmedPath);
            console.log('[DEBUG] Encoded path:', encodedPath);
            console.log('[DEBUG] Request URL:', `/api/model/info?path=${encodedPath}`);

            const response = await fetch(`/api/model/info?path=${encodedPath}`);
            console.log('[DEBUG] Response status:', response.status, response.statusText);
            console.log('[DEBUG] Response OK:', response.ok);

            if (response.ok) {
              const data = await response.json();
              console.log('[DEBUG] File info:', data);
              setFileExists(true);
              setDirectoryError(null);
              setDirectorySuggestions([]);

              // Save to history when file is validated
              await saveToHistory(trimmedPath);

              // Automatically set model metadata
              console.log('[DEBUG] GGUF metadata received:', data.gguf_metadata);

              // Parse file_size_gb from file_size string (e.g., "11.65 GB" → 11.65)
              let fileSizeGb: number | undefined;
              if (data.file_size && typeof data.file_size === 'string') {
                const match = data.file_size.match(/([\d.]+)\s*GB/i);
                if (match) {
                  fileSizeGb = parseFloat(match[1]);
                }
              }

              setModelInfo({
                name: data.name || trimmedPath.split(/[\\/]/).pop() || 'Unknown',
                architecture: data.architecture || "Unknown",
                parameters: data.parameters || "Unknown",
                quantization: data.quantization || "Unknown",
                file_size: data.file_size || "Unknown",
                file_size_gb: fileSizeGb,
                context_length: data.context_length || "Unknown",
                file_path: trimmedPath,
                estimated_layers: data.estimated_layers,
                gguf_metadata: data.gguf_metadata,
                // Extract architecture details if available
                block_count: data.gguf_metadata?.['gemma3.block_count'] || data.gguf_metadata?.['llama.block_count'],
                attention_head_count_kv: data.gguf_metadata?.['gemma3.attention.head_count_kv'] || data.gguf_metadata?.['llama.attention.head_count_kv'],
                embedding_length: data.gguf_metadata?.['gemma3.embedding_length'] || data.gguf_metadata?.['llama.embedding_length'],
              });

              // Update max layers if available
              if (data.estimated_layers) {
                setMaxLayers(data.estimated_layers);
              }
            } else {
              // Check if it's a directory error with suggestions
              const errorData = await response.json();
              console.log('[DEBUG] Error response:', errorData);

              if (errorData.is_directory && errorData.suggestions) {
                console.log('[DEBUG] Directory detected with suggestions:', errorData.suggestions);
                setDirectoryError(errorData.error);
                setDirectorySuggestions(errorData.suggestions);
                setFileExists(false);

                // Auto-complete if there's only one .gguf file
                if (errorData.suggestions.length === 1) {
                  const autoPath = trimmedPath.endsWith('\\') || trimmedPath.endsWith('/')
                    ? `${trimmedPath}${errorData.suggestions[0]}`
                    : `${trimmedPath}\\${errorData.suggestions[0]}`;
                  console.log('[DEBUG] Auto-completing with only suggestion:', autoPath);
                  setModelPath(autoPath);
                }
              } else {
                setFileExists(false);
                setDirectoryError(null);
                setDirectorySuggestions([]);
                setModelInfo(null);
              }
            }
          } catch (error) {
            console.error('[DEBUG] Error checking file:', error);
            setFileExists(false);
            setDirectoryError(null);
            setDirectorySuggestions([]);
            setModelInfo(null);
          }
        }
      } catch (error) {
        setFileExists(false);
        setModelInfo(null);
      } finally {
        setIsCheckingFile(false);
      }
    };

    // Debounce the check by 500ms
    const timeoutId = setTimeout(checkFileExists, 500);
    return () => clearTimeout(timeoutId);
  }, [modelPath, isTauri]);



  const handleInputChange = (field: keyof SamplerConfig, value: string | number) => {
    setConfig(prev => ({
      ...prev,
      [field]: value
    }));
  };

  const handleSave = () => {
    if (!modelPath.trim()) {
      alert('Please select a model file or enter a model path.');
      return;
    }

    if (fileExists === false) {
      alert('The specified file does not exist or is not accessible. Please check the path and try again.');
      return;
    }

    // Log what we're trying to load for debugging
    console.log('Attempting to load model:', modelPath);
    console.log('Model path type:', typeof modelPath);
    console.log('Full model path being saved:', modelPath);
    console.log('File exists:', fileExists);

    // Determine system prompt based on mode
    let systemPrompt: string | null = null;
    if (systemPromptMode === 'default') {
      // Use null to let backend use model's default from chat template
      systemPrompt = null;
    } else {
      // Use custom prompt
      systemPrompt = customSystemPrompt;
    }

    const finalConfig = {
      ...config,
      model_path: modelPath,
      context_size: contextSize,
      system_prompt: systemPrompt ?? undefined,
    };
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

        setIsLoadingInfo(true);
        setModelPath(filePath);
        setModelInfo(null);

        try {
          const metadata = await invoke<ModelMetadata>('get_model_metadata', { modelPath: filePath });
          console.log('Received metadata object:', metadata);
          setModelInfo(metadata);

          // Update max layers if available
          if (metadata.estimated_layers) {
            setMaxLayers(metadata.estimated_layers);
          }
        } catch (error) {
          console.error('Failed to get model metadata:', error);
          const fileName = filePath.split(/[\\/]/).pop() || filePath;
          setModelInfo({
            name: fileName,
            architecture: "Unknown",
            parameters: "Unknown",
            quantization: "Unknown",
            file_size: "Unknown",
            context_length: "Unknown",
            file_path: filePath,
          });
        } finally {
          setIsLoadingInfo(false);
        }
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
          {modelPath && (
            <p className="text-sm text-muted-foreground">
              Model: {getModelFileName()}
            </p>
          )}
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
                isLoadingInfo={isLoadingInfo}
                handleBrowseFile={handleBrowseFile}
              />

              {isLoadingInfo && (
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

                  <PresetsSection
                    onApplyPreset={(preset) => setConfig(prev => ({ ...prev, ...preset }))}
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
          <Button data-testid="load-model-button" onClick={handleSave} disabled={!modelPath.trim() || isLoadingInfo || isLoading}>
            {isLoading ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin mr-2" />
                Loading...
              </>
            ) : isLoadingInfo ? (
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
