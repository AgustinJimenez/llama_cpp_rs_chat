import React, { useState } from 'react';
import { Tag, ChevronDown, ChevronRight, RotateCcw } from 'lucide-react';
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
  const [isExpanded, setIsExpanded] = useState(false);
  const detected = modelInfo?.detected_tool_tags;

  const hasOverrides = TAG_FIELDS.some(
    (f) => (config[f.key] as string | undefined)?.trim()
  );

  return (
    <div className="space-y-3">
      <button
        type="button"
        className="flex items-center gap-1.5 w-full text-left"
        onClick={() => setIsExpanded(!isExpanded)}
      >
        {isExpanded ? (
          <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 text-muted-foreground" />
        )}
        <h4 className="text-xs font-semibold text-muted-foreground uppercase tracking-wider flex items-center gap-1.5">
          <Tag className="h-3.5 w-3.5" />
          Tool Tags
          {hasOverrides && (
            <span className="text-[10px] font-normal normal-case tracking-normal text-amber-500">
              (overridden)
            </span>
          )}
        </h4>
      </button>

      {isExpanded && (
        <div className="space-y-3">
          <p className="text-xs text-muted-foreground">
            Tags used to detect and wrap tool calls in model output. Auto-detected from model name.
            Override only if the model uses non-standard tags.
          </p>

          <div className="grid grid-cols-2 gap-3">
            {TAG_FIELDS.map((field) => {
              const placeholder =
                detected?.[field.descKey as keyof typeof detected] ?? '';
              const value = (config[field.key] as string | undefined) ?? '';

              return (
                <div key={field.key}>
                  <div className="flex items-center justify-between mb-1">
                    <span className="text-sm font-medium">{field.label}</span>
                    {value && (
                      <button
                        type="button"
                        className="text-muted-foreground hover:text-foreground transition-colors"
                        title="Reset to auto-detected"
                        onClick={() => onConfigChange(field.key, '')}
                      >
                        <RotateCcw className="h-3 w-3" />
                      </button>
                    )}
                  </div>
                  <input
                    type="text"
                    className="w-full rounded-md border bg-background px-3 py-1.5 text-sm font-mono"
                    placeholder={placeholder || 'auto-detected'}
                    value={value}
                    onChange={(e) => onConfigChange(field.key, e.target.value)}
                  />
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
};
