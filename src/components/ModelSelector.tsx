import React, { useState } from 'react';
import { Brain, FolderOpen } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ModelConfigModal } from './ModelConfigModal';
import type { SamplerConfig } from '../types';

interface ModelSelectorProps {
  onModelLoad: (modelPath: string, config: SamplerConfig) => void;
  currentModelPath?: string;
  isLoading?: boolean;
  error?: string | null;
}

export const ModelSelector: React.FC<ModelSelectorProps> = ({ 
  onModelLoad, 
  currentModelPath, 
  isLoading = false,
  error = null
}) => {
  const [isConfigModalOpen, setIsConfigModalOpen] = useState(false);

  const handleConfigSave = (config: SamplerConfig) => {
    if (config.model_path) {
      onModelLoad(config.model_path, config);
    }
    setIsConfigModalOpen(false);
  };

  const handleConfigCancel = () => {
    setIsConfigModalOpen(false);
  };

  const handleButtonClick = () => {
    setIsConfigModalOpen(true);
  };

  const getDisplayText = () => {
    if (isLoading) return "Loading model...";
    if (currentModelPath) {
      const fileName = currentModelPath.split('/').pop() || currentModelPath;
      return `Model: ${fileName}`;
    }
    return "Select a model to load";
  };

  const getIcon = () => {
    if (currentModelPath) {
      return <Brain className="h-4 w-4" />;
    }
    return <FolderOpen className="h-4 w-4" />;
  };

  return (
    <div className="relative">
      <Button
        onClick={handleButtonClick}
        disabled={isLoading}
        variant="outline"
        className="flex items-center gap-2 text-sm"
      >
        {getIcon()}
        {getDisplayText()}
      </Button>

      {/* Error Display */}
      {error && (
        <div className="mt-3 p-3 bg-red-50 border border-red-200 rounded-md max-w-md">
          <p className="text-sm text-red-700">
            <strong>Error:</strong> {error}
          </p>
        </div>
      )}

      {/* Model Configuration Modal */}
      <ModelConfigModal
        isOpen={isConfigModalOpen}
        onClose={handleConfigCancel}
        onSave={handleConfigSave}
        isLoading={isLoading}
        initialModelPath={currentModelPath}
      />
    </div>
  );
};