import React, { useState, useEffect } from 'react';
import { Brain, FolderOpen, Loader2, ChevronDown, ChevronRight, CheckCircle, XCircle, Clock } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Button } from '@/components/ui/button';
import { Slider } from '@/components/ui/slider';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import type { SamplerConfig, SamplerType, ModelMetadata } from '../types';

interface ModelConfigModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSave: (config: SamplerConfig) => void;
  isLoading?: boolean;
}

const SAMPLER_OPTIONS: SamplerType[] = [
  'Greedy',
  'Temperature', 
  'Mirostat',
  'TopP',
  'TopK',
  'Typical',
  'MinP',
  'TempExt',
  'ChainTempTopP',
  'ChainTempTopK',
  'ChainFull'
];

const SAMPLER_DESCRIPTIONS: Record<SamplerType, string> = {
  'Greedy': 'Deterministic selection - always picks the most likely token',
  'Temperature': 'Controls randomness in text generation',
  'Mirostat': 'Advanced perplexity-based sampling method',
  'TopP': 'Nucleus sampling - considers top tokens by cumulative probability',
  'TopK': 'Considers only the top K most likely tokens',
  'Typical': 'Selects tokens with typical information content',
  'MinP': 'Minimum probability threshold sampling',
  'TempExt': 'Extended temperature sampling with enhanced control',
  'ChainTempTopP': 'Chains Temperature and Top-P sampling methods',
  'ChainTempTopK': 'Chains Temperature and Top-K sampling methods',
  'ChainFull': 'Full chain sampling (IBM recommended for best results)'
};

const CONTEXT_SIZE_OPTIONS = [
  { value: 2048, label: '2K - Fast, minimal memory' },
  { value: 4096, label: '4K - Good for most tasks' },
  { value: 8192, label: '8K - Balanced performance' },
  { value: 16384, label: '16K - Large contexts' },
  { value: 32768, label: '32K - Very large contexts' },
  { value: 65536, label: '64K - Maximum contexts' },
];

export const ModelConfigModal: React.FC<ModelConfigModalProps> = ({ 
  isOpen, 
  onClose, 
  onSave,
  isLoading = false
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
  const [isConfigExpanded, setIsConfigExpanded] = useState(false);
  const [fileExists, setFileExists] = useState<boolean | null>(null);
  const [isCheckingFile, setIsCheckingFile] = useState(false);
  const [directorySuggestions, setDirectorySuggestions] = useState<string[]>([]);
  const [directoryError, setDirectoryError] = useState<string | null>(null);

  // Check if we're in Tauri environment
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

  useEffect(() => {
    if (modelPath) {
      setConfig(prev => ({
        ...prev,
        model_path: modelPath
      }));
    }
  }, [modelPath]);

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
            await invoke('get_model_metadata', { modelPath: modelPath });
            setFileExists(true);
          } catch (error) {
            setFileExists(false);
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
              }
            }
          } catch (error) {
            console.error('[DEBUG] Error checking file:', error);
            setFileExists(false);
            setDirectoryError(null);
            setDirectorySuggestions([]);
          }
        }
      } catch (error) {
        setFileExists(false);
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
    
    const finalConfig = {
      ...config,
      model_path: modelPath,
      context_size: contextSize,
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

  const handleGetMetadata = async () => {
    if (!modelPath.trim()) {
      alert('Please enter a file path first.');
      return;
    }

    setIsLoadingInfo(true);
    setModelInfo(null);

    try {
      if (isTauri) {
        // Desktop: Use Tauri command
        const metadata = await invoke<ModelMetadata>('get_model_metadata', { modelPath: modelPath });
        console.log('Received metadata object:', metadata);
        setModelInfo(metadata);
      } else {
        // Web: Use HTTP API
        const encodedPath = encodeURIComponent(modelPath);
        const response = await fetch(`/api/model/info?path=${encodedPath}`);
        
        if (response.ok) {
          const metadata = await response.json();
          console.log('Received metadata object:', JSON.stringify(metadata, null, 2));
          setModelInfo({
            name: metadata.name || modelPath.split(/[\\/]/).pop() || 'Unknown',
            architecture: metadata.architecture || "Unknown",
            parameters: metadata.parameters || "Unknown",
            quantization: metadata.quantization || "Unknown",
            file_size: metadata.file_size || "Unknown",
            context_length: metadata.context_length || "Unknown",
            file_path: modelPath,
          });
        } else {
          throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
      }
    } catch (error) {
      console.error('Failed to get model metadata:', error);
      const fileName = modelPath.split(/[\\/]/).pop() || modelPath;
      setModelInfo({
        name: fileName,
        architecture: "Unknown",
        parameters: "Unknown", 
        quantization: "Unknown",
        file_size: "Unknown",
        context_length: "Unknown",
        file_path: modelPath,
      });
    } finally {
      setIsLoadingInfo(false);
    }
  };




  const getModelFileName = () => {
    if (!modelPath) return 'No model selected';
    const fileName = modelPath.split('/').pop() || modelPath;
    return fileName;
  };

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="max-w-2xl max-h-[80vh] overflow-y-auto">
        <DialogHeader>
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
        
        <div className="space-y-6">
          {/* Model File Selection */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm">Model File Selection</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="space-y-2">
                <div className="flex gap-2">
                  <div className="flex-1 relative">
                    <input
                      type="text"
                      value={modelPath}
                      onChange={(e) => setModelPath(e.target.value)}
                      placeholder={isTauri ? "Select a .gguf file or enter full path" : "Enter full path to .gguf file (e.g., C:\\path\\to\\model.gguf)"}
                      className={`w-full px-3 py-2 pr-8 text-sm border rounded-md bg-background ${
                        fileExists === true ? 'border-green-500' : 
                        fileExists === false ? 'border-red-500' : 
                        'border-input'
                      }`}
                    />
                    {modelPath.trim() && (
                      <div className="absolute right-2 top-1/2 transform -translate-y-1/2">
                        {isCheckingFile ? (
                          <Clock className="h-4 w-4 text-muted-foreground animate-pulse" />
                        ) : fileExists === true ? (
                          <CheckCircle className="h-4 w-4 text-green-500" />
                        ) : fileExists === false ? (
                          <XCircle className="h-4 w-4 text-red-500" />
                        ) : null}
                      </div>
                    )}
                  </div>
                  {isTauri ? (
                    <Button
                      type="button"
                      onClick={handleBrowseFile}
                      disabled={isLoadingInfo}
                      variant="outline"
                      className="flex items-center gap-2 px-3"
                    >
                      {isLoadingInfo ? (
                        <>
                          <Loader2 className="h-4 w-4 animate-spin" />
                          Reading...
                        </>
                      ) : (
                        <>
                          <FolderOpen className="h-4 w-4" />
                          Browse
                        </>
                      )}
                    </Button>
                  ) : (
                    <Button
                      type="button"
                      onClick={handleGetMetadata}
                      disabled={isLoadingInfo || !modelPath.trim()}
                      variant="outline"
                      className="flex items-center gap-2 px-3"
                    >
                      {isLoadingInfo ? (
                        <>
                          <Loader2 className="h-4 w-4 animate-spin" />
                          Reading...
                        </>
                      ) : (
                        <>
                          <Brain className="h-4 w-4" />
                          Get Info
                        </>
                      )}
                    </Button>
                  )}
                </div>
                
                {!isTauri && (
                  <p className="text-xs text-muted-foreground">
                    📝 Web mode: Please enter the full file path manually (e.g., C:\Users\Name\Documents\model.gguf)
                  </p>
                )}
                
                {/* File existence status */}
                {modelPath.trim() && (
                  <div className="text-xs space-y-2">
                    {isCheckingFile ? (
                      <span className="text-muted-foreground flex items-center gap-1">
                        <Clock className="h-3 w-3" />
                        Checking file...
                      </span>
                    ) : fileExists === true ? (
                      <span className="text-green-600 flex items-center gap-1">
                        <CheckCircle className="h-3 w-3" />
                        File found and accessible
                      </span>
                    ) : fileExists === false ? (
                      <>
                        {directoryError ? (
                          <div className="space-y-2">
                            <span className="text-amber-600 flex items-center gap-1">
                              <XCircle className="h-3 w-3" />
                              {directoryError}
                            </span>
                            {directorySuggestions.length > 0 && (
                              <div className="pl-4 space-y-1">
                                {directorySuggestions.map((suggestion, idx) => (
                                  <button
                                    key={idx}
                                    type="button"
                                    onClick={() => {
                                      const basePath = modelPath.trim();
                                      const newPath = basePath.endsWith('\\') || basePath.endsWith('/')
                                        ? `${basePath}${suggestion}`
                                        : `${basePath}\\${suggestion}`;
                                      setModelPath(newPath);
                                    }}
                                    className="block w-full text-left px-3 py-2 text-xs bg-muted hover:bg-accent rounded border border-border transition-colors"
                                  >
                                    📄 {suggestion}
                                  </button>
                                ))}
                              </div>
                            )}
                          </div>
                        ) : (
                          <span className="text-red-600 flex items-center gap-1">
                            <XCircle className="h-3 w-3" />
                            File not found or inaccessible
                          </span>
                        )}
                      </>
                    ) : null}
                  </div>
                )}
                
                {/* Model Information */}
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
                  <Card className="mt-3">
                    <CardHeader className="pb-3">
                      <button
                        className="flex items-center justify-between w-full text-left"
                        onClick={() => setIsMetadataExpanded(!isMetadataExpanded)}
                        type="button"
                      >
                        <CardTitle className="text-sm flex items-center gap-2">
                          {isMetadataExpanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
                          Model Metadata
                        </CardTitle>
                      </button>
                    </CardHeader>
                    {isMetadataExpanded && (
                      <CardContent className="pt-0">
                        <div className="space-y-1 text-xs text-muted-foreground max-h-48 overflow-y-auto">
                          <p><strong>File Size:</strong> {modelInfo.file_size}</p>
                          <p><strong>File Path:</strong> {modelInfo.file_path}</p>
                          <div className="mt-3 p-2 bg-muted rounded text-xs">
                            <p><strong>Note:</strong> Complete GGUF metadata is displayed in the console/terminal.</p>
                            <p>Architecture: {modelInfo.architecture}</p>
                            <p>Parameters: {modelInfo.parameters}</p>
                            <p>Quantization: {modelInfo.quantization}</p>
                            <p>Context Length: {modelInfo.context_length}</p>
                          </div>
                        </div>
                      </CardContent>
                    )}
                  </Card>
                )}
              </div>
            </CardContent>
          </Card>

          {/* Configuration Options - Only show when model is selected */}
          {modelPath && (
            <Card>
              <CardHeader>
                <button
                  className="flex items-center justify-between w-full text-left"
                  onClick={() => setIsConfigExpanded(!isConfigExpanded)}
                  type="button"
                >
                  <CardTitle className="text-sm flex items-center gap-2">
                    {isConfigExpanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
                    Model Configurations
                  </CardTitle>
                </button>
              </CardHeader>
              {isConfigExpanded && (
                <CardContent className="space-y-4 pt-0">
                  {/* Context Size */}
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Context Length</label>
              <Select
                value={contextSize.toString()}
                onValueChange={(value) => setContextSize(parseInt(value))}
              >
                <SelectTrigger>
                  <SelectValue placeholder="Select context size" />
                </SelectTrigger>
                <SelectContent>
                  {CONTEXT_SIZE_OPTIONS.map(option => (
                    <SelectItem key={option.value} value={option.value.toString()}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                Larger context sizes allow longer conversations but use more memory and are slower.
              </p>
                  </div>

                  {/* GPU Layers */}
                  <div className="space-y-2">
                    <div className="flex justify-between items-center">
                      <label className="text-sm font-medium">GPU Layers (CUDA)</label>
                      <span className="text-sm font-mono text-muted-foreground">{config.gpu_layers || 0}</span>
                    </div>
                    <Slider
                      value={[config.gpu_layers || 0]}
                      onValueChange={([value]) => handleInputChange('gpu_layers', value)}
                      max={99}
                      min={0}
                      step={1}
                      className="w-full"
                    />
                    <p className="text-xs text-muted-foreground">
                      Number of model layers to offload to GPU. Higher values = faster inference but more VRAM usage. 0 = CPU only, 32+ recommended for RTX 4090.
                    </p>
                  </div>

          {/* Sampler Type */}
          <div className="space-y-2">
            <label className="text-sm font-medium">Sampler Type</label>
              <Select
                value={config.sampler_type}
                onValueChange={(value) => handleInputChange('sampler_type', value)}
              >
                <SelectTrigger>
                  <SelectValue placeholder="Select a sampler type" />
                </SelectTrigger>
                <SelectContent>
                  {SAMPLER_OPTIONS.map(option => (
                    <SelectItem key={option} value={option}>
                      {option}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                {SAMPLER_DESCRIPTIONS[config.sampler_type as SamplerType] || 'Select a sampler type for more information'}
              </p>
          </div>

          {/* Temperature */}
          <div className="space-y-2">
            <div className="flex justify-between items-center">
              <label className="text-sm font-medium">Temperature</label>
              <span className="text-sm font-mono text-muted-foreground">{config.temperature.toFixed(2)}</span>
            </div>
              <Slider
                value={[config.temperature]}
                onValueChange={([value]) => handleInputChange('temperature', value)}
                max={2}
                min={0}
                step={0.1}
                className="w-full"
              />
              <p className="text-xs text-muted-foreground">
                Higher values make output more random, lower values more focused
              </p>
          </div>

          {/* Top P */}
          <div className="space-y-2">
            <div className="flex justify-between items-center">
              <label className="text-sm font-medium">Top P (Nucleus)</label>
              <span className="text-sm font-mono text-muted-foreground">{config.top_p.toFixed(2)}</span>
            </div>
              <Slider
                value={[config.top_p]}
                onValueChange={([value]) => handleInputChange('top_p', value)}
                max={1}
                min={0}
                step={0.05}
                className="w-full"
              />
              <p className="text-xs text-muted-foreground">
                Only consider tokens that make up the top P probability mass
              </p>
          </div>

          {/* Top K */}
          <div className="space-y-2">
            <div className="flex justify-between items-center">
              <label className="text-sm font-medium">Top K</label>
              <span className="text-sm font-mono text-muted-foreground">{config.top_k}</span>
            </div>
              <Slider
                value={[config.top_k]}
                onValueChange={([value]) => handleInputChange('top_k', Math.round(value))}
                max={100}
                min={1}
                step={1}
                className="w-full"
              />
              <p className="text-xs text-muted-foreground">
                Consider only the top K most likely tokens
              </p>
          </div>

          {/* Mirostat Parameters */}
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <div className="flex justify-between items-center">
                <label className="text-sm font-medium">Mirostat Tau</label>
                <span className="text-sm font-mono text-muted-foreground">{config.mirostat_tau.toFixed(1)}</span>
              </div>
              <Slider
                value={[config.mirostat_tau]}
                onValueChange={([value]) => handleInputChange('mirostat_tau', value)}
                max={10}
                min={0}
                step={0.1}
                className="w-full"
              />
            </div>

            <div className="space-y-2">
              <div className="flex justify-between items-center">
                <label className="text-sm font-medium">Mirostat Eta</label>
                <span className="text-sm font-mono text-muted-foreground">{config.mirostat_eta.toFixed(2)}</span>
              </div>
              <Slider
                value={[config.mirostat_eta]}
                onValueChange={([value]) => handleInputChange('mirostat_eta', value)}
                max={1}
                min={0}
                step={0.01}
                className="w-full"
              />
            </div>
          </div>

          {/* Recommended Presets */}
          <div className="space-y-2">
            <label className="text-sm font-medium">Quick Presets</label>
            <div className="grid grid-cols-2 gap-2">
                <Button 
                  variant="outline" 
                  onClick={() => {
                    setConfig(prev => ({
                      ...prev,
                      sampler_type: 'ChainFull',
                      temperature: 0.7,
                      top_p: 0.95,
                      top_k: 20,
                      gpu_layers: 32,
                    }));
                  }}
                  className="text-xs"
                >
                  IBM Recommended
                </Button>
                <Button 
                  variant="outline" 
                  onClick={() => {
                    setConfig(prev => ({
                      ...prev,
                      sampler_type: 'Greedy',
                      temperature: 0.1,
                      top_p: 0.1,
                      top_k: 1,
                    }));
                  }}
                  className="text-xs"
                >
                  Conservative
                </Button>
                <Button 
                  variant="outline" 
                  onClick={() => {
                    setConfig(prev => ({
                      ...prev,
                      sampler_type: 'Temperature',
                      temperature: 1.2,
                      top_p: 0.8,
                      top_k: 50,
                    }));
                  }}
                  className="text-xs"
                >
                  Creative
                </Button>
                <Button 
                  variant="outline" 
                  onClick={() => {
                    setConfig(prev => ({
                      ...prev,
                      sampler_type: 'Temperature',
                      temperature: 0.7,
                      top_p: 0.95,
                      top_k: 20,
                    }));
                  }}
                  className="text-xs"
                >
                  Balanced
                </Button>
            </div>
          </div>
                </CardContent>
              )}
            </Card>
          )}
        </div>
        
        {isLoading && (
          <div className="px-6 pb-4">
            <div className="flex items-center justify-center gap-2 py-4">
              <Loader2 className="h-5 w-5 animate-spin" />
              <span className="text-sm font-medium">Loading model...</span>
            </div>
          </div>
        )}
        
        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={isLoading}>
            Cancel
          </Button>
          <Button onClick={handleSave} disabled={!modelPath.trim() || isLoadingInfo || isLoading}>
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