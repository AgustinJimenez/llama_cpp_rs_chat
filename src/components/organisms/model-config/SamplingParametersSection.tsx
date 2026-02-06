import React from 'react';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../atoms/select';
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
      <label className="text-sm font-medium">Temperature</label>
      <input
        type="number"
        value={config.temperature}
        onChange={(e) => onConfigChange('temperature', parseFloat(e.target.value) || 0)}
        min={0}
        max={2}
        step={0.1}
        className="w-full px-3 py-2 bg-background border border-input rounded-md text-sm"
      />
      <p className="text-xs text-muted-foreground">
        Higher values make output more random, lower values more focused
      </p>
    </div>

    {/* Top P */}
    <div className="space-y-2">
      <label className="text-sm font-medium">Top P (Nucleus)</label>
      <input
        type="number"
        value={config.top_p}
        onChange={(e) => onConfigChange('top_p', parseFloat(e.target.value) || 0)}
        min={0}
        max={1}
        step={0.05}
        className="w-full px-3 py-2 bg-background border border-input rounded-md text-sm"
      />
      <p className="text-xs text-muted-foreground">
        Only consider tokens that make up the top P probability mass
      </p>
    </div>

    {/* Top K */}
    <div className="space-y-2">
      <label className="text-sm font-medium">Top K</label>
      <input
        type="number"
        value={config.top_k}
        onChange={(e) => onConfigChange('top_k', parseInt(e.target.value) || 1)}
        min={1}
        max={100}
        step={1}
        className="w-full px-3 py-2 bg-background border border-input rounded-md text-sm"
      />
      <p className="text-xs text-muted-foreground">
        Consider only the top K most likely tokens
      </p>
    </div>

    {/* Min P */}
    <div className="space-y-2">
      <label className="text-sm font-medium">Min P</label>
      <input
        type="number"
        value={config.min_p ?? ''}
        onChange={(e) => {
          const value = e.target.value === '' ? undefined : parseFloat(e.target.value);
          onConfigChange('min_p', value);
        }}
        placeholder="0.01 (recommended for deepseek2)"
        min={0}
        max={1}
        step={0.001}
        className="w-full px-3 py-2 bg-background border border-input rounded-md text-sm"
      />
      <p className="text-xs text-muted-foreground">
        Minimum probability threshold (critical for deepseek2 models)
      </p>
    </div>

    {/* Mirostat Parameters */}
    <div className="grid grid-cols-2 gap-4">
      <div className="space-y-2">
        <label className="text-sm font-medium">Mirostat Tau</label>
        <input
          type="number"
          value={config.mirostat_tau}
          onChange={(e) => onConfigChange('mirostat_tau', parseFloat(e.target.value) || 0)}
          min={0}
          max={10}
          step={0.1}
          className="w-full px-3 py-2 bg-background border border-input rounded-md text-sm"
        />
      </div>

      <div className="space-y-2">
        <label className="text-sm font-medium">Mirostat Eta</label>
        <input
          type="number"
          value={config.mirostat_eta}
          onChange={(e) => onConfigChange('mirostat_eta', parseFloat(e.target.value) || 0)}
          min={0}
          max={1}
          step={0.01}
          className="w-full px-3 py-2 bg-background border border-input rounded-md text-sm"
        />
      </div>
    </div>
  </>
);
