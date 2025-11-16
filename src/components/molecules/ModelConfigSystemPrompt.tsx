import React from 'react';
import { Button } from '../atoms/button';
import type { ModelMetadata } from '@/types';

export interface SystemPromptSectionProps {
  systemPromptMode: 'default' | 'custom';
  setSystemPromptMode: (mode: 'default' | 'custom') => void;
  customSystemPrompt: string;
  setCustomSystemPrompt: (prompt: string) => void;
  modelInfo: ModelMetadata | null;
}

export const SystemPromptSection: React.FC<SystemPromptSectionProps> = ({
  systemPromptMode,
  setSystemPromptMode,
  customSystemPrompt,
  setCustomSystemPrompt,
  modelInfo
}) => (
  <div className="space-y-3 pt-2 border-t">
    <label className="text-sm font-medium">System Prompt</label>

    <div className="flex gap-2">
      <Button
        type="button"
        variant={systemPromptMode === 'default' ? 'default' : 'outline'}
        onClick={() => setSystemPromptMode('default')}
        className="flex-1"
      >
        Use Model Default
      </Button>
      <Button
        type="button"
        variant={systemPromptMode === 'custom' ? 'default' : 'outline'}
        onClick={() => setSystemPromptMode('custom')}
        className="flex-1"
      >
        Custom Prompt
      </Button>
    </div>

    {systemPromptMode === 'default' ? (
      <div className="space-y-2">
        <p className="text-xs text-muted-foreground">
          Using the model's built-in default system prompt from chat template.
        </p>
        {modelInfo?.default_system_prompt && (
          <div className="p-3 bg-muted rounded-md text-xs max-h-40 overflow-y-auto">
            <pre className="whitespace-pre-wrap break-words text-muted-foreground">
              {modelInfo.default_system_prompt}
            </pre>
          </div>
        )}
      </div>
    ) : (
      <div className="space-y-2">
        <textarea
          value={customSystemPrompt}
          onChange={(e) => setCustomSystemPrompt(e.target.value)}
          placeholder="Enter your custom system prompt..."
          className="w-full px-3 py-2 text-sm border rounded-md bg-background min-h-[100px] resize-y"
        />
        <p className="text-xs text-muted-foreground">
          Custom system prompt that will be used instead of the model's default.
        </p>
      </div>
    )}
  </div>
);
