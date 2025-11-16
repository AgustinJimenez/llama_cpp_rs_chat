import React from 'react';
import { Button } from '../../atoms/button';
import type { SamplerConfig } from '@/types';

export interface PresetsSectionProps {
  onApplyPreset: (preset: Partial<SamplerConfig>) => void;
}

export const PresetsSection: React.FC<PresetsSectionProps> = ({ onApplyPreset }) => (
  <div className="space-y-2">
    <label className="text-sm font-medium">Quick Presets</label>
    <div className="grid grid-cols-2 gap-2">
      <Button
        variant="outline"
        onClick={() => onApplyPreset({
          sampler_type: 'ChainFull',
          temperature: 0.7,
          top_p: 0.95,
          top_k: 20,
          gpu_layers: 32,
        })}
        className="text-xs"
      >
        IBM Recommended
      </Button>
      <Button
        variant="outline"
        onClick={() => onApplyPreset({
          sampler_type: 'Greedy',
          temperature: 0.1,
          top_p: 0.1,
          top_k: 1,
        })}
        className="text-xs"
      >
        Conservative
      </Button>
      <Button
        variant="outline"
        onClick={() => onApplyPreset({
          sampler_type: 'Temperature',
          temperature: 1.2,
          top_p: 0.8,
          top_k: 50,
        })}
        className="text-xs"
      >
        Creative
      </Button>
      <Button
        variant="outline"
        onClick={() => onApplyPreset({
          sampler_type: 'Temperature',
          temperature: 0.7,
          top_p: 0.95,
          top_k: 20,
        })}
        className="text-xs"
      >
        Balanced
      </Button>
    </div>
  </div>
);
