import React, { useState, useEffect } from 'react';
import { Settings as SettingsIcon } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogDescription,
  DialogTitle,
} from '../atoms/dialog';
import { Card, CardContent } from '../atoms/card';
import { useSettings } from '../../hooks/useSettings';
import { FileBrowser } from './FileBrowser';
import { toast } from 'react-hot-toast';
import {
  ModelPathSection,
  SystemPromptSection,
  SamplerTypeSection,
  ParameterSliderSection,
  MirostatSection,
  PresetSection
} from '../molecules/settings-sections';
import type { SamplerConfig } from '../../types';

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
}

// eslint-disable-next-line max-lines-per-function
export const SettingsModal: React.FC<SettingsModalProps> = ({ isOpen, onClose }) => {
  const { config, isLoading, error, updateConfig } = useSettings();
  const [localConfig, setLocalConfig] = useState<SamplerConfig | null>(null);
  const [isFileBrowserOpen, setIsFileBrowserOpen] = useState(false);

  useEffect(() => {
    if (config) {
      setLocalConfig(config);
    }
  }, [config]);

  const validateConfig = (cfg: SamplerConfig): string | null => {
    if (cfg.temperature < 0 || cfg.temperature > 2) return 'Temperature must be between 0.0 and 2.0';
    if (cfg.top_p < 0 || cfg.top_p > 1) return 'Top P must be between 0.0 and 1.0';
    if (cfg.top_k < 0) return 'Top K must be non-negative';
    if ((cfg.context_size ?? 0) <= 0) return 'Context size must be positive';
    return null;
  };

  const handleSave = async () => {
    if (localConfig) {
      const validationError = validateConfig(localConfig);
      if (validationError) {
        toast.error(validationError, { duration: 4000 });
        return;
      }
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
          <DialogDescription className="sr-only">
            Application configuration settings
          </DialogDescription>
        </DialogHeader>
        
        {isLoading ? <div className="flex items-center justify-center py-8">
            <div className="text-sm text-muted-foreground">Loading configuration...</div>
          </div> : null}
        
        {error ? <Card className="border-destructive">
            <CardContent className="p-4">
              <div className="text-destructive text-sm">Error: {error}</div>
            </CardContent>
          </Card> : null}
        
        {localConfig ? <div className="space-y-6">
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
          </div> : null}
        
        <DialogFooter>
          <button className="flat-button bg-muted px-6 py-2" onClick={onClose}>
            Cancel
          </button>
          <button className="flat-button bg-primary text-white px-6 py-2 disabled:opacity-50" onClick={handleSave} disabled={isLoading}>
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
