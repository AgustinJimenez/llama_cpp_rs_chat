import React, { useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';

export type SystemPromptMode = 'system' | 'custom';

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
}

export const SystemPromptSection: React.FC<SystemPromptSectionProps> = ({
  systemPromptMode,
  setSystemPromptMode,
  customSystemPrompt,
  setCustomSystemPrompt,
}) => {
  const [isExpanded, setIsExpanded] = useState(false);

  return (
    <div className="space-y-1">
      <div className="flex items-center gap-2">
        <span className="text-xs font-medium">System Prompt</span>
        <div className="flex rounded-full border border-input overflow-hidden">
          <button
            type="button"
            className={`px-3 py-0.5 text-xs transition-colors ${
              systemPromptMode === 'system'
                ? 'bg-primary text-primary-foreground'
                : 'bg-background hover:bg-muted text-muted-foreground'
            }`}
            onClick={() => setSystemPromptMode('system')}
          >
            Agentic
          </button>
          <button
            type="button"
            className={`px-3 py-0.5 text-xs transition-colors ${
              systemPromptMode === 'custom'
                ? 'bg-primary text-primary-foreground'
                : 'bg-background hover:bg-muted text-muted-foreground'
            }`}
            onClick={() => setSystemPromptMode('custom')}
          >
            Custom
          </button>
        </div>
        <button
          type="button"
          onClick={() => setIsExpanded(!isExpanded)}
          className="p-0.5 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors"
        >
          {isExpanded ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
        </button>
      </div>

      {isExpanded && (
        systemPromptMode === 'custom' ? (
          <textarea
            value={customSystemPrompt}
            onChange={(e) => setCustomSystemPrompt(e.target.value)}
            placeholder="Enter your custom system prompt..."
            className="w-full px-2 py-1.5 text-xs border rounded-md min-h-[80px] resize-y bg-background"
          />
        ) : (
          <pre className="w-full px-2 py-1.5 text-xs border rounded-md max-h-[150px] overflow-y-auto whitespace-pre-wrap bg-muted text-foreground">
            {AGENTIC_SYSTEM_PROMPT}
          </pre>
        )
      )}
    </div>
  );
};
