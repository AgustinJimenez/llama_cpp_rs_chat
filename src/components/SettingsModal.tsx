import React, { useState, useEffect } from 'react';
import { Settings as SettingsIcon } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { useSettings } from '../hooks/useSettings';
import { FileBrowser } from './FileBrowser';
import {
  ModelPathSection,
  SystemPromptSection,
  SamplerTypeSection,
  ParameterSliderSection,
  MirostatSection,
  PresetSection
} from './settings-sections';
import type { SamplerConfig } from '../types';

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
}

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
            <ModelPathSection
              modelPath={localConfig.model_path || ''}
              onModelPathChange={(path) => handleInputChange('model_path', path)}
              onBrowseClick={() => setIsFileBrowserOpen(true)}
            />

            <SystemPromptSection
              systemPrompt={localConfig.system_prompt || ''}
              onSystemPromptChange={(prompt) => handleInputChange('system_prompt', prompt)}
            />

            <SamplerTypeSection
              samplerType={localConfig.sampler_type}
              onSamplerTypeChange={(type) => handleInputChange('sampler_type', type)}
            />

            <ParameterSliderSection
              title="Temperature"
              value={localConfig.temperature}
              displayValue={localConfig.temperature.toFixed(2)}
              onValueChange={(value) => handleInputChange('temperature', value)}
              min={0}
              max={2}
              step={0.1}
              description="Higher values make output more random, lower values more focused"
            />

            <ParameterSliderSection
              title="Top P (Nucleus)"
              value={localConfig.top_p}
              displayValue={localConfig.top_p.toFixed(2)}
              onValueChange={(value) => handleInputChange('top_p', value)}
              min={0}
              max={1}
              step={0.05}
              description="Only consider tokens that make up the top P probability mass"
            />

            <ParameterSliderSection
              title="Top K"
              value={localConfig.top_k}
              displayValue={String(localConfig.top_k)}
              onValueChange={(value) => handleInputChange('top_k', Math.round(value))}
              min={1}
              max={100}
              step={1}
              description="Consider only the top K most likely tokens"
            />

            <MirostatSection
              tauValue={localConfig.mirostat_tau}
              etaValue={localConfig.mirostat_eta}
              onTauChange={(value) => handleInputChange('mirostat_tau', value)}
              onEtaChange={(value) => handleInputChange('mirostat_eta', value)}
            />

            <PresetSection
              onApplyIBMPreset={() => {
                setLocalConfig({
                  ...localConfig,
                  sampler_type: 'ChainFull',
                  temperature: 0.7,
                  top_p: 0.95,
                  top_k: 20,
                });
              }}
            />
          </div>
        )}
        
        <DialogFooter>
          <button className="flat-button bg-muted px-6 py-2" onClick={onClose}>
            Cancel
          </button>
          <button className="flat-button bg-flat-red text-white px-6 py-2 disabled:opacity-50" onClick={handleSave} disabled={isLoading}>
            Save Configuration
          </button>
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