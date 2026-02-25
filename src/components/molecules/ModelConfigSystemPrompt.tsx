import React, { useState } from 'react';
import { Button } from '../atoms/button';
import type { ModelMetadata } from '@/types';

export type SystemPromptMode = 'model' | 'system' | 'custom';

// Agentic mode system prompt (mirrors backend's get_universal_system_prompt)
const AGENTIC_SYSTEM_PROMPT = `You are a helpful AI assistant with full system access.

## CRITICAL: Command Execution Format

To execute system commands, you MUST use EXACTLY this format (copy it exactly):

<||SYSTEM.EXEC>command_here<SYSTEM.EXEC||>

The format is: opening tag <||SYSTEM.EXEC> then command then closing tag <SYSTEM.EXEC||>

IMPORTANT RULES:
1. Use ONLY this exact format - do NOT use [TOOL_CALLS], <function>, <tool_call>, or any other format
2. The opening tag MUST start with <|| (less-than, pipe, pipe)
3. The closing tag MUST end with ||> (pipe, pipe, greater-than)
4. Do NOT add any prefix before <||SYSTEM.EXEC>
5. Do NOT modify or abbreviate the tags

Examples (copy exactly):
<||SYSTEM.EXEC>dir<SYSTEM.EXEC||>
<||SYSTEM.EXEC>type filename.txt<SYSTEM.EXEC||>
<||SYSTEM.EXEC>echo content > filename.txt<SYSTEM.EXEC||>

After execution, the system will inject the result between <||SYSTEM.OUTPUT> and <SYSTEM.OUTPUT||> tags. Do NOT generate <||SYSTEM.OUTPUT> yourself — the system does this automatically. Wait for the injected result before continuing.

## Current Environment
- OS: (detected at runtime)
- Working Directory: (detected at runtime)
- Shell: cmd/powershell or bash`;

export interface SystemPromptSectionProps {
  systemPromptMode: SystemPromptMode;
  setSystemPromptMode: (mode: SystemPromptMode) => void;
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
}) => {
  const [isExpanded, setIsExpanded] = useState(false);

  const readOnlyText = systemPromptMode === 'model'
    ? (modelInfo?.default_system_prompt || '')
    : AGENTIC_SYSTEM_PROMPT;

  return (
    <div className="space-y-3 pt-2 border-t">
      <span className="text-sm font-medium">System Prompt</span>

      <div className="flex gap-2">
        <Button
          type="button"
          variant={systemPromptMode === 'model' ? 'default' : 'outline'}
          onClick={() => setSystemPromptMode('model')}
          className="flex-1 text-xs px-2"
        >
          Model Default
        </Button>
        <Button
          type="button"
          variant={systemPromptMode === 'system' ? 'default' : 'outline'}
          onClick={() => setSystemPromptMode('system')}
          className="flex-1 text-xs px-2"
        >
          Agentic Mode
        </Button>
        <Button
          type="button"
          variant={systemPromptMode === 'custom' ? 'default' : 'outline'}
          onClick={() => setSystemPromptMode('custom')}
          className="flex-1 text-xs px-2"
        >
          Custom
        </Button>
      </div>

      <div className="flex justify-end">
        <button
          type="button"
          onClick={() => setIsExpanded(!isExpanded)}
          className="text-xs text-muted-foreground hover:text-foreground transition-colors"
        >
          {isExpanded ? '▼ Hide prompt' : '◀ Show prompt'}
        </button>
      </div>

      {isExpanded ? <div className="space-y-2">
          {systemPromptMode === 'custom' ? (
            <textarea
              value={customSystemPrompt}
              onChange={(e) => setCustomSystemPrompt(e.target.value)}
              placeholder="Enter your custom system prompt..."
              className="w-full px-3 py-2 text-sm border rounded-md min-h-[100px] resize-y bg-background"
            />
          ) : readOnlyText ? (
            <pre className="w-full px-3 py-2 text-sm border rounded-md max-h-[200px] overflow-y-auto whitespace-pre-wrap bg-muted text-foreground">
              {readOnlyText}
            </pre>
          ) : (
            <p className="w-full px-3 py-2 text-sm text-muted-foreground italic">
              (No default system prompt found in model)
            </p>
          )}
          <p className="text-xs text-muted-foreground">
            {systemPromptMode === 'model'
              ? "Using the model's built-in default system prompt from GGUF chat template."
              : systemPromptMode === 'system'
                ? 'Agentic mode with command execution. The model can run system commands.'
                : 'Custom system prompt that will be used instead of the model\'s default.'}
          </p>
        </div> : null}
    </div>
  );
};
