import React, { useState, useEffect } from 'react';
import { Settings as SettingsIcon, FolderOpen } from 'lucide-react';
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
import { useSettings } from '../hooks/useSettings';
import { FileBrowser } from './FileBrowser';
import type { SamplerConfig, SamplerType } from '../types';

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
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

export const SettingsModal: React.FC<SettingsModalProps> = ({ isOpen, onClose }) => {
  const { config, isLoading, error, updateConfig } = useSettings();
  const [localConfig, setLocalConfig] = useState<SamplerConfig | null>(null);
  const [isFileBrowserOpen, setIsFileBrowserOpen] = useState(false);

  useEffect(() => {
    if (config) {
      setLocalConfig(config);
    }
  }, [config]);

  const handleSave = async () => {
    if (localConfig) {
      await updateConfig(localConfig);
      onClose();
    }
  };

  const handleInputChange = (field: keyof SamplerConfig, value: string | number) => {
    if (localConfig) {
      setLocalConfig({
        ...localConfig,
        [field]: value
      });
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="max-w-2xl max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <SettingsIcon className="h-5 w-5" />
            Configuration
          </DialogTitle>
        </DialogHeader>
        
        {isLoading && (
          <div className="flex items-center justify-center py-8">
            <div className="text-sm text-muted-foreground">Loading configuration...</div>
          </div>
        )}
        
        {error && (
          <Card className="border-destructive">
            <CardContent className="p-4">
              <div className="text-destructive text-sm">Error: {error}</div>
            </CardContent>
          </Card>
        )}
        
        {localConfig && (
          <div className="space-y-6">
            {/* Model Path */}
            <Card>
              <CardHeader>
                <CardTitle className="text-sm">Model Path</CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={localConfig.model_path || '/app/models/'}
                    onChange={(e) => handleInputChange('model_path', e.target.value)}
                    placeholder="/app/models/model.gguf"
                    className="flex-1 px-3 py-2 text-sm border border-input rounded-md bg-background"
                    readOnly
                  />
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => setIsFileBrowserOpen(true)}
                    className="flex items-center gap-2 px-3"
                  >
                    <FolderOpen className="h-4 w-4" />
                    Browse
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground">
                  Select a GGUF model file for LLaMA inference
                </p>
              </CardContent>
            </Card>

            {/* System Prompt */}
            <Card>
              <CardHeader>
                <CardTitle className="text-sm">System Prompt</CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <textarea
                  value={localConfig.system_prompt || ''}
                  onChange={(e) => handleInputChange('system_prompt', e.target.value)}
                  placeholder="Enter system prompt for new conversations..."
                  className="w-full px-3 py-2 text-sm border border-input rounded-md bg-background min-h-[100px] resize-vertical"
                  rows={4}
                />
                <p className="text-xs text-muted-foreground">
                  This prompt will be added at the beginning of every new conversation to set the AI's behavior and context.
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
                  value={localConfig.sampler_type}
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
                  {SAMPLER_DESCRIPTIONS[localConfig.sampler_type as SamplerType] || 'Select a sampler type for more information'}
                </p>
              </CardContent>
            </Card>

            {/* Temperature */}
            <Card>
              <CardHeader>
                <CardTitle className="text-sm flex justify-between">
                  Temperature
                  <span className="font-mono text-slate-400">{localConfig.temperature.toFixed(2)}</span>
                </CardTitle>
              </CardHeader>
              <CardContent>
                <Slider
                  value={[localConfig.temperature]}
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
                  <span className="font-mono text-slate-400">{localConfig.top_p.toFixed(2)}</span>
                </CardTitle>
              </CardHeader>
              <CardContent>
                <Slider
                  value={[localConfig.top_p]}
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
                  <span className="font-mono text-slate-400">{localConfig.top_k}</span>
                </CardTitle>
              </CardHeader>
              <CardContent>
                <Slider
                  value={[localConfig.top_k]}
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
                    <span className="font-mono text-slate-400">{localConfig.mirostat_tau.toFixed(1)}</span>
                  </CardTitle>
                </CardHeader>
                <CardContent>
                  <Slider
                    value={[localConfig.mirostat_tau]}
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
                    <span className="font-mono text-slate-400">{localConfig.mirostat_eta.toFixed(2)}</span>
                  </CardTitle>
                </CardHeader>
                <CardContent>
                  <Slider
                    value={[localConfig.mirostat_eta]}
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
                <CardTitle className="text-sm">IBM Recommended Preset</CardTitle>
              </CardHeader>
              <CardContent>
                <Button 
                  variant="outline" 
                  onClick={() => {
                    setLocalConfig({
                      ...localConfig,
                      sampler_type: 'ChainFull',
                      temperature: 0.7,
                      top_p: 0.95,
                      top_k: 20,
                    });
                  }}
                  className="w-full"
                >
                  Apply IBM Settings (ChainFull, temp: 0.7, top_p: 0.95, top_k: 20)
                </Button>
              </CardContent>
            </Card>
          </div>
        )}
        
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={handleSave} disabled={isLoading}>
            Save Configuration
          </Button>
        </DialogFooter>
      </DialogContent>
      
      {/* File Browser Modal */}
      <FileBrowser
        isOpen={isFileBrowserOpen}
        onClose={() => setIsFileBrowserOpen(false)}
        onSelectFile={(filePath) => {
          if (localConfig) {
            setLocalConfig({
              ...localConfig,
              model_path: filePath
            });
          }
          setIsFileBrowserOpen(false);
        }}
        filter=".gguf"
        title="Select Model File"
      />
    </Dialog>
  );
};