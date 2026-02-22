import React, { useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
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

// Which sampler types use which parameters
const USES_TEMPERATURE: SamplerType[] = ['Temperature', 'TempExt', 'ChainTempTopP', 'ChainTempTopK', 'ChainFull'];
const USES_TOP_P: SamplerType[] = ['Temperature', 'TopP', 'ChainTempTopP', 'ChainFull'];
const USES_TOP_K: SamplerType[] = ['Temperature', 'TopK', 'ChainTempTopK', 'ChainFull'];
const USES_MIN_P: SamplerType[] = ['Temperature', 'MinP', 'ChainFull'];
const USES_TYPICAL_P: SamplerType[] = ['Typical', 'ChainFull'];
const USES_MIROSTAT: SamplerType[] = ['Mirostat'];

type ConfigChange = (field: keyof SamplerConfig, value: string | number) => void;

/** Collapsible advanced penalties section */
const PenaltiesSection: React.FC<{ config: SamplerConfig; onConfigChange: ConfigChange }> = ({ config, onConfigChange }) => {
  const [expanded, setExpanded] = useState(false);
  const isActive = (config.frequency_penalty ?? 0) > 0 || (config.presence_penalty ?? 0) > 0;

  return (
    <div className="border rounded-lg">
      <button type="button" className="flex items-center gap-2 w-full text-left px-3 py-2 text-sm font-medium hover:bg-muted/50 transition-colors rounded-lg" onClick={() => setExpanded(!expanded)}>
        {expanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
        Advanced Penalties
        {isActive && <span className="text-xs text-amber-500 ml-1">active</span>}
      </button>
      {expanded && (
        <div className="px-3 pb-3 space-y-3">
          <SliderParam label="Frequency Penalty" value={config.frequency_penalty ?? 0} format={v => v.toFixed(2)}
            description="Penalize tokens based on how often they appear (0 = disabled)"
            onChange={v => onConfigChange('frequency_penalty', v)} min={0} max={2} step={0.05} />
          <SliderParam label="Presence Penalty" value={config.presence_penalty ?? 0} format={v => v.toFixed(2)}
            description="Penalize tokens that have appeared at all (0 = disabled)"
            onChange={v => onConfigChange('presence_penalty', v)} min={0} max={2} step={0.05} />
          <SliderParam label="Penalty Window" value={config.penalty_last_n ?? 64} format={v => String(v)}
            description="Number of recent tokens to consider for penalties"
            onChange={v => onConfigChange('penalty_last_n', Math.round(v))} min={0} max={256} step={8} />
        </div>
      )}
    </div>
  );
};

/** Collapsible DRY anti-repetition section */
const DrySection: React.FC<{ config: SamplerConfig; onConfigChange: ConfigChange }> = ({ config, onConfigChange }) => {
  const [expanded, setExpanded] = useState(false);
  const isActive = (config.dry_multiplier ?? 0) > 0;

  return (
    <div className="border rounded-lg">
      <button type="button" className="flex items-center gap-2 w-full text-left px-3 py-2 text-sm font-medium hover:bg-muted/50 transition-colors rounded-lg" onClick={() => setExpanded(!expanded)}>
        {expanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
        DRY Anti-Repetition
        {isActive && <span className="text-xs text-amber-500 ml-1">active</span>}
      </button>
      {expanded && (
        <div className="px-3 pb-3 space-y-3">
          <SliderParam label="DRY Multiplier" value={config.dry_multiplier ?? 0} format={v => v.toFixed(1)}
            description="DRY penalty strength (0 = disabled)"
            onChange={v => onConfigChange('dry_multiplier', v)} min={0} max={5} step={0.1} />
          {isActive && (
            <>
              <SliderParam label="DRY Base" value={config.dry_base ?? 1.75} format={v => v.toFixed(2)}
                description="Exponential base for DRY penalty growth"
                onChange={v => onConfigChange('dry_base', v)} min={1} max={4} step={0.05} />
              <SliderParam label="DRY Allowed Length" value={config.dry_allowed_length ?? 2} format={v => String(v)}
                description="Minimum repeat length before penalty applies"
                onChange={v => onConfigChange('dry_allowed_length', Math.round(v))} min={1} max={10} step={1} />
              <SliderParam label="DRY Token Window" value={config.dry_penalty_last_n ?? -1} format={v => v < 0 ? 'Full ctx' : String(v)}
                description="Tokens to scan for repeats (-1 = full context)"
                onChange={v => onConfigChange('dry_penalty_last_n', Math.round(v))} min={-1} max={512} step={16} />
            </>
          )}
        </div>
      )}
    </div>
  );
};

// eslint-disable-next-line complexity
export const SamplingParametersSection: React.FC<SamplingParametersSectionProps> = ({
  config,
  onConfigChange
}) => {
  const sampler = config.sampler_type as SamplerType;
  const showAdvanced = sampler !== 'Mirostat' && sampler !== 'Greedy';

  return (
    <>
      {/* Sampler Type */}
      <div className="space-y-2">
        <span className="text-sm font-medium">Sampler Type</span>
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
          {SAMPLER_DESCRIPTIONS[sampler] || 'Select a sampler type for more information'}
        </p>
      </div>

      {USES_TEMPERATURE.includes(sampler) && (
        <SliderParam label="Temperature" value={config.temperature} format={v => v.toFixed(2)}
          description="Higher values make output more random, lower values more focused"
          onChange={v => onConfigChange('temperature', v)} min={0} max={2} step={0.1} />
      )}

      {USES_TOP_P.includes(sampler) && (
        <SliderParam label="Top P (Nucleus)" value={config.top_p} format={v => v.toFixed(2)}
          description="Only consider tokens that make up the top P probability mass"
          onChange={v => onConfigChange('top_p', v)} min={0} max={1} step={0.05} />
      )}

      {USES_TOP_K.includes(sampler) && (
        <SliderParam label="Top K" value={config.top_k} format={v => String(v)}
          description="Consider only the top K most likely tokens"
          onChange={v => onConfigChange('top_k', Math.round(v))} min={1} max={100} step={1} />
      )}

      {USES_MIN_P.includes(sampler) && (
        <SliderParam label="Min P" value={config.min_p} format={v => v.toFixed(2)}
          description="Filters tokens below min_p * max_probability (0 = disabled)"
          onChange={v => onConfigChange('min_p', v)} min={0} max={0.5} step={0.01} />
      )}

      {USES_TYPICAL_P.includes(sampler) && (
        <SliderParam label="Typical P" value={config.typical_p ?? 1.0} format={v => v.toFixed(2)}
          description="Filters tokens by typical information content (1.0 = disabled)"
          onChange={v => onConfigChange('typical_p', v)} min={0.1} max={1} step={0.05} />
      )}

      {showAdvanced && (
        <SliderParam label="Top-N Sigma" value={config.top_n_sigma ?? -1.0} format={v => v <= 0 ? 'Off' : v.toFixed(1)}
          description="Keep tokens within N standard deviations of mean logit (-1 = disabled)"
          onChange={v => onConfigChange('top_n_sigma', v)} min={-1} max={5} step={0.1} />
      )}

      {sampler !== 'Mirostat' && (
        <SliderParam label="Repeat Penalty" value={config.repeat_penalty} format={v => v.toFixed(2)}
          description="Penalizes repeated tokens (1.0 = disabled, higher = less repetition)"
          onChange={v => onConfigChange('repeat_penalty', v)} min={1} max={2} step={0.05} />
      )}

      {USES_MIROSTAT.includes(sampler) && (
        <div className="grid grid-cols-2 gap-4">
          <SliderParam label="Mirostat Tau" value={config.mirostat_tau} format={v => v.toFixed(1)}
            onChange={v => onConfigChange('mirostat_tau', v)} min={0} max={10} step={0.1} />
          <SliderParam label="Mirostat Eta" value={config.mirostat_eta} format={v => v.toFixed(2)}
            onChange={v => onConfigChange('mirostat_eta', v)} min={0} max={1} step={0.01} />
        </div>
      )}

      {showAdvanced && <PenaltiesSection config={config} onConfigChange={onConfigChange} />}
      {showAdvanced && <DrySection config={config} onConfigChange={onConfigChange} />}
    </>
  );
};
