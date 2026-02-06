import React from 'react';
import { Button } from '../../atoms/button';
import type { SamplerConfig } from '@/types';
import { DEFAULT_PRESET, findPresetByName, type ModelPreset } from '@/config/modelPresets';

export interface PresetsSectionProps {
  generalName?: string; // From GGUF general.name
  recommendedParams?: Partial<SamplerConfig>; // From GGUF general.sampling.*
  onApplyPreset: (preset: Partial<SamplerConfig>) => void;
}

export const PresetsSection: React.FC<PresetsSectionProps> = ({
  generalName,
  recommendedParams,
  onApplyPreset
}) => {
  // Determine the best preset to show
  const getModelPreset = (): ModelPreset | null => {
    // First, use GGUF embedded params if available
    if (recommendedParams && Object.keys(recommendedParams).length > 0) {
      return recommendedParams;
    }
    // Then try to find a preset by model name
    if (generalName) {
      return findPresetByName(generalName);
    }
    return null;
  };

  const modelPreset = getModelPreset();
  const hasModelPreset = modelPreset !== null;

  return (
    <div className="space-y-2">
      <label className="text-sm font-medium">Quick Presets</label>
      <div className="grid grid-cols-2 gap-2">
        {/* Model-specific preset (if available) */}
        {hasModelPreset && (
          <Button
            variant="default"
            onClick={() => onApplyPreset({
              ...DEFAULT_PRESET,
              ...modelPreset,
            })}
            className="text-xs col-span-2 bg-orange-600 hover:bg-orange-700"
          >
            Apply Recommended for Model
          </Button>
        )}

        {/* Fallback default */}
        <Button
          variant="outline"
          onClick={() => onApplyPreset(DEFAULT_PRESET)}
          className="text-xs"
        >
          Default (Balanced)
        </Button>

        {/* Greedy for deterministic output */}
        <Button
          variant="outline"
          onClick={() => onApplyPreset({
            sampler_type: 'Greedy',
            temperature: 0.0,
            top_p: 1.0,
            top_k: 1,
          })}
          className="text-xs"
        >
          Greedy (Deterministic)
        </Button>
      </div>

      {generalName && (
        <p className="text-xs text-muted-foreground mt-2">
          Model: {generalName}
          {hasModelPreset && " (preset available)"}
        </p>
      )}
    </div>
  );
};
