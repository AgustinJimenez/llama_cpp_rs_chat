import React, { useState, useEffect, useRef } from 'react';
import { Brain, Upload, FolderOpen, Loader2 } from 'lucide-react';
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
import type { SamplerConfig, SamplerType } from '../types';

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
  });

  const [contextSize, setContextSize] = useState(32768);
  const [modelPath, setModelPath] = useState('');
  const [modelInfo, setModelInfo] = useState<any>(null);
  const [isLoadingInfo, setIsLoadingInfo] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (modelPath) {
      setConfig(prev => ({
        ...prev,
        model_path: modelPath
      }));
    }
  }, [modelPath]);


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
    
    // Log what we're trying to load for debugging
    console.log('Attempting to load model:', modelPath);
    
    const finalConfig = {
      ...config,
      model_path: modelPath,
      context_size: contextSize,
    };
    onSave(finalConfig);
  };

  const handleBrowseFile = () => {
    fileInputRef.current?.click();
  };

  const handleFileSelect = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;

    if (!file.name.endsWith('.gguf')) {
      alert('Please select a .gguf file.');
      return;
    }

    setIsLoadingInfo(true);
    setModelPath(file.name);
    setModelInfo(null);

    try {
      // Read GGUF metadata from the file
      const metadata = await readGGUFMetadata(file);
      setModelInfo(metadata);
    } catch (error) {
      console.error('Failed to read GGUF metadata:', error);
      // Fallback to filename parsing
      const fallbackInfo = createMockModelInfo(file.name);
      setModelInfo({
        ...fallbackInfo,
        error: "Could not read file metadata, showing info from filename"
      });
    } finally {
      setIsLoadingInfo(false);
    }
    
    // Reset the file input
    if (fileInputRef.current) {
      fileInputRef.current.value = '';
    }
  };

  const readGGUFMetadata = async (file: File) => {
    // Read the first part of the file to get GGUF header and metadata
    const headerSize = 32 * 1024; // Read first 32KB which should contain metadata
    const slice = file.slice(0, headerSize);
    const buffer = await slice.arrayBuffer();
    const dataView = new DataView(buffer);
    
    // Check GGUF magic number (first 4 bytes should be "GGUF")
    const magic = new TextDecoder().decode(new Uint8Array(buffer, 0, 4));
    if (magic !== 'GGUF') {
      throw new Error('Not a valid GGUF file');
    }
    
    // Read version (4 bytes)
    const version = dataView.getUint32(4, true);
    
    // Read tensor count (8 bytes)
    const tensorCount = dataView.getBigUint64(8, true);
    
    // Read metadata key-value count (8 bytes)
    const metadataCount = dataView.getBigUint64(16, true);
    
    let offset = 24;
    const metadata: Record<string, any> = {};
    
    // Read metadata key-value pairs
    for (let i = 0; i < Number(metadataCount); i++) {
      if (offset >= buffer.byteLength) break;
      
      try {
        // Read key length (8 bytes)
        const keyLength = dataView.getBigUint64(offset, true);
        offset += 8;
        
        if (offset + Number(keyLength) >= buffer.byteLength) break;
        
        // Read key string
        const key = new TextDecoder().decode(new Uint8Array(buffer, offset, Number(keyLength)));
        offset += Number(keyLength);
        
        if (offset + 4 >= buffer.byteLength) break;
        
        // Read value type (4 bytes)
        const valueType = dataView.getUint32(offset, true);
        offset += 4;
        
        // Read value based on type
        let value: any;
        switch (valueType) {
          case 8: // String
            if (offset + 8 >= buffer.byteLength) break;
            const strLength = dataView.getBigUint64(offset, true);
            offset += 8;
            if (offset + Number(strLength) >= buffer.byteLength) break;
            value = new TextDecoder().decode(new Uint8Array(buffer, offset, Number(strLength)));
            offset += Number(strLength);
            break;
          case 4: // Uint32
            if (offset + 4 >= buffer.byteLength) break;
            value = dataView.getUint32(offset, true);
            offset += 4;
            break;
          case 6: // Float32
            if (offset + 4 >= buffer.byteLength) break;
            value = dataView.getFloat32(offset, true);
            offset += 4;
            break;
          default:
            // Skip unknown types - read next 8 bytes as length and skip
            if (offset + 8 >= buffer.byteLength) break;
            const skipLength = dataView.getBigUint64(offset, true);
            offset += 8 + Number(skipLength);
            continue;
        }
        
        metadata[key] = value;
      } catch (e) {
        console.warn('Error reading metadata entry:', e);
        break;
      }
    }
    
    // Format file size
    const fileSizeGB = (file.size / (1024 * 1024 * 1024)).toFixed(1);
    const fileSize = `${fileSizeGB} GB`;
    
    return {
      name: metadata['general.name'] || metadata['general.basename'] || file.name,
      architecture: metadata['general.architecture'] || metadata['llama.architecture'] || 'Unknown',
      parameters: metadata['general.parameter_count'] || metadata['llama.vocab_size'] || 'Unknown',
      quantization: metadata['general.quantization_version'] || metadata['general.file_type'] || 'Unknown',
      context_length: metadata['llama.context_length'] || metadata['general.max_context_length'] || 'Unknown',
      file_size: fileSize,
      metadata // Include raw metadata for debugging
    };
  };

  const createMockModelInfo = (filename: string) => {
    return {
      name: filename,
      architecture: "Unknown",
      parameters: "Unknown", 
      quantization: "Unknown",
      file_size: "Unknown (local file)",
      context_length: "Unknown"
    };
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
            Configure Model
          </DialogTitle>
          <p className="text-sm text-muted-foreground">
            Model: {getModelFileName()}
          </p>
        </DialogHeader>
        
        <div className="space-y-6">
          {/* Model File Selection */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm">Model File Selection</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="space-y-2">
                <label className="text-sm font-medium text-muted-foreground">
                  Browse and select a model file (.gguf files)
                </label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={modelPath ? getModelFileName() : ''}
                    readOnly
                    placeholder="No model selected - use Browse button to select a .gguf file"
                    className="flex-1 px-3 py-2 text-sm border border-input rounded-md bg-muted cursor-not-allowed"
                  />
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
                </div>
                <input
                  type="file"
                  ref={fileInputRef}
                  onChange={handleFileSelect}
                  accept=".gguf"
                  style={{ display: 'none' }}
                />
                <p className="text-xs text-muted-foreground">
                  Click Browse to select a .gguf model file from your file system.
                </p>
                
                {/* Model Information */}
                {isLoadingInfo && (
                  <div className="mt-3 p-3 bg-blue-50 border border-blue-200 rounded-md">
                    <p className="text-sm text-blue-700">Reading GGUF metadata...</p>
                  </div>
                )}
                
                {modelInfo && (
                  <div className="mt-3 p-3 bg-green-50 border border-green-200 rounded-md">
                    <h4 className="text-sm font-medium text-green-800 mb-2">Model Metadata</h4>
                    <div className="space-y-1 text-xs text-green-700 max-h-48 overflow-y-auto">
                      <p><strong>file_size:</strong> {modelInfo.file_size}</p>
                      {modelInfo.metadata && Object.keys(modelInfo.metadata).map(key => (
                        <p key={key}><strong>{key}:</strong> {String(modelInfo.metadata[key])}</p>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            </CardContent>
          </Card>

          {/* Configuration Options - Only show when model is selected */}
          {modelPath && (
            <>
              {/* Context Size */}
              <Card>
            <CardHeader>
              <CardTitle className="text-sm">Context Length</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
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
            </CardContent>
          </Card>

          {/* Sampler Type */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm">Sampler Type</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
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
            </CardContent>
          </Card>

          {/* Temperature */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm flex justify-between">
                Temperature
                <span className="font-mono text-slate-400">{config.temperature.toFixed(2)}</span>
              </CardTitle>
            </CardHeader>
            <CardContent>
              <Slider
                value={[config.temperature]}
                onValueChange={([value]) => handleInputChange('temperature', value)}
                max={2}
                min={0}
                step={0.1}
                className="w-full"
              />
              <p className="text-xs text-muted-foreground mt-2">
                Higher values make output more random, lower values more focused
              </p>
            </CardContent>
          </Card>

          {/* Top P */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm flex justify-between">
                Top P (Nucleus)
                <span className="font-mono text-slate-400">{config.top_p.toFixed(2)}</span>
              </CardTitle>
            </CardHeader>
            <CardContent>
              <Slider
                value={[config.top_p]}
                onValueChange={([value]) => handleInputChange('top_p', value)}
                max={1}
                min={0}
                step={0.05}
                className="w-full"
              />
              <p className="text-xs text-muted-foreground mt-2">
                Only consider tokens that make up the top P probability mass
              </p>
            </CardContent>
          </Card>

          {/* Top K */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm flex justify-between">
                Top K
                <span className="font-mono text-slate-400">{config.top_k}</span>
              </CardTitle>
            </CardHeader>
            <CardContent>
              <Slider
                value={[config.top_k]}
                onValueChange={([value]) => handleInputChange('top_k', Math.round(value))}
                max={100}
                min={1}
                step={1}
                className="w-full"
              />
              <p className="text-xs text-muted-foreground mt-2">
                Consider only the top K most likely tokens
              </p>
            </CardContent>
          </Card>

          {/* Mirostat Parameters */}
          <div className="grid grid-cols-2 gap-4">
            <Card>
              <CardHeader>
                <CardTitle className="text-sm flex justify-between">
                  Mirostat Tau
                  <span className="font-mono text-slate-400">{config.mirostat_tau.toFixed(1)}</span>
                </CardTitle>
              </CardHeader>
              <CardContent>
                <Slider
                  value={[config.mirostat_tau]}
                  onValueChange={([value]) => handleInputChange('mirostat_tau', value)}
                  max={10}
                  min={0}
                  step={0.1}
                  className="w-full"
                />
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="text-sm flex justify-between">
                  Mirostat Eta
                  <span className="font-mono text-slate-400">{config.mirostat_eta.toFixed(2)}</span>
                </CardTitle>
              </CardHeader>
              <CardContent>
                <Slider
                  value={[config.mirostat_eta]}
                  onValueChange={([value]) => handleInputChange('mirostat_eta', value)}
                  max={1}
                  min={0}
                  step={0.01}
                  className="w-full"
                />
              </CardContent>
            </Card>
          </div>

          {/* Recommended Presets */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm">Quick Presets</CardTitle>
            </CardHeader>
            <CardContent className="space-y-2">
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
            </CardContent>
          </Card>
            </>
          )}
        </div>
        
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={handleSave} disabled={!modelPath.trim() || isLoadingInfo || isLoading}>
            {isLoading ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin mr-2" />
                Loading Model...
              </>
            ) : isLoadingInfo ? 'Reading file...' : 'Load Model'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};