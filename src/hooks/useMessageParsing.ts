import { useMemo } from 'react';
import { autoParseToolCalls, stripToolCalls } from '../utils/toolParser';
import type { Message, ToolCall } from '../types';

export interface SystemExecBlock {
  command: string;
  output: string | null;
}

export type ContentSegment =
  | { type: 'text'; content: string }
  | { type: 'thinking'; content: string }
  | { type: 'exec'; command: string; output: string | null };

export interface ParsedMessage {
  toolCalls: ToolCall[];
  cleanContent: string;
  thinkingContent: string | null;
  systemExecBlocks: SystemExecBlock[];
  contentWithoutThinking: string;
  isError: boolean;
  orderedSegments: ContentSegment[];
}

/**
 * Parse a message and extract various components:
 * - Tool calls
 * - Thinking content (for reasoning models)
 * - SYSTEM.EXEC blocks (command executions)
 * - Clean content without special tags
 */
export function useMessageParsing(message: Message): ParsedMessage {
  // Parse tool calls from assistant messages
  const toolCalls = useMemo(() => {
    if (message.role === 'assistant') {
      return autoParseToolCalls(message.content);
    }
    return [];
  }, [message.content, message.role]);

  // Strip tool call markers from content
  const cleanContent = useMemo(() => {
    let content = message.content;
    if (toolCalls.length > 0) {
      content = stripToolCalls(content);
    }
    return content;
  }, [message.content, toolCalls.length]);

  // Extract thinking content (for reasoning models like Qwen3)
  const thinkingContent = useMemo(() => {
    const thinkMatch = message.content.match(/<think>([\s\S]*?)<\/think>/);
    return thinkMatch ? thinkMatch[1].trim() : null;
  }, [message.content]);

  // Extract SYSTEM.EXEC blocks (command executions)
  const systemExecBlocks = useMemo(() => {
    const blocks: SystemExecBlock[] = [];
    // Accept multiple tag variations:
    // - <||SYSTEM.EXEC>...<SYSTEM.EXEC||>  (canonical)
    // - [TOOL_CALLS]||SYSTEM.EXEC>...<SYSTEM.EXEC||>  (with TOOL_CALLS prefix)
    // - ||SYSTEM.EXEC>...<SYSTEM.EXEC||>  (without <)
    const execRegex =
      /(?:\[TOOL_CALLS\])?(?:<\|\||\|\||<)?SYSTEM\.EXEC>([\s\S]*?)<SYSTEM\.EXEC\|\|>/g;
    const outputRegex = /(?:<\|\||\|\||<)?SYSTEM\.OUTPUT>([\s\S]*?)<SYSTEM\.OUTPUT\|\|>/g;

    // Also match incomplete/unclosed tags during streaming
    const incompleteExecRegex =
      /(?:\[TOOL_CALLS\])?(?:<\|\||\|\||<)?SYSTEM\.EXEC>([\s\S]*?)(?:<SYSTEM\.EXEC\|\|>|$)/g;
    const incompleteOutputRegex =
      /(?:<\|\||\|\||<)?SYSTEM\.OUTPUT>([\s\S]*?)(?:<SYSTEM\.OUTPUT\|\|>|$)/g;

    let match;

    // First, try to match complete blocks (with closing tags)
    const completeMatches: { index: number; command: string; output: string | null }[] = [];
    while ((match = execRegex.exec(message.content)) !== null) {
      completeMatches.push({ index: match.index, command: match[1].trim(), output: null });
    }

    // If we have complete matches, use them
    if (completeMatches.length > 0) {
      blocks.push(...completeMatches);

      // Match outputs to commands (in order)
      let outputIndex = 0;
      while ((match = outputRegex.exec(message.content)) !== null) {
        if (outputIndex < blocks.length) {
          blocks[outputIndex].output = match[1].trim();
          outputIndex++;
        }
      }
    } else {
      // No complete matches, check for incomplete/streaming blocks
      while ((match = incompleteExecRegex.exec(message.content)) !== null) {
        blocks.push({ command: match[1].trim(), output: null });
      }

      // Match incomplete outputs to commands (in order)
      let outputIndex = 0;
      while ((match = incompleteOutputRegex.exec(message.content)) !== null) {
        if (outputIndex < blocks.length) {
          blocks[outputIndex].output = match[1].trim();
          outputIndex++;
        }
      }
    }

    return blocks;
  }, [message.content]);

  // Get content without thinking tags and SYSTEM.EXEC/OUTPUT tags
  const contentWithoutThinking = useMemo(() => {
    let content = cleanContent;
    content = content.replace(/<think>[\s\S]*?<\/think>/g, '');

    // Remove SYSTEM.EXEC blocks (canonical: <||SYSTEM.EXEC>...<SYSTEM.EXEC||>)
    // Also handles malformed: [TOOL_CALLS]||SYSTEM.EXEC>...<SYSTEM.EXEC||>
    // Also handles incomplete/streaming blocks without closing tags
    content = content.replace(
      /(?:\[TOOL_CALLS\])?(?:<\|\||\|\||<)?SYSTEM\.EXEC>[\s\S]*?(?:<SYSTEM\.EXEC\|\|>|$)/g,
      ''
    );

    // Remove SYSTEM.OUTPUT blocks (canonical: <||SYSTEM.OUTPUT>...<SYSTEM.OUTPUT||>)
    // Also handles incomplete/streaming blocks without closing tags
    content = content.replace(
      /(?:<\|\||\|\||<)?SYSTEM\.OUTPUT>[\s\S]*?(?:<SYSTEM\.OUTPUT\|\|>|$)/g,
      ''
    );

    // Strip standalone [[TOOL_CALLS]] or [TOOL_CALLS] markers
    content = content.replace(/\[\[?TOOL_CALLS\]\]?/g, '');

    // Strip any remaining standalone SYSTEM.EXEC/OUTPUT tags (orphaned tags)
    content = content.replace(/<\|\|SYSTEM\.(EXEC|OUTPUT)\|\|>/g, '');
    content = content.replace(/<SYSTEM\.(EXEC|OUTPUT)\|\|>/g, '');

    // Strip standalone || markers (often appear as line artifacts)
    content = content.replace(/^\|\|$/gm, '');
    content = content.replace(/^\s*\|\|>\s*$/gm, ''); // Also ||> at end of lines
    content = content.replace(/^\s*<?\|\|\s*$/gm, ''); // Also <|| or || alone

    // Clean up multiple consecutive newlines
    content = content.replace(/\n{3,}/g, '\n\n');

    // Strip any partial opening tags at the end (for streaming)
    // Patterns for partial opening tags that might appear during streaming
    const partialTagPatterns = [
      /<\|\|SYSTEM\.?$/, // <||SYSTEM. or <||SYSTEM
      /<\|\|SYST?E?M?\.?E?X?E?C?$/i, // <||S, <||SY, <||SYS, <||SYST, etc.
      /<\|\|SYST?E?M?\.?O?U?T?P?U?T?$/i, // <||S, <||SY, <||SYS, <||SYST, etc.
      /<th?i?n?k?$/i, // <t, <th, <thi, <thin, <think
      /\[T?O?O?L?_?C?A?L?L?S?$/i, // [, [T, [TO, [TOO, [TOOL, etc.
      /<\|?\|?$/, // <, <|, <||
      /<$/,  // lone <
    ];

    for (const pattern of partialTagPatterns) {
      if (pattern.test(content)) {
        content = content.replace(pattern, '');
        break;
      }
    }

    return content.trim();
  }, [cleanContent]);

  // Detect error messages
  const isError = message.role === 'system' && (
    message.content.includes('❌') ||
    message.content.includes('Generation Crashed') ||
    message.content.includes('Error')
  );

  // Parse content into ordered segments (preserves original order during streaming)
  const orderedSegments = useMemo((): ContentSegment[] => {
    const segments: ContentSegment[] = [];
    let content = cleanContent;
    let lastIndex = 0;

    // Find all tag positions
    interface TagMatch {
      type: 'thinking' | 'exec' | 'output';
      start: number;
      end: number;
      content: string;
    }
    const tags: TagMatch[] = [];

    // Find thinking tags
    const thinkRegex = /<think>([\s\S]*?)<\/think>/g;
    let match;
    while ((match = thinkRegex.exec(content)) !== null) {
      tags.push({
        type: 'thinking',
        start: match.index,
        end: match.index + match[0].length,
        content: match[1].trim(),
      });
    }

    // Find exec tags (both complete and incomplete)
    // Matches: [TOOL_CALLS]<||SYSTEM.EXEC>, <||SYSTEM.EXEC>, ||SYSTEM.EXEC>, SYSTEM.EXEC>
    const execRegex =
      /(?:\[TOOL_CALLS\])?(?:<\|\||\|\||<)?SYSTEM\.EXEC>([\s\S]*?)(?:<SYSTEM\.EXEC\|\|>|$)/g;
    while ((match = execRegex.exec(content)) !== null) {
      tags.push({
        type: 'exec',
        start: match.index,
        end: match.index + match[0].length,
        content: match[1].trim(),
      });
    }

    // Find output tags (both complete and incomplete)
    // Matches: <||SYSTEM.OUTPUT>, ||SYSTEM.OUTPUT>, SYSTEM.OUTPUT>
    const outputRegex = /(?:<\|\||\|\||<)?SYSTEM\.OUTPUT>([\s\S]*?)(?:<SYSTEM\.OUTPUT\|\|>|$)/g;
    while ((match = outputRegex.exec(content)) !== null) {
      tags.push({
        type: 'output',
        start: match.index,
        end: match.index + match[0].length,
        content: match[1].trim(),
      });
    }

    // Sort tags by position
    tags.sort((a, b) => a.start - b.start);

    // Build segments in order
    let execIndex = 0;
    const execCommands: string[] = [];

    // Helper function to clean orphaned tags from text segments
    const cleanOrphanedTags = (text: string): string => {
      let cleaned = text;

      // Remove orphaned opening tags
      cleaned = cleaned.replace(/<\|\|SYSTEM\.(EXEC|OUTPUT)>/g, '');

      // Remove orphaned closing tags
      cleaned = cleaned.replace(/<\|\|SYSTEM\.(EXEC|OUTPUT)\|\|>/g, '');
      cleaned = cleaned.replace(/<SYSTEM\.(EXEC|OUTPUT)\|\|>/g, '');

      // Remove [TOOL_CALLS] markers
      cleaned = cleaned.replace(/\[\[?TOOL_CALLS\]\]?/g, '');

      // Remove standalone || markers
      cleaned = cleaned.replace(/^\|\|$/gm, '');
      cleaned = cleaned.replace(/^\s*\|\|>\s*$/gm, '');
      cleaned = cleaned.replace(/^\s*<?\|\|\s*$/gm, '');

      return cleaned.trim();
    };

    // Helper function to strip partial/incomplete opening tags at the end of streaming content
    const stripTrailingPartialTags = (text: string): string => {
      // Patterns for partial opening tags that might appear during streaming
      const partialTagPatterns = [
        // Partial <||SYSTEM.EXEC> or <||SYSTEM.OUTPUT>
        /<\|\|SYSTEM\.?$/, // <||SYSTEM. or <||SYSTEM
        /<\|\|SYST?E?M?\.?E?X?E?C?$/i, // <||S, <||SY, <||SYS, <||SYST, <||SYSTE, <||SYSTEM, <||SYSTEM.E, etc.
        /<\|\|SYST?E?M?\.?O?U?T?P?U?T?$/i, // <||S, <||SY, <||SYS, <||SYST, <||SYSTE, <||SYSTEM, <||SYSTEM.O, etc.

        // Partial <think>
        /<th?i?n?k?$/i, // <t, <th, <thi, <thin, <think

        // Partial [TOOL_CALLS]
        /\[T?O?O?L?_?C?A?L?L?S?$/i, // [, [T, [TO, [TOO, [TOOL, [TOOL_, etc.

        // Very basic - just starting a tag
        /<\|?\|?$/, // <, <|, <||
        /<$/,  // lone <
      ];

      let cleaned = text;

      // Check each pattern and strip if matched at end
      for (const pattern of partialTagPatterns) {
        if (pattern.test(cleaned)) {
          cleaned = cleaned.replace(pattern, '');
          break; // Only strip one partial tag
        }
      }

      return cleaned;
    };

    for (const tag of tags) {
      // Add text before this tag
      if (tag.start > lastIndex) {
        let textContent = content.substring(lastIndex, tag.start);
        textContent = cleanOrphanedTags(textContent);
        textContent = stripTrailingPartialTags(textContent);
        if (textContent) {
          segments.push({ type: 'text', content: textContent });
        }
      }

      // Add the tag's segment
      if (tag.type === 'thinking') {
        segments.push({ type: 'thinking', content: tag.content });
      } else if (tag.type === 'exec') {
        execCommands.push(tag.content);
        segments.push({ type: 'exec', command: tag.content, output: null });
        execIndex++;
      } else if (tag.type === 'output') {
        // Match output to the last exec command
        const lastExecSegment = segments
          .slice()
          .reverse()
          .find((s) => s.type === 'exec' && s.output === null);
        if (lastExecSegment && lastExecSegment.type === 'exec') {
          lastExecSegment.output = tag.content;
        }
      }

      lastIndex = tag.end;
    }

    // Add remaining text after all tags
    if (lastIndex < content.length) {
      let textContent = content.substring(lastIndex);
      textContent = cleanOrphanedTags(textContent);
      textContent = stripTrailingPartialTags(textContent);
      if (textContent) {
        segments.push({ type: 'text', content: textContent });
      }
    }

    return segments;
  }, [cleanContent]);

  return {
    toolCalls,
    cleanContent,
    thinkingContent,
    systemExecBlocks,
    contentWithoutThinking,
    isError,
    orderedSegments,
  };
}
