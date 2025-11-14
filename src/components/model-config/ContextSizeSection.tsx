import React from 'react';
import { Button } from '@/components/ui/button';
import type { ModelMetadata } from '@/types';
import { CONTEXT_SIZE_PRESETS } from './constants';

export interface ContextSizeSectionProps {
  contextSize: number;
  setContextSize: (size: number) => void;
  modelInfo: ModelMetadata | null;
}

export const ContextSizeSection: React.FC<ContextSizeSectionProps> = ({
  contextSize,
  setContextSize,
  modelInfo
}) => (
  <div className="space-y-2">
    <div className="flex justify-between items-center">
      <label className="text-sm font-medium">Context Length</label>
      <span className="text-sm font-mono text-muted-foreground">
        {contextSize.toLocaleString()} tokens
      </span>
    </div>

    <input
      type="number"
      value={contextSize}
      onChange={(e) => {
        const value = parseInt(e.target.value);
        if (!isNaN(value) && value > 0) {
          setContextSize(value);
        }
      }}
      min={512}
      max={2097152}
      step={512}
      className="w-full px-3 py-2 text-sm border rounded-md bg-background"
    />

    <div className="flex gap-2 flex-wrap">
      {CONTEXT_SIZE_PRESETS.map(preset => (
        <Button
          key={preset}
          type="button"
          variant={contextSize === preset ? 'default' : 'outline'}
          size="sm"
          onClick={() => setContextSize(preset)}
          className="text-xs"
        >
          {preset >= 1048576 ? `${preset / 1048576}M` : preset >= 1024 ? `${preset / 1024}K` : preset}
        </Button>
      ))}
      {modelInfo?.context_length && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => {
            const maxContext = parseInt(modelInfo.context_length.toString().replace(/,/g, ''));
            if (!isNaN(maxContext)) {
              setContextSize(maxContext);
            }
          }}
          className="text-xs bg-muted hover:bg-muted/80"
        >
          Max ({parseInt(modelInfo.context_length.toString().replace(/,/g, '')).toLocaleString()})
        </Button>
      )}
    </div>

    <p className="text-xs text-muted-foreground">
      Larger context sizes allow longer conversations but use more memory and are slower.
      {modelInfo?.context_length && ` Model maximum: ${modelInfo.context_length}`}
    </p>
  </div>
);
