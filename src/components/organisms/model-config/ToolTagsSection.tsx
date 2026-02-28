import React from 'react';
import { RotateCcw } from 'lucide-react';
import type { SamplerConfig, ModelMetadata } from '@/types';

interface ToolTagsSectionProps {
  config: SamplerConfig;
  onConfigChange: (field: keyof SamplerConfig, value: string | number | boolean) => void;
  modelInfo: ModelMetadata | null;
}

const TAG_FIELDS = [
  { key: 'tool_tag_exec_open' as keyof SamplerConfig, label: 'Exec Open', descKey: 'exec_open' },
  { key: 'tool_tag_exec_close' as keyof SamplerConfig, label: 'Exec Close', descKey: 'exec_close' },
  { key: 'tool_tag_output_open' as keyof SamplerConfig, label: 'Output Open', descKey: 'output_open' },
  { key: 'tool_tag_output_close' as keyof SamplerConfig, label: 'Output Close', descKey: 'output_close' },
] as const;

export const ToolTagsSection: React.FC<ToolTagsSectionProps> = ({
  config,
  onConfigChange,
  modelInfo,
}) => {
  const detected = modelInfo?.detected_tool_tags;

  return (
    <div className="rounded-md border border-zinc-700 px-3 py-2 space-y-2">
      <span className="text-xs font-medium">Tool Tags</span>
      <div className="grid grid-cols-2 gap-x-4 gap-y-2">
        {TAG_FIELDS.map((field) => {
          const placeholder =
            detected?.[field.descKey as keyof typeof detected] ?? '';
          const value = (config[field.key] as string | undefined) ?? '';

          return (
            <div key={field.key} className="flex items-center gap-1.5">
              <label className="text-xs text-muted-foreground whitespace-nowrap shrink-0">{field.label}</label>
              <input
                type="text"
                className="flex-1 h-6 rounded border border-input bg-background px-1.5 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-ring"
                placeholder={placeholder || 'auto-detected'}
                value={value}
                onChange={(e) => onConfigChange(field.key, e.target.value)}
              />
              {value ? <button
                  type="button"
                  className="text-muted-foreground hover:text-foreground transition-colors shrink-0"
                  title="Reset to auto-detected"
                  onClick={() => onConfigChange(field.key, '')}
                >
                  <RotateCcw className="h-3 w-3" />
                </button> : null}
            </div>
          );
        })}
      </div>
    </div>
  );
};
