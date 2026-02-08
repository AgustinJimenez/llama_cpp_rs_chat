import type { Message } from '../types';

interface ParseState {
  currentRole: string;
  currentContent: string;
  hasSystemPrompt: boolean;
  systemPromptContent: string;
}

function isRoleHeader(line: string): string | null {
  if (!line.endsWith(':')) return null;
  for (const role of ['SYSTEM', 'USER', 'ASSISTANT']) {
    if (line === `${role}:`) return role;
  }
  return null;
}

function flushMessage(state: ParseState, messages: Message[]) {
  if (!state.currentRole || !state.currentContent.trim()) return;

  const message = createMessageIfValid(
    state.currentRole,
    state.currentContent.trim(),
    state.hasSystemPrompt,
    state.systemPromptContent
  );
  if (!message) return;

  if (message.role === 'system' && message.isSystemPrompt) {
    state.hasSystemPrompt = true;
    state.systemPromptContent = message.content;
  }
  messages.push(message);
}

/**
 * Parse conversation file content into Message array.
 * Handles SYSTEM, USER, and ASSISTANT messages, filtering out
 * tool results and tool-only responses for cleaner UI display.
 */
export function parseConversationFile(content: string): Message[] {
  const messages: Message[] = [];
  const state: ParseState = { currentRole: '', currentContent: '', hasSystemPrompt: false, systemPromptContent: '' };

  for (const line of content.split('\n')) {
    const role = isRoleHeader(line);
    if (role) {
      flushMessage(state, messages);
      state.currentRole = role;
      state.currentContent = '';
    } else if (!line.startsWith('[COMMAND:') && line.trim()) {
      state.currentContent += line + '\n';
    }
  }

  flushMessage(state, messages);
  return messages;
}

/**
 * Create a message if it passes validation (not system, not tool-only, etc.)
 */
function createMessageIfValid(
  currentRole: string,
  content: string,
  hasSystemPrompt: boolean,
  systemPromptContent: string
): Message | null {
  const role = currentRole === 'USER' ? 'user' : currentRole === 'ASSISTANT' ? 'assistant' : 'system';

  // Always allow system messages; mark the first as the system prompt.
  if (role === 'system') {
    if (hasSystemPrompt && content === systemPromptContent) return null;
    return {
      id: crypto.randomUUID(),
      role: 'system',
      content,
      timestamp: Date.now(),
      isSystemPrompt: !hasSystemPrompt,
    };
  }

  // Skip tool results
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

  const hasToolCall =
    contentWithoutThinking.includes('<tool_call>') ||
    contentWithoutThinking.includes('<function=') ||
    contentWithoutThinking.includes('[TOOL_CALLS]');

  if (!hasToolCall) return false;

  const contentWithoutTools = contentWithoutThinking
    .replace(/<tool_call>[\s\S]*?<\/tool_call>/g, '')
    .replace(/<function=[\s\S]*?<\/function>/g, '')
    .replace(/\[TOOL_CALLS\][\s\S]*?\[\/ARGS\]/g, '')
    .trim();

  return !contentWithoutTools;
}
