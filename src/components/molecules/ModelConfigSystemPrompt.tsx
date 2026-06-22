import { ChevronDown, ChevronRight } from 'lucide-react';
import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';

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
  const { t } = useTranslation();
  const [isExpanded, setIsExpanded] = useState(false);
  const expandIcon = isExpanded ? (
    <ChevronDown className="size-3.5" />
  ) : (
    <ChevronRight className="size-3.5" />
  );

  return (
    <div className="space-y-1">
      <div className="flex items-center gap-2">
        <span className="text-xs font-medium">{t('modelConfig.systemPrompt')}</span>
        <div className="flex overflow-hidden rounded-full border border-input">
          <button
            type="button"
            className={`px-3 py-0.5 text-xs transition-colors ${
              systemPromptMode === 'system'
                ? 'bg-primary text-primary-foreground'
                : 'bg-background text-muted-foreground hover:bg-muted'
            }`}
            onClick={() => setSystemPromptMode('system')}
          >
            {t('modelConfig.agenticMode')}
          </button>
          <button
            type="button"
            className={`px-3 py-0.5 text-xs transition-colors ${
              systemPromptMode === 'custom'
                ? 'bg-primary text-primary-foreground'
                : 'bg-background text-muted-foreground hover:bg-muted'
            }`}
            onClick={() => setSystemPromptMode('custom')}
          >
            {t('modelConfig.customMode')}
          </button>
        </div>
        <button
          type="button"
          onClick={() => setIsExpanded(!isExpanded)}
          className="rounded p-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          {expandIcon}
        </button>
      </div>

      {!!isExpanded && systemPromptMode === 'custom' && (
        <textarea
          value={customSystemPrompt}
          onChange={(e) => setCustomSystemPrompt(e.target.value)}
          placeholder="Enter your custom system prompt..."
          className="min-h-[80px] w-full resize-y rounded-md border bg-background px-2 py-1.5 text-xs"
        />
      )}
      {!!isExpanded && systemPromptMode !== 'custom' && (
        <pre className="max-h-[150px] w-full overflow-y-auto whitespace-pre-wrap rounded-md border bg-muted px-2 py-1.5 text-xs text-foreground">
          {AGENTIC_SYSTEM_PROMPT}
        </pre>
      )}
    </div>
  );
};
