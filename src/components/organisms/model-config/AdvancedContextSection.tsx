import React from 'react';
import { Zap } from 'lucide-react';
import type { SamplerConfig } from '@/types';

const KV_CACHE_OPTIONS = [
  { value: 'f16', label: 'F16 (default)', description: 'Full precision, best quality' },
  { value: 'q8_0', label: 'Q8_0', description: '~50% less KV memory, minimal quality loss' },
  { value: 'q4_0', label: 'Q4_0', description: '~75% less KV memory, some quality loss' },
];

const BATCH_PRESETS = [512, 1024, 2048, 4096];

interface AdvancedContextSectionProps {
  config: SamplerConfig;
  onConfigChange: (field: keyof SamplerConfig, value: string | number | boolean) => void;
}

export const AdvancedContextSection: React.FC<AdvancedContextSectionProps> = ({
  config,
  onConfigChange,
}) => {
  return (
    <div className="space-y-4">
      <h4 className="text-xs font-semibold text-muted-foreground uppercase tracking-wider flex items-center gap-1.5">
        <Zap className="h-3.5 w-3.5" />
        Advanced Context Options
      </h4>

      {/* Flash Attention */}
      <div className="flex items-center justify-between">
        <div>
          <label className="text-sm font-medium">Flash Attention</label>
          <p className="text-xs text-muted-foreground">Faster inference, lower memory usage</p>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={config.flash_attention ?? false}
          className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
            config.flash_attention ? 'bg-primary' : 'bg-muted'
          }`}
          onClick={() => onConfigChange('flash_attention', !(config.flash_attention ?? false))}
        >
          <span
            className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
              config.flash_attention ? 'translate-x-6' : 'translate-x-1'
            }`}
          />
        </button>
      </div>

      {/* KV Cache Quantization */}
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="text-sm font-medium block mb-1">KV Cache K Type</label>
          <select
            className="w-full rounded-md border bg-background px-3 py-1.5 text-sm"
            value={config.cache_type_k ?? 'f16'}
            onChange={(e) => onConfigChange('cache_type_k', e.target.value)}
          >
            {KV_CACHE_OPTIONS.map(opt => (
              <option key={opt.value} value={opt.value}>{opt.label}</option>
            ))}
          </select>
        </div>
        <div>
          <label className="text-sm font-medium block mb-1">KV Cache V Type</label>
          <select
            className="w-full rounded-md border bg-background px-3 py-1.5 text-sm"
            value={config.cache_type_v ?? 'f16'}
            onChange={(e) => onConfigChange('cache_type_v', e.target.value)}
          >
            {KV_CACHE_OPTIONS.map(opt => (
              <option key={opt.value} value={opt.value}>{opt.label}</option>
            ))}
          </select>
        </div>
      </div>
      {(config.cache_type_k !== 'f16' || config.cache_type_v !== 'f16') && (
        <p className="text-xs text-amber-500">
          Quantized KV cache saves VRAM but may slightly reduce output quality.
        </p>
      )}

      {/* Batch Size */}
      <div>
        <div className="flex items-center justify-between mb-1">
          <label className="text-sm font-medium">Batch Size</label>
          <span className="text-xs text-muted-foreground">{config.n_batch ?? 2048}</span>
        </div>
        <div className="flex gap-1.5">
          {BATCH_PRESETS.map(size => (
            <button
              key={size}
              type="button"
              className={`flex-1 px-2 py-1 text-xs rounded border transition-colors ${
                (config.n_batch ?? 2048) === size
                  ? 'bg-primary text-primary-foreground border-primary'
                  : 'bg-background hover:bg-muted border-border'
              }`}
              onClick={() => onConfigChange('n_batch', size)}
            >
              {size}
            </button>
          ))}
        </div>
        <p className="text-xs text-muted-foreground mt-1">
          Tokens processed per batch during prompt evaluation. Higher = faster prompt processing.
        </p>
      </div>
    </div>
  );
};
