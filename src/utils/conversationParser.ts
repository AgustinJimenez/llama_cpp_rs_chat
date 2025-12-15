import type { Message } from '../types';

/**
 * Parse conversation file content into Message array.
 * Handles SYSTEM, USER, and ASSISTANT messages, filtering out
 * tool results and tool-only responses for cleaner UI display.
 */
export function parseConversationFile(content: string): Message[] {
  const messages: Message[] = [];
  let currentRole = '';
  let currentContent = '';

  for (const line of content.split('\n')) {
    if (
      line.endsWith(':') &&
      (line.startsWith('SYSTEM:') || line.startsWith('USER:') || line.startsWith('ASSISTANT:'))
    ) {
      // Save previous message if it exists
      if (currentRole && currentContent.trim()) {
        const message = createMessageIfValid(currentRole, currentContent.trim());
        if (message) {
          messages.push(message);
        }
      }

      // Start new message
      currentRole = line.replace(':', '');
      currentContent = '';
    } else if (!line.startsWith('[COMMAND:') && line.trim()) {
      // Skip command execution logs, add content
      currentContent += line + '\n';
    }
  }

  // Add the final message
  if (currentRole && currentContent.trim()) {
    const message = createMessageIfValid(currentRole, currentContent.trim());
    if (message) {
      messages.push(message);
    }
  }

  return messages;
}

/**
 * Create a message if it passes validation (not system, not tool-only, etc.)
 */
function createMessageIfValid(currentRole: string, content: string): Message | null {
  const role = currentRole === 'USER' ? 'user' : currentRole === 'ASSISTANT' ? 'assistant' : 'system';

  // Skip system messages and tool results
  if (role === 'system') return null;
  if (content.startsWith('[TOOL_RESULTS]')) return null;

  // Check if message only contains tool calls (and optionally thinking tags)
  if (isToolCallOnly(content)) return null;

  return {
    id: crypto.randomUUID(),
    role: role as 'user' | 'assistant',
    content,
    timestamp: Date.now(),
  };
}

/**
 * Check if content only contains tool calls (no meaningful text)
 */
function isToolCallOnly(content: string): boolean {
  const contentWithoutThinking = content.replace(/<think>[\s\S]*?<\/think>/g, '').trim();

  const hasQwenToolCall = contentWithoutThinking.includes('<tool_call>');
  const hasLlama3ToolCall = contentWithoutThinking.includes('<function=');
  const hasMistralToolCall = contentWithoutThinking.includes('[TOOL_CALLS]');
  const hasToolCall = hasQwenToolCall || hasLlama3ToolCall || hasMistralToolCall;

  if (!hasToolCall) return false;

  const contentWithoutTools = contentWithoutThinking
    .replace(/<tool_call>[\s\S]*?<\/tool_call>/g, '')
    .replace(/<function=[\s\S]*?<\/function>/g, '')
    .replace(/\[TOOL_CALLS\][\s\S]*?\[\/ARGS\]/g, '')
    .trim();

  return !contentWithoutTools;
}
