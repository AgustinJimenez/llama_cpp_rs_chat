import React from 'react';
import { Slider } from '../../atoms/slider';
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

interface SliderParamProps {
  label: string;
  value: number;
  format?: (v: number) => string;
  description?: string;
  onChange: (value: number) => void;
  min: number;
  max: number;
  step: number;
}

const SliderParam: React.FC<SliderParamProps> = ({ label, value, format, description, onChange, min, max, step }) => (
  <div className="space-y-2">
    <div className="flex justify-between items-center">
      <label className="text-sm font-medium">{label}</label>
      <span className="text-sm font-mono text-muted-foreground">{format ? format(value) : value}</span>
    </div>
    <Slider
      value={[value]}
      onValueChange={([v]) => onChange(v)}
      max={max}
      min={min}
      step={step}
      className="w-full"
    />
    {description && <p className="text-xs text-muted-foreground">{description}</p>}
  </div>
);

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

    <SliderParam label="Temperature" value={config.temperature} format={v => v.toFixed(2)}
      description="Higher values make output more random, lower values more focused"
      onChange={v => onConfigChange('temperature', v)} min={0} max={2} step={0.1} />

    <SliderParam label="Top P (Nucleus)" value={config.top_p} format={v => v.toFixed(2)}
      description="Only consider tokens that make up the top P probability mass"
      onChange={v => onConfigChange('top_p', v)} min={0} max={1} step={0.05} />

    <SliderParam label="Top K" value={config.top_k} format={v => String(v)}
      description="Consider only the top K most likely tokens"
      onChange={v => onConfigChange('top_k', Math.round(v))} min={1} max={100} step={1} />

    <SliderParam label="Min P" value={config.min_p} format={v => v.toFixed(2)}
      description="Filters tokens below min_p * max_probability (0 = disabled)"
      onChange={v => onConfigChange('min_p', v)} min={0} max={0.5} step={0.01} />

    <SliderParam label="Repeat Penalty" value={config.repeat_penalty} format={v => v.toFixed(2)}
      description="Penalizes repeated tokens (1.0 = disabled, higher = less repetition)"
      onChange={v => onConfigChange('repeat_penalty', v)} min={1} max={2} step={0.05} />

    {/* Mirostat Parameters */}
    <div className="grid grid-cols-2 gap-4">
      <SliderParam label="Mirostat Tau" value={config.mirostat_tau} format={v => v.toFixed(1)}
        onChange={v => onConfigChange('mirostat_tau', v)} min={0} max={10} step={0.1} />
      <SliderParam label="Mirostat Eta" value={config.mirostat_eta} format={v => v.toFixed(2)}
        onChange={v => onConfigChange('mirostat_eta', v)} min={0} max={1} step={0.01} />
    </div>
  </>
);
