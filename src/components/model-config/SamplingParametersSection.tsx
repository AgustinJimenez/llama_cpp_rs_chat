import React from 'react';
import { Slider } from '@/components/ui/slider';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import type { SamplerConfig, SamplerType } from '@/types';
import { SAMPLER_OPTIONS, SAMPLER_DESCRIPTIONS } from './constants';

export interface SamplingParametersSectionProps {
  config: SamplerConfig;
  onConfigChange: (field: keyof SamplerConfig, value: string | number) => void;
}

export const SamplingParametersSection: React.FC<SamplingParametersSectionProps> = ({
  config,
  onConfigChange
}) => (
  <>
    {/* Sampler Type */}
    <div className="space-y-2">
      <label className="text-sm font-medium">Sampler Type</label>
      <Select
        value={config.sampler_type}
        onValueChange={(value) => onConfigChange('sampler_type', value)}
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
        onValueChange={([value]) => onConfigChange('temperature', value)}
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
        onValueChange={([value]) => onConfigChange('top_p', value)}
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
        onValueChange={([value]) => onConfigChange('top_k', Math.round(value))}
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
          onValueChange={([value]) => onConfigChange('mirostat_tau', value)}
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
          onValueChange={([value]) => onConfigChange('mirostat_eta', value)}
          max={1}
          min={0}
          step={0.01}
          className="w-full"
        />
      </div>
    </div>
  </>
);
