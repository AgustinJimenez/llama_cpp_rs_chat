import React from 'react';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../atoms/select';
import type { SamplerConfig, SamplerType } from '@/types';
import { SAMPLER_OPTIONS } from './constants';
import { ParamGroup } from './ParamGroup';

export interface SamplingParametersSectionProps {
  config: SamplerConfig;
  onConfigChange: (field: keyof SamplerConfig, value: string | number) => void;
}

interface NumericParamProps {
  label: string;
  value: number;
  format?: (v: number) => string;
  onChange: (value: number) => void;
  min: number;
  max: number;
  step: number;
  integer?: boolean;
}

const NumericParam: React.FC<NumericParamProps> = ({ label, value, format, onChange, min, max, step, integer }) => (
  <div className="flex items-center gap-1.5">
    <label className="text-xs text-muted-foreground whitespace-nowrap">{label}</label>
    <input
      type="number"
      value={format ? format(value) : value}
      onChange={e => {
        const v = parseFloat(e.target.value);
        if (!isNaN(v)) onChange(integer ? Math.round(Math.min(max, Math.max(min, v))) : Math.min(max, Math.max(min, v)));
      }}
      min={min}
      max={max}
      step={step}
      className="w-16 h-6 px-1.5 text-xs font-mono text-right rounded border border-input bg-background focus:outline-none focus:ring-1 focus:ring-ring"
    />
  </div>
);

// Which sampler types use which parameters
const USES_TEMPERATURE: SamplerType[] = ['Temperature', 'TempExt', 'ChainTempTopP', 'ChainTempTopK', 'ChainFull'];
const USES_TOP_P: SamplerType[] = ['Temperature', 'TopP', 'ChainTempTopP', 'ChainFull'];
const USES_TOP_K: SamplerType[] = ['Temperature', 'TopK', 'ChainTempTopK', 'ChainFull'];
const USES_MIN_P: SamplerType[] = ['Temperature', 'MinP', 'ChainFull'];
const USES_TYPICAL_P: SamplerType[] = ['Typical', 'ChainFull'];
const USES_MIROSTAT: SamplerType[] = ['Mirostat'];

// eslint-disable-next-line complexity
export const SamplingParametersSection: React.FC<SamplingParametersSectionProps> = ({
  config,
  onConfigChange
}) => {
  const sampler = config.sampler_type as SamplerType;
  const showAdvanced = sampler !== 'Mirostat' && sampler !== 'Greedy';
  const dryActive = (config.dry_multiplier ?? 0) > 0;

  return (
    <div className="flex flex-wrap gap-3">
      {/* Sampling */}
      <ParamGroup title="Sampling">
        <div className="flex items-center gap-1.5">
          <label className="text-xs text-muted-foreground whitespace-nowrap">Sampler</label>
          <Select
            value={config.sampler_type}
            onValueChange={(value) => onConfigChange('sampler_type', value)}
          >
            <SelectTrigger className="w-36 h-6 text-xs">
              <SelectValue placeholder="Select type" />
            </SelectTrigger>
            <SelectContent>
              {SAMPLER_OPTIONS.map(option => (
                <SelectItem key={option} value={option}>
                  {option}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        {USES_TEMPERATURE.includes(sampler) && (
          <NumericParam label="Temp" value={config.temperature} format={v => v.toFixed(2)}
            onChange={v => onConfigChange('temperature', v)} min={0} max={2} step={0.1} />
        )}
        {USES_TOP_P.includes(sampler) && (
          <NumericParam label="Top P" value={config.top_p} format={v => v.toFixed(2)}
            onChange={v => onConfigChange('top_p', v)} min={0} max={1} step={0.05} />
        )}
        {USES_TOP_K.includes(sampler) && (
          <NumericParam label="Top K" value={config.top_k}
            onChange={v => onConfigChange('top_k', v)} min={0} max={100} step={1} integer />
        )}
        {USES_MIN_P.includes(sampler) && (
          <NumericParam label="Min P" value={config.min_p} format={v => v.toFixed(2)}
            onChange={v => onConfigChange('min_p', v)} min={0} max={0.5} step={0.01} />
        )}
        {USES_TYPICAL_P.includes(sampler) && (
          <NumericParam label="Typical P" value={config.typical_p ?? 1.0} format={v => v.toFixed(2)}
            onChange={v => onConfigChange('typical_p', v)} min={0.1} max={1} step={0.05} />
        )}
        {showAdvanced && (
          <NumericParam label="Top-N σ" value={config.top_n_sigma ?? -1.0} format={v => v <= 0 ? '-1' : v.toFixed(1)}
            onChange={v => onConfigChange('top_n_sigma', v)} min={-1} max={5} step={0.1} />
        )}
        {sampler !== 'Mirostat' && (
          <NumericParam label="Repeat" value={config.repeat_penalty} format={v => v.toFixed(2)}
            onChange={v => onConfigChange('repeat_penalty', v)} min={1} max={2} step={0.05} />
        )}
        {USES_MIROSTAT.includes(sampler) && (
          <>
            <NumericParam label="Tau" value={config.mirostat_tau} format={v => v.toFixed(1)}
              onChange={v => onConfigChange('mirostat_tau', v)} min={0} max={10} step={0.1} />
            <NumericParam label="Eta" value={config.mirostat_eta} format={v => v.toFixed(2)}
              onChange={v => onConfigChange('mirostat_eta', v)} min={0} max={1} step={0.01} />
          </>
        )}
        <NumericParam label="Seed" value={config.seed ?? -1} format={v => v < 0 ? '-1' : String(v)}
          onChange={v => onConfigChange('seed', v)} min={-1} max={2147483647} step={1} integer />
      </ParamGroup>

      {/* Penalties */}
      {showAdvanced && (
        <ParamGroup title="Penalties">
          <NumericParam label="Freq" value={config.frequency_penalty ?? 0} format={v => v.toFixed(2)}
            onChange={v => onConfigChange('frequency_penalty', v)} min={0} max={2} step={0.05} />
          <NumericParam label="Presence" value={config.presence_penalty ?? 0} format={v => v.toFixed(2)}
            onChange={v => onConfigChange('presence_penalty', v)} min={0} max={2} step={0.05} />
          <NumericParam label="Window" value={config.penalty_last_n ?? 64}
            onChange={v => onConfigChange('penalty_last_n', v)} min={0} max={256} step={8} integer />
        </ParamGroup>
      )}

      {/* DRY Anti-Repetition */}
      {showAdvanced && (
        <ParamGroup title="DRY Anti-Repetition">
          <NumericParam label="Multiplier" value={config.dry_multiplier ?? 0} format={v => v.toFixed(1)}
            onChange={v => onConfigChange('dry_multiplier', v)} min={0} max={5} step={0.1} />
          <div className={dryActive ? '' : 'opacity-40 pointer-events-none'}>
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
              <NumericParam label="Base" value={config.dry_base ?? 1.75} format={v => v.toFixed(2)}
                onChange={v => onConfigChange('dry_base', v)} min={1} max={4} step={0.05} />
              <NumericParam label="Min Len" value={config.dry_allowed_length ?? 2}
                onChange={v => onConfigChange('dry_allowed_length', v)} min={1} max={10} step={1} integer />
              <NumericParam label="Window" value={config.dry_penalty_last_n ?? -1} format={v => v < 0 ? '-1' : String(v)}
                onChange={v => onConfigChange('dry_penalty_last_n', v)} min={-1} max={512} step={16} integer />
            </div>
          </div>
        </ParamGroup>
      )}
    </div>
  );
};
