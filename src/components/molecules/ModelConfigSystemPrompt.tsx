import React, { useState } from 'react';
import { Button } from '../atoms/button';
import { ChevronDown, ChevronUp } from 'lucide-react';
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

After execution, the output will appear in:
<||SYSTEM.OUTPUT>
...output here...
<SYSTEM.OUTPUT||>

Wait for the output before continuing your response.

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
  const [isCollapsed, setIsCollapsed] = useState(true);

  return (
    <div className="space-y-3 pt-2 border-t">
      <label className="text-sm font-medium">System Prompt</label>

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

      <div className="space-y-2">
        <button
          type="button"
          onClick={() => setIsCollapsed(!isCollapsed)}
          className="w-full flex items-center justify-between text-sm text-muted-foreground hover:opacity-70 transition-opacity py-2 px-3 rounded-md border border-border bg-muted/30"
        >
          <span>
            {systemPromptMode === 'model'
              ? "Using the model's built-in default system prompt from GGUF chat template."
              : systemPromptMode === 'system'
                ? 'Agentic mode with command execution. The model can run system commands.'
                : 'Custom system prompt that will be used instead of the model\'s default.'}
          </span>
          {isCollapsed ? (
            <ChevronDown className="h-4 w-4 flex-shrink-0 ml-2" />
          ) : (
            <ChevronUp className="h-4 w-4 flex-shrink-0 ml-2" />
          )}
        </button>

        {!isCollapsed && (
          <textarea
            value={
              systemPromptMode === 'model'
                ? (modelInfo?.default_system_prompt || '(No default system prompt found in model)')
                : systemPromptMode === 'system'
                  ? AGENTIC_SYSTEM_PROMPT
                  : customSystemPrompt
            }
            onChange={(e) => {
              if (systemPromptMode === 'custom') {
                setCustomSystemPrompt(e.target.value);
              }
            }}
            disabled={systemPromptMode !== 'custom'}
            placeholder={systemPromptMode === 'custom' ? 'Enter your custom system prompt...' : ''}
            className={`w-full px-3 py-2 text-sm border-2 rounded-md min-h-[400px] resize-y ${
              systemPromptMode === 'custom'
                ? 'bg-background border-primary/50 focus:border-primary'
                : systemPromptMode === 'system'
                  ? 'border-blue-500/50 bg-blue-500/5 text-blue-300 cursor-not-allowed'
                  : 'bg-muted text-muted-foreground cursor-not-allowed opacity-70 border-border'
            }`}
          />
        )}
      </div>
    </div>
  );
};
