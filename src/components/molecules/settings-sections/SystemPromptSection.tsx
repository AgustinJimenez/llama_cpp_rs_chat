import React from 'react';
import { Card, CardContent, CardHeader, CardTitle } from '../../atoms/card';

interface SystemPromptSectionProps {
  systemPrompt: string;
  onSystemPromptChange: (prompt: string) => void;
}

export const SystemPromptSection: React.FC<SystemPromptSectionProps> = ({
  systemPrompt,
  onSystemPromptChange
}) => {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm">System Prompt</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <textarea
          value={systemPrompt || ''}
          onChange={(e) => onSystemPromptChange(e.target.value)}
          placeholder="Enter system prompt for new conversations..."
          className="w-full px-3 py-2 text-sm border border-input rounded-md bg-background min-h-[100px] resize-vertical"
          rows={4}
        />
        <p className="text-xs text-muted-foreground">
          This prompt will be added at the beginning of every new conversation to set the AI&#39;s behavior and context.
        </p>
      </CardContent>
    </Card>
  );
};
