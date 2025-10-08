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
  initialModelPath?: string;
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

const CONTEXT_SIZE_PRESETS = [2048, 4096, 8192, 16384, 32768, 65536, 131072, 262144, 524288, 1048576];

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
  const [isConfigExpanded, setIsConfigExpanded] = useState(false);
  const [fileExists, setFileExists] = useState<boolean | null>(null);
  const [isCheckingFile, setIsCheckingFile] = useState(false);
  const [directorySuggestions, setDirectorySuggestions] = useState<string[]>([]);
  const [directoryError, setDirectoryError] = useState<string | null>(null);
  const [modelHistory, setModelHistory] = useState<string[]>([]);
  const [showHistory, setShowHistory] = useState(false);
  const [maxLayers, setMaxLayers] = useState(99);  // Dynamic max layers based on model
  const [systemPromptMode, setSystemPromptMode] = useState<'default' | 'custom'>('default');
  const [customSystemPrompt, setCustomSystemPrompt] = useState('You are a helpful AI assistant.');

  // Check if we're in Tauri environment
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

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

  // Auto-expand metadata when loaded
  useEffect(() => {
    if (modelInfo) {
      setIsMetadataExpanded(true);
    }
  }, [modelInfo]);

  // Set context size to model's max when metadata is loaded
  useEffect(() => {
    if (modelInfo?.context_length) {
      const maxContext = parseInt(modelInfo.context_length.toString().replace(/,/g, ''));
      if (!isNaN(maxContext)) {
        setContextSize(maxContext);
      }
    }
  }, [modelInfo]);

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

              // Automatically set model metadata
              console.log('[DEBUG] GGUF metadata received:', data.gguf_metadata);
              setModelInfo({
                name: data.name || trimmedPath.split(/[\\/]/).pop() || 'Unknown',
                architecture: data.architecture || "Unknown",
                parameters: data.parameters || "Unknown",
                quantization: data.quantization || "Unknown",
                file_size: data.file_size || "Unknown",
                context_length: data.context_length || "Unknown",
                file_path: trimmedPath,
                estimated_layers: data.estimated_layers,
                gguf_metadata: data.gguf_metadata,
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
      system_prompt: systemPrompt,
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
              <div className="space-y-2">
                <div className="flex gap-2">
                  <div className="flex-1 relative">
                    <input
                      type="text"
                      value={modelPath}
                      onChange={(e) => setModelPath(e.target.value.replace(/"/g, ''))}
                      onFocus={() => setShowHistory(true)}
                      onBlur={() => setTimeout(() => setShowHistory(false), 200)}
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
                    {/* Model History Suggestions */}
                    {showHistory && modelHistory.length > 0 && !modelPath.trim() && (
                      <div className="absolute z-10 w-full mt-1 bg-background border border-input rounded-md shadow-lg max-h-60 overflow-y-auto">
                        <div className="p-2 text-xs text-muted-foreground border-b">
                          Previously used paths:
                        </div>
                        {modelHistory.map((path, idx) => (
                          <button
                            key={idx}
                            type="button"
                            onClick={() => {
                              setModelPath(path);
                              setShowHistory(false);
                            }}
                            className="block w-full text-left px-3 py-2 text-sm hover:bg-accent transition-colors border-b last:border-b-0"
                          >
                            <div className="font-mono text-xs truncate">{path}</div>
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                  {isTauri && (
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
                        <div className="space-y-3 text-xs max-h-96 overflow-y-auto">
                          {/* Basic Info */}
                          <div className="space-y-1">
                            <h4 className="font-semibold text-sm mb-2">Basic Information</h4>
                            <p><strong>File Name:</strong> <span className="text-muted-foreground">{modelInfo.name}</span></p>
                            {modelInfo.general_name && <p><strong>Model Name:</strong> <span className="text-muted-foreground">{modelInfo.general_name}</span></p>}
                            <p><strong>File Size:</strong> <span className="text-muted-foreground">{modelInfo.file_size}</span></p>
                            <p><strong>Architecture:</strong> <span className="text-muted-foreground">{modelInfo.architecture}</span></p>
                            <p><strong>Parameters:</strong> <span className="text-muted-foreground">{modelInfo.parameters}</span></p>
                            <p><strong>Quantization:</strong> <span className="text-muted-foreground">{modelInfo.quantization}</span></p>
                            {modelInfo.file_type && <p><strong>File Type:</strong> <span className="text-muted-foreground">{modelInfo.file_type}</span></p>}
                            {modelInfo.quantization_version && <p><strong>Quant Version:</strong> <span className="text-muted-foreground">{modelInfo.quantization_version}</span></p>}
                          </div>

                          {/* Model Details */}
                          {(modelInfo.description || modelInfo.author || modelInfo.organization || modelInfo.version || modelInfo.license) && (
                            <div className="space-y-1 pt-2 border-t">
                              <h4 className="font-semibold text-sm mb-2">Model Details</h4>
                              {modelInfo.description && <p><strong>Description:</strong> <span className="text-muted-foreground">{modelInfo.description}</span></p>}
                              {modelInfo.author && <p><strong>Author:</strong> <span className="text-muted-foreground">{modelInfo.author}</span></p>}
                              {modelInfo.organization && <p><strong>Organization:</strong> <span className="text-muted-foreground">{modelInfo.organization}</span></p>}
                              {modelInfo.version && <p><strong>Version:</strong> <span className="text-muted-foreground">{modelInfo.version}</span></p>}
                              {modelInfo.license && <p><strong>License:</strong> <span className="text-muted-foreground">{modelInfo.license}</span></p>}
                              {modelInfo.url && (
                                <p><strong>URL:</strong> <a href={modelInfo.url} target="_blank" rel="noopener noreferrer" className="text-blue-500 hover:underline break-all">{modelInfo.url}</a></p>
                              )}
                              {modelInfo.repo_url && (
                                <p><strong>Repository:</strong> <a href={modelInfo.repo_url} target="_blank" rel="noopener noreferrer" className="text-blue-500 hover:underline break-all">{modelInfo.repo_url}</a></p>
                              )}
                            </div>
                          )}

                          {/* Architecture Specs */}
                          {(modelInfo.context_length || modelInfo.block_count || modelInfo.embedding_length || modelInfo.feed_forward_length) && (
                            <div className="space-y-1 pt-2 border-t">
                              <h4 className="font-semibold text-sm mb-2">Architecture Specifications</h4>
                              <p><strong>Context Length:</strong> <span className="text-muted-foreground">{modelInfo.context_length}</span></p>
                              {modelInfo.block_count && <p><strong>Block Count (Layers):</strong> <span className="text-muted-foreground">{modelInfo.block_count}</span></p>}
                              {modelInfo.embedding_length && <p><strong>Embedding Length:</strong> <span className="text-muted-foreground">{modelInfo.embedding_length}</span></p>}
                              {modelInfo.feed_forward_length && <p><strong>FFN Length:</strong> <span className="text-muted-foreground">{modelInfo.feed_forward_length}</span></p>}
                              {modelInfo.attention_head_count && <p><strong>Attention Heads:</strong> <span className="text-muted-foreground">{modelInfo.attention_head_count}</span></p>}
                              {modelInfo.attention_head_count_kv && <p><strong>KV Heads:</strong> <span className="text-muted-foreground">{modelInfo.attention_head_count_kv}</span></p>}
                              {modelInfo.layer_norm_epsilon && <p><strong>Layer Norm Epsilon:</strong> <span className="text-muted-foreground font-mono">{modelInfo.layer_norm_epsilon}</span></p>}
                              {modelInfo.rope_dimension_count && <p><strong>RoPE Dimensions:</strong> <span className="text-muted-foreground">{modelInfo.rope_dimension_count}</span></p>}
                              {modelInfo.rope_freq_base && <p><strong>RoPE Freq Base:</strong> <span className="text-muted-foreground">{modelInfo.rope_freq_base}</span></p>}
                            </div>
                          )}

                          {/* Tokenizer Info */}
                          {(modelInfo.tokenizer_model || modelInfo.bos_token_id || modelInfo.eos_token_id || modelInfo.chat_template) && (
                            <div className="space-y-1 pt-2 border-t">
                              <h4 className="font-semibold text-sm mb-2">Tokenizer Information</h4>
                              {modelInfo.tokenizer_model && <p><strong>Tokenizer Type:</strong> <span className="text-muted-foreground">{modelInfo.tokenizer_model}</span></p>}
                              {modelInfo.bos_token_id && <p><strong>BOS Token ID:</strong> <span className="text-muted-foreground font-mono">{modelInfo.bos_token_id}</span></p>}
                              {modelInfo.eos_token_id && <p><strong>EOS Token ID:</strong> <span className="text-muted-foreground font-mono">{modelInfo.eos_token_id}</span></p>}
                              {modelInfo.padding_token_id && <p><strong>Padding Token ID:</strong> <span className="text-muted-foreground font-mono">{modelInfo.padding_token_id}</span></p>}
                              {modelInfo.chat_template && (
                                <div>
                                  <p><strong>Chat Template:</strong></p>
                                  <pre className="mt-1 p-2 bg-muted rounded text-xs overflow-x-auto whitespace-pre-wrap break-words max-h-32 overflow-y-auto">{modelInfo.chat_template}</pre>
                                </div>
                              )}
                            </div>
                          )}

                          {/* All GGUF Metadata */}
                          {modelInfo.gguf_metadata && Object.keys(modelInfo.gguf_metadata).length > 0 && (
                            <div className="space-y-1 pt-2 border-t">
                              <h4 className="font-semibold text-sm mb-2">All GGUF Metadata</h4>
                              <div className="space-y-1">
                                {Object.entries(modelInfo.gguf_metadata)
                                  .sort(([a], [b]) => a.localeCompare(b))
                                  .map(([key, value]) => (
                                    <p key={key} className="text-xs">
                                      <strong className="font-mono text-blue-600 dark:text-blue-400">{key}:</strong>{' '}
                                      <span className="text-muted-foreground font-mono">
                                        {typeof value === 'string'
                                          ? value
                                          : typeof value === 'object'
                                            ? JSON.stringify(value)
                                            : String(value)}
                                      </span>
                                    </p>
                                  ))}
                              </div>
                            </div>
                          )}

                          {/* File Path at the end */}
                          <div className="pt-2 border-t">
                            <p className="text-xs"><strong>File Path:</strong></p>
                            <p className="text-xs text-muted-foreground font-mono break-all mt-1">{modelInfo.file_path}</p>
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
                    <div className="flex justify-between items-center">
                      <label className="text-sm font-medium">Context Length</label>
                      <span className="text-sm font-mono text-muted-foreground">
                        {contextSize.toLocaleString()} tokens
                      </span>
                    </div>

                    <input
                      type="number"
                      value={contextSize}
                      onChange={(e) => {
                        const value = parseInt(e.target.value);
                        if (!isNaN(value) && value > 0) {
                          setContextSize(value);
                        }
                      }}
                      min={512}
                      max={2097152}
                      step={512}
                      className="w-full px-3 py-2 text-sm border rounded-md bg-background"
                    />

                    <div className="flex gap-2 flex-wrap">
                      {CONTEXT_SIZE_PRESETS.map(preset => (
                        <Button
                          key={preset}
                          type="button"
                          variant={contextSize === preset ? 'default' : 'outline'}
                          size="sm"
                          onClick={() => setContextSize(preset)}
                          className="text-xs"
                        >
                          {preset >= 1048576 ? `${preset / 1048576}M` : preset >= 1024 ? `${preset / 1024}K` : preset}
                        </Button>
                      ))}
                      {modelInfo?.context_length && (
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          onClick={() => {
                            const maxContext = parseInt(modelInfo.context_length.toString().replace(/,/g, ''));
                            if (!isNaN(maxContext)) {
                              setContextSize(maxContext);
                            }
                          }}
                          className="text-xs bg-blue-50 dark:bg-blue-950 border-blue-300 dark:border-blue-700"
                        >
                          Max ({parseInt(modelInfo.context_length.toString().replace(/,/g, '')).toLocaleString()})
                        </Button>
                      )}
                    </div>

                    <p className="text-xs text-muted-foreground">
                      Larger context sizes allow longer conversations but use more memory and are slower.
                      {modelInfo?.context_length && ` Model maximum: ${modelInfo.context_length}`}
                    </p>
                  </div>

                  {/* System Prompt */}
                  <div className="space-y-3 pt-2 border-t">
                    <label className="text-sm font-medium">System Prompt</label>

                    {/* Toggle between Default and Custom */}
                    <div className="flex gap-2">
                      <Button
                        type="button"
                        variant={systemPromptMode === 'default' ? 'default' : 'outline'}
                        onClick={() => setSystemPromptMode('default')}
                        className="flex-1"
                      >
                        Use Model Default
                      </Button>
                      <Button
                        type="button"
                        variant={systemPromptMode === 'custom' ? 'default' : 'outline'}
                        onClick={() => setSystemPromptMode('custom')}
                        className="flex-1"
                      >
                        Custom Prompt
                      </Button>
                    </div>

                    {/* Show model's default prompt or custom input based on mode */}
                    {systemPromptMode === 'default' ? (
                      <div className="space-y-2">
                        <p className="text-xs text-muted-foreground">
                          Using the model's built-in default system prompt from chat template.
                        </p>
                        {modelInfo?.default_system_prompt && (
                          <div className="p-3 bg-muted rounded-md text-xs max-h-40 overflow-y-auto">
                            <pre className="whitespace-pre-wrap break-words text-muted-foreground">
                              {modelInfo.default_system_prompt}
                            </pre>
                          </div>
                        )}
                      </div>
                    ) : (
                      <div className="space-y-2">
                        <textarea
                          value={customSystemPrompt}
                          onChange={(e) => setCustomSystemPrompt(e.target.value)}
                          placeholder="Enter your custom system prompt..."
                          className="w-full px-3 py-2 text-sm border rounded-md bg-background min-h-[100px] resize-y"
                        />
                        <p className="text-xs text-muted-foreground">
                          Custom system prompt that will be used instead of the model's default.
                        </p>
                      </div>
                    )}
                  </div>

                  {/* GPU Layers */}
                  <div className="space-y-2">
                    <div className="flex justify-between items-center">
                      <label className="text-sm font-medium">GPU Layers (CUDA)</label>
                      <span className="text-sm font-mono text-muted-foreground">{config.gpu_layers || 0} / {maxLayers}</span>
                    </div>

                    {/* Visual representation of GPU vs CPU split */}
                    <div className="relative w-full h-8 rounded-md overflow-hidden border border-border bg-background">
                      <div className="absolute inset-0 flex">
                        {/* GPU portion (green) */}
                        <div
                          className="h-full bg-gradient-to-r from-green-600 to-green-500 transition-all duration-200"
                          style={{ width: `${((config.gpu_layers || 0) / maxLayers) * 100}%` }}
                        >
                          {(config.gpu_layers || 0) > 0 && (
                            <div className="h-full flex items-center justify-center text-xs font-semibold text-white">
                              GPU
                            </div>
                          )}
                        </div>
                        {/* CPU portion (light gray) */}
                        <div
                          className="h-full bg-gradient-to-r from-slate-300 to-slate-200 transition-all duration-200"
                          style={{ width: `${((maxLayers - (config.gpu_layers || 0)) / maxLayers) * 100}%` }}
                        >
                          {(maxLayers - (config.gpu_layers || 0)) > (maxLayers * 0.1) && (
                            <div className="h-full flex items-center justify-center text-xs font-semibold text-slate-700">
                              CPU
                            </div>
                          )}
                        </div>
                      </div>
                    </div>

                    <Slider
                      value={[config.gpu_layers || 0]}
                      onValueChange={([value]) => handleInputChange('gpu_layers', value)}
                      max={maxLayers}
                      min={0}
                      step={1}
                      className="w-full"
                    />
                    <p className="text-xs text-muted-foreground">
                      Number of model layers to offload to GPU. Higher values = faster inference but more VRAM usage. 0 = CPU only. Model has ~{maxLayers} layers total.
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