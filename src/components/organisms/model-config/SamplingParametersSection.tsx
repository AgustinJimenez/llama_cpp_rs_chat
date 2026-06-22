import React from 'react';
import { useTranslation } from 'react-i18next';

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../../atoms/select';

const DEFAULT_PENALTY_WINDOW = 64;
const DEFAULT_DRY_BASE = 1.75;

import { SAMPLER_OPTIONS } from './constants';
import { ParamGroup } from './ParamGroup';

import type { SamplerConfig, SamplerType } from '@/types';

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

const NumericParam: React.FC<NumericParamProps> = ({
  label,
  value,
  format,
  onChange,
  min,
  max,
  step,
  integer,
}) => {
  const displayValue = format ? format(value) : value;
  return (
    <div className="flex items-center gap-1.5">
      <label className="whitespace-nowrap text-xs text-muted-foreground">{label}</label>
      <input
        type="number"
        value={displayValue}
        onChange={(e) => {
          const v = parseFloat(e.target.value);
          if (!isNaN(v)) {
            onChange(
              integer
                ? Math.round(Math.min(max, Math.max(min, v)))
                : Math.min(max, Math.max(min, v)),
            );
          }
        }}
        min={min}
        max={max}
        step={step}
        className="h-6 w-16 rounded border border-input bg-background px-1.5 text-right font-mono text-xs focus:outline-none focus:ring-1 focus:ring-ring"
      />
    </div>
  );
};

// Which sampler types use which parameters
const USES_TEMPERATURE: SamplerType[] = [
  'Temperature',
  'TempExt',
  'ChainTempTopP',
  'ChainTempTopK',
  'ChainFull',
];
const USES_TOP_P: SamplerType[] = ['Temperature', 'TopP', 'ChainTempTopP', 'ChainFull'];
const USES_TOP_K: SamplerType[] = ['Temperature', 'TopK', 'ChainTempTopK', 'ChainFull'];
const USES_MIN_P: SamplerType[] = ['Temperature', 'MinP', 'ChainFull'];
const USES_TYPICAL_P: SamplerType[] = ['Typical', 'ChainFull'];
const USES_MIROSTAT: SamplerType[] = ['Mirostat'];

type ConfigChanger = (field: keyof SamplerConfig, value: string | number) => void;

const SamplerGroup = ({
  config,
  onConfigChange,
}: {
  config: SamplerConfig;
  onConfigChange: ConfigChanger;
}) => {
  const { t } = useTranslation();
  const sampler = config.sampler_type as SamplerType;
  const showAdvanced = sampler !== 'Mirostat' && sampler !== 'Greedy';
  return (
    <ParamGroup title={t('modelConfig.samplingGroup')}>
      <div className="flex items-center gap-1.5">
        <label
          htmlFor="sampler-type-select"
          className="whitespace-nowrap text-xs text-muted-foreground"
        >
          {t('modelConfig.samplerLabel')}
        </label>
        <Select
          value={config.sampler_type}
          onValueChange={(value) => onConfigChange('sampler_type', value)}
        >
          <SelectTrigger id="sampler-type-select" className="h-6 w-36 text-xs">
            <SelectValue placeholder={t('modelConfig.selectType')} />
          </SelectTrigger>
          <SelectContent>
            {SAMPLER_OPTIONS.map((option) => (
              <SelectItem key={option} value={option}>
                {option}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      {USES_TEMPERATURE.includes(sampler) && (
        <NumericParam
          label={t('modelConfig.temperature')}
          value={config.temperature}
          format={(v) => v.toFixed(2)}
          onChange={(v) => onConfigChange('temperature', v)}
          min={0}
          max={2}
          step={0.1}
        />
      )}
      {USES_TOP_P.includes(sampler) && (
        <NumericParam
          label={t('modelConfig.topP')}
          value={config.top_p}
          format={(v) => v.toFixed(2)}
          onChange={(v) => onConfigChange('top_p', v)}
          min={0}
          max={1}
          step={0.05}
        />
      )}
      {USES_TOP_K.includes(sampler) && (
        <NumericParam
          label={t('modelConfig.topK')}
          value={config.top_k}
          onChange={(v) => onConfigChange('top_k', v)}
          min={0}
          max={100}
          step={1}
          integer
        />
      )}
      {USES_MIN_P.includes(sampler) && (
        <NumericParam
          label={t('modelConfig.minP')}
          value={config.min_p}
          format={(v) => v.toFixed(2)}
          onChange={(v) => onConfigChange('min_p', v)}
          min={0}
          max={0.5}
          step={0.01}
        />
      )}
      {USES_TYPICAL_P.includes(sampler) && (
        <NumericParam
          label={t('modelConfig.typicalP')}
          value={config.typical_p ?? 1.0}
          format={(v) => v.toFixed(2)}
          onChange={(v) => onConfigChange('typical_p', v)}
          min={0.1}
          max={1}
          step={0.05}
        />
      )}
      {!!showAdvanced && (
        <NumericParam
          label={t('modelConfig.topNSigma')}
          value={config.top_n_sigma ?? -1.0}
          format={(v) => (v <= 0 ? '-1' : v.toFixed(1))}
          onChange={(v) => onConfigChange('top_n_sigma', v)}
          min={-1}
          max={5}
          step={0.1}
        />
      )}
      {sampler !== 'Mirostat' && (
        <NumericParam
          label={t('modelConfig.repeatPenalty')}
          value={config.repeat_penalty}
          format={(v) => v.toFixed(2)}
          onChange={(v) => onConfigChange('repeat_penalty', v)}
          min={1}
          max={2}
          step={0.05}
        />
      )}
      {USES_MIROSTAT.includes(sampler) && (
        <>
          <NumericParam
            label={t('modelConfig.mirostatTau')}
            value={config.mirostat_tau}
            format={(v) => v.toFixed(1)}
            onChange={(v) => onConfigChange('mirostat_tau', v)}
            min={0}
            max={10}
            step={0.1}
          />
          <NumericParam
            label={t('modelConfig.mirostatEta')}
            value={config.mirostat_eta}
            format={(v) => v.toFixed(2)}
            onChange={(v) => onConfigChange('mirostat_eta', v)}
            min={0}
            max={1}
            step={0.01}
          />
        </>
      )}
      <NumericParam
        label={t('modelConfig.seed')}
        value={config.seed ?? -1}
        format={(v) => (v < 0 ? '-1' : String(v))}
        onChange={(v) => onConfigChange('seed', v)}
        min={-1}
        max={2147483647}
        step={1}
        integer
      />
    </ParamGroup>
  );
};

const PenaltyGroup = ({
  config,
  onConfigChange,
}: {
  config: SamplerConfig;
  onConfigChange: ConfigChanger;
}) => {
  const { t } = useTranslation();
  return (
    <ParamGroup title={t('modelConfig.penaltiesGroup')}>
      <NumericParam
        label={t('modelConfig.frequencyPenalty')}
        value={config.frequency_penalty ?? 0}
        format={(v) => v.toFixed(2)}
        onChange={(v) => onConfigChange('frequency_penalty', v)}
        min={0}
        max={2}
        step={0.05}
      />
      <NumericParam
        label={t('modelConfig.presencePenalty')}
        value={config.presence_penalty ?? 0}
        format={(v) => v.toFixed(2)}
        onChange={(v) => onConfigChange('presence_penalty', v)}
        min={0}
        max={2}
        step={0.05}
      />
      <NumericParam
        label={t('modelConfig.penaltyWindow')}
        value={config.penalty_last_n ?? DEFAULT_PENALTY_WINDOW}
        onChange={(v) => onConfigChange('penalty_last_n', v)}
        min={0}
        max={256}
        step={8}
        integer
      />
    </ParamGroup>
  );
};

const DryGroup = ({
  config,
  onConfigChange,
}: {
  config: SamplerConfig;
  onConfigChange: ConfigChanger;
}) => {
  const { t } = useTranslation();
  const dryActive = (config.dry_multiplier ?? 0) > 0;
  const dryGroupClass = dryActive ? '' : 'opacity-40 pointer-events-none';
  return (
    <ParamGroup title={t('modelConfig.dryGroup')}>
      <NumericParam
        label={t('modelConfig.dryMultiplier')}
        value={config.dry_multiplier ?? 0}
        format={(v) => v.toFixed(1)}
        onChange={(v) => onConfigChange('dry_multiplier', v)}
        min={0}
        max={5}
        step={0.1}
      />
      <div className={dryGroupClass}>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
          <NumericParam
            label={t('modelConfig.dryBase')}
            value={config.dry_base ?? DEFAULT_DRY_BASE}
            format={(v) => v.toFixed(2)}
            onChange={(v) => onConfigChange('dry_base', v)}
            min={1}
            max={4}
            step={0.05}
          />
          <NumericParam
            label={t('modelConfig.dryMinLen')}
            value={config.dry_allowed_length ?? 2}
            onChange={(v) => onConfigChange('dry_allowed_length', v)}
            min={1}
            max={10}
            step={1}
            integer
          />
          <NumericParam
            label={t('modelConfig.dryWindow')}
            value={config.dry_penalty_last_n ?? -1}
            format={(v) => (v < 0 ? '-1' : String(v))}
            onChange={(v) => onConfigChange('dry_penalty_last_n', v)}
            min={-1}
            max={512}
            step={16}
            integer
          />
        </div>
      </div>
    </ParamGroup>
  );
};

export const SamplingParametersSection: React.FC<SamplingParametersSectionProps> = ({
  config,
  onConfigChange,
}) => {
  const sampler = config.sampler_type as SamplerType;
  const showAdvanced = sampler !== 'Mirostat' && sampler !== 'Greedy';

  return (
    <div className="flex flex-wrap gap-3">
      <SamplerGroup config={config} onConfigChange={onConfigChange} />
      {!!showAdvanced && <PenaltyGroup config={config} onConfigChange={onConfigChange} />}
      {!!showAdvanced && <DryGroup config={config} onConfigChange={onConfigChange} />}
    </div>
  );
};
