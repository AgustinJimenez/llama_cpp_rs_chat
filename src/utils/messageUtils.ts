import type { Message } from '../types';

/**
 * Process raw messages from the backend API into display-ready Message objects.
 *
 * - Marks the first real system message (the prompt) with `isSystemPrompt: true`
 *   so MessageBubble renders it as a collapsed <details> block.
 * - Lets compaction summaries and crash-recovery notes through (they start with
 *   '[Conversation summary', '[Compacted history', or '[System:').
 * - Filters out hidden [TOOL_RESULTS] messages.
 * - Drops any additional duplicate system prompt messages.
 *
 * Call this every time you map raw API messages to UI state, instead of
 * duplicating this logic in each hook.
 */
export function processConversationMessages(
  rawMessages: Array<Record<string, unknown>>,
): Message[] {
  let systemPromptSeen = false;
  const mapped = rawMessages.map((msg) => ({
    id: String(msg.id ?? ''),
    role: String(msg.role ?? '') as Message['role'],
    content: String(msg.content ?? ''),
    timestamp: Number(msg.timestamp ?? 0),
  }));

  return mapped.filter((msg) => {
    if (msg.role === 'system') {
      if (
        msg.content.startsWith('[Conversation summary') ||
        msg.content.startsWith('[Compacted history')
      ) {
        return true;
      }
      if (msg.content.startsWith('[System:')) {
        return true;
      }
      if (!systemPromptSeen) {
        systemPromptSeen = true;
        (msg as Message).isSystemPrompt = true;
        return true;
      }
      return false;
    }
    return !msg.content.startsWith('[TOOL_RESULTS]');
  });
}
