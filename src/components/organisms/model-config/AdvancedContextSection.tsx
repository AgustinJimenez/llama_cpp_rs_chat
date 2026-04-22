import React from 'react';

import { ParamGroup } from './ParamGroup';

import type { SamplerConfig } from '@/types';

const KV_CACHE_OPTIONS = [
  { value: 'f16', label: 'F16' },
  { value: 'q8_0', label: 'Q8_0' },
  { value: 'q4_0', label: 'Q4_0' },
  { value: 'turbo4', label: 'TQ4 — TurboQuant 4-bit (3.8x)' },
  { value: 'turbo3', label: 'TQ3 — TurboQuant 3-bit (4.9x)' },
  { value: 'turbo2', label: 'TQ2 — TurboQuant 2-bit (6.4x)' },
];

const BATCH_SIZE_512 = 512;
const BATCH_SIZE_2048 = 2048;
const BATCH_SIZE_4096 = 4096;
const BATCH_PRESETS = [BATCH_SIZE_512, 1024, BATCH_SIZE_2048, BATCH_SIZE_4096];

const SPLIT_MODE_OPTIONS = [
  { value: 'layer', label: 'Layer' },
  { value: 'row', label: 'Row' },
  { value: 'none', label: 'None' },
];

interface AdvancedContextSectionProps {
  config: SamplerConfig;
  onConfigChange: (field: keyof SamplerConfig, value: string | number | boolean) => void;
}

const Toggle: React.FC<{
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}> = ({ label, checked, onChange }) => (
  <div className="flex items-center gap-1.5">
    <label className="text-xs text-muted-foreground whitespace-nowrap">{label}</label>
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
        checked ? 'bg-primary' : 'bg-muted'
      }`}
      onClick={() => onChange(!checked)}
    >
      <span
        className={`inline-block h-3.5 w-3.5 transform rounded-full bg-background transition-transform ${
          checked ? 'translate-x-4' : 'translate-x-0.5'
        }`}
      />
    </button>
  </div>
);

const NumInput: React.FC<{
  label: string;
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step: number;
  integer?: boolean;
  width?: string;
}> = ({ label, value, onChange, min, max, step, integer, width = 'w-16' }) => (
  <div className="flex items-center gap-1.5">
    <label className="text-xs text-muted-foreground whitespace-nowrap">{label}</label>
    <input
      type="number"
      value={value}
      onChange={(e) => {
        const v = parseFloat(e.target.value);
        if (!isNaN(v)) {
          onChange(
            integer ? Math.round(Math.min(max, Math.max(min, v))) : Math.min(max, Math.max(min, v)),
          );
        }
      }}
      min={min}
      max={max}
      step={step}
      className={`${width} h-6 px-1.5 text-xs font-mono text-right rounded border border-input bg-background focus:outline-none focus:ring-1 focus:ring-ring`}
    />
  </div>
);

const KvCacheGroup = ({
  config,
  onConfigChange,
}: {
  config: SamplerConfig;
  onConfigChange: (field: keyof SamplerConfig, value: string | number | boolean) => void;
}) => (
  <ParamGroup
    title={
      <span className="flex items-center gap-1.5">
        KV Cache{' '}
        <span className="text-[9px] font-medium px-1.5 py-0.5 rounded-full bg-primary/20 text-primary">
          TurboQuant
        </span>
        <span
          className="inline-flex items-center justify-center w-3.5 h-3.5 rounded-full border border-muted-foreground/40 text-[9px] text-muted-foreground cursor-help"
          title={
            'TurboQuant uses asymmetric K/V types for best quality-per-bit.\n\n' +
            'Recommended configs (memory savings vs F16):\n' +
            '  K=TQ2, V=TQ3 — best balance (5.5x savings, minimal quality loss)\n' +
            '  K=Q8_0, V=TQ3 — safer (3.5x savings, near-lossless)\n' +
            '  K=TQ3, V=TQ3 — aggressive (4.9x savings)\n\n' +
            'K cache (keys) tolerates lower precision than V cache (values).\n' +
            'Using different types for K and V is intentional, not a mistake.'
          }
        >
          ?
        </span>
      </span>
    }
  >
    <div className="flex items-center gap-1.5">
      <label htmlFor="cache-type-k" className="text-xs text-muted-foreground whitespace-nowrap">
        K Type
      </label>
      <select
        id="cache-type-k"
        className="h-6 rounded border border-input bg-background px-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-ring"
        value={config.cache_type_k ?? 'f16'}
        onChange={(e) => onConfigChange('cache_type_k', e.target.value)}
      >
        {KV_CACHE_OPTIONS.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
    </div>
    <div className="flex items-center gap-1.5">
      <label htmlFor="cache-type-v" className="text-xs text-muted-foreground whitespace-nowrap">
        V Type
      </label>
      <select
        id="cache-type-v"
        className="h-6 rounded border border-input bg-background px-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-ring"
        value={config.cache_type_v ?? 'f16'}
        onChange={(e) => onConfigChange('cache_type_v', e.target.value)}
      >
        {KV_CACHE_OPTIONS.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
    </div>
    <Toggle
      label="Flash Attn"
      checked={config.flash_attention ?? true}
      onChange={(v) => onConfigChange('flash_attention', v)}
    />
  </ParamGroup>
);

const HardwareGroup = ({
  config,
  onConfigChange,
}: {
  config: SamplerConfig;
  onConfigChange: (field: keyof SamplerConfig, value: string | number | boolean) => void;
}) => (
  <ParamGroup title="Hardware">
    <NumInput
      label="Threads"
      value={config.n_threads ?? 0}
      onChange={(v) => onConfigChange('n_threads', v)}
      min={0}
      max={128}
      step={1}
      integer
    />
    <NumInput
      label="Batch Thr"
      value={config.n_threads_batch ?? 0}
      onChange={(v) => onConfigChange('n_threads_batch', v)}
      min={0}
      max={128}
      step={1}
      integer
    />
    <NumInput
      label="Main GPU"
      value={config.main_gpu ?? 0}
      onChange={(v) => onConfigChange('main_gpu', v)}
      min={0}
      max={7}
      step={1}
      integer
    />
    <div className="flex items-center gap-1.5">
      <label
        htmlFor="split-mode-select"
        className="text-xs text-muted-foreground whitespace-nowrap"
      >
        Split
      </label>
      <select
        id="split-mode-select"
        className="h-6 rounded border border-input bg-background px-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-ring"
        value={config.split_mode ?? 'layer'}
        onChange={(e) => onConfigChange('split_mode', e.target.value)}
      >
        {SPLIT_MODE_OPTIONS.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
    </div>
    <Toggle
      label="mlock"
      checked={config.use_mlock ?? false}
      onChange={(v) => onConfigChange('use_mlock', v)}
    />
    <Toggle
      label="mmap"
      checked={config.use_mmap ?? true}
      onChange={(v) => onConfigChange('use_mmap', v)}
    />
  </ParamGroup>
);

export const AdvancedContextSection: React.FC<AdvancedContextSectionProps> = ({
  config,
  onConfigChange,
}) => (
  <div className="flex flex-wrap gap-3">
    <KvCacheGroup config={config} onConfigChange={onConfigChange} />
    <ParamGroup title="Batch">
      <div className="flex gap-1">
        {BATCH_PRESETS.map((size) => (
          <button
            key={size}
            type="button"
            className={`px-2.5 py-0.5 text-xs rounded border transition-colors ${
              (config.n_batch ?? BATCH_SIZE_2048) === size
                ? 'bg-primary text-primary-foreground border-primary'
                : 'bg-background hover:bg-muted border-border'
            }`}
            onClick={() => onConfigChange('n_batch', size)}
          >
            {size}
          </button>
        ))}
      </div>
      <NumInput
        label="uBatch"
        value={config.n_ubatch ?? BATCH_SIZE_512}
        onChange={(v) => onConfigChange('n_ubatch', v)}
        min={32}
        max={8192}
        step={64}
        integer
      />
    </ParamGroup>
    <ParamGroup title="Context">
      <NumInput
        label="RoPE Base"
        value={config.rope_freq_base ?? 0}
        onChange={(v) => onConfigChange('rope_freq_base', v)}
        min={0}
        max={10000000}
        step={1000}
      />
      <NumInput
        label="RoPE Scale"
        value={config.rope_freq_scale ?? 0}
        onChange={(v) => onConfigChange('rope_freq_scale', v)}
        min={0}
        max={32}
        step={0.1}
      />
    </ParamGroup>
    <HardwareGroup config={config} onConfigChange={onConfigChange} />
  </div>
);
