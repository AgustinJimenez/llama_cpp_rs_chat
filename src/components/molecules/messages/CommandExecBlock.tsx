import React, { useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import type { SystemExecBlock } from '../../../hooks/useMessageParsing';

interface CommandExecBlockProps {
  blocks: SystemExecBlock[];
}

/** Extract a short summary like: web_search --query "today's news about Mexico..." */
function getCommandSummary(command: string, maxLen = 60): string {
  // Try JSON format: {"name":"web_search","arguments":{"query":"..."}}
  try {
    const parsed = JSON.parse(command);
    if (parsed.name && parsed.arguments) {
      const parts = Object.entries(parsed.arguments).map(([k, v]) => `--${k} "${v}"`);
      const full = `${parsed.name}  ${parts.join(' ')}`;
      return full.length > maxLen ? full.slice(0, maxLen - 3) + '...' : full;
    }
  } catch { /* not JSON */ }

  // tool_name({"key":"value",...})
  const funcMatch = command.match(/^(\w+)\((\{.*\})\)$/);
  if (funcMatch) {
    const [, name, argsJson] = funcMatch;
    try {
      const args = JSON.parse(argsJson);
      const parts = Object.entries(args).map(([k, v]) => `--${k} "${v}"`);
      const full = `${name}  ${parts.join(' ')}`;
      return full.length > maxLen ? full.slice(0, maxLen - 3) + '...' : full;
    } catch { /* fall through */ }
  }

  // tool_name: value or plain text
  return command.length > maxLen ? command.slice(0, maxLen - 3) + '...' : command;
}

/** Extract a human-readable status from the command string. */
function getToolStatusDescription(command: string): string {
  // web_search({"query":"Mexico news"})
  const searchMatch = command.match(/^web_search\((\{.*\})\)$/);
  if (searchMatch) {
    try {
      const { query } = JSON.parse(searchMatch[1]);
      if (query) return `Searching the web for: ${query}`;
    } catch { /* fall through */ }
    return 'Searching the web...';
  }

  // web_fetch({"url":"https://..."})
  const fetchMatch = command.match(/^web_fetch\((\{.*\})\)$/);
  if (fetchMatch) {
    try {
      const { url } = JSON.parse(fetchMatch[1]);
      if (url) return `Fetching page: ${url}`;
    } catch { /* fall through */ }
    return 'Fetching web page...';
  }

  // read_file: /path/to/file
  if (command.startsWith('read_file:')) {
    return `Reading file: ${command.slice(10).trim()}`;
  }

  // execute_command({"command":"ls -la"})
  const execMatch = command.match(/^execute_command\((\{.*\})\)$/);
  if (execMatch) {
    try {
      const { command: cmd } = JSON.parse(execMatch[1]);
      if (cmd) return `Running: ${cmd}`;
    } catch { /* fall through */ }
    return 'Running command...';
  }

  // Fallback: truncate long commands
  const truncated = command.length > 60 ? command.slice(0, 57) + '...' : command;
  return `Executing: ${truncated}`;
}

const CommandBlock: React.FC<{ block: SystemExecBlock; index: number }> = ({ block, index }) => {
  const isExecuting = block.output === null;
  const [isOpen, setIsOpen] = useState(isExecuting);

  // Auto-collapse when execution completes
  const prevExecuting = React.useRef(isExecuting);
  React.useEffect(() => {
    if (prevExecuting.current && !isExecuting) {
      setIsOpen(false);
    }
    prevExecuting.current = isExecuting;
  }, [isExecuting]);

  return (
    <div
      key={`exec-${index}`}
      className={`rounded-lg overflow-hidden border ${
        isExecuting ? 'border-yellow-500/30' : 'border-green-500/30'
      }`}
    >
      {/* Command header (clickable toggle when completed) */}
      <button
        type="button"
        onClick={() => !isExecuting && setIsOpen(!isOpen)}
        className={`w-full px-3 py-2 flex items-center justify-between ${
          isExecuting ? 'bg-yellow-950/70' : 'bg-green-950/70 cursor-pointer'
        }`}
      >
        <div className="flex items-center gap-2">
          {isExecuting ? (
            <>
              <span className="inline-block w-3 h-3 border-2 border-yellow-400 border-t-transparent rounded-full animate-spin" />
              <span className="text-xs font-medium text-yellow-300">Executing Tool...</span>
            </>
          ) : (
            <>
              <span className="text-xs font-medium text-green-300">Command Executed</span>
              <span className="text-xs text-green-300/50 font-mono truncate">{getCommandSummary(block.command)}</span>
            </>
          )}
        </div>
        {!isExecuting && (
          isOpen
            ? <ChevronDown className="w-3.5 h-3.5 text-green-500" />
            : <ChevronRight className="w-3.5 h-3.5 text-green-500" />
        )}
      </button>

      {/* Command content + details (visible when executing or expanded) */}
      {(isExecuting || isOpen) && (
        <>
          {/* Command content */}
          <div className="bg-black/40 px-3 py-2 overflow-hidden">
            <code className={`text-sm font-mono break-all ${
              isExecuting ? 'text-yellow-200' : 'text-green-200'
            }`}>
              {block.command}
            </code>
          </div>

          {/* Executing indicator (when output is null) */}
          {isExecuting && (
            <div className="bg-black/60 px-3 py-2.5 flex items-center gap-2.5 border-t border-yellow-500/20">
              <div className="flex gap-1 items-center">
                <div className="w-1.5 h-1.5 bg-yellow-400 rounded-full animate-bounce" style={{ animationDelay: '0ms', animationDuration: '1s' }} />
                <div className="w-1.5 h-1.5 bg-yellow-400 rounded-full animate-bounce" style={{ animationDelay: '200ms', animationDuration: '1s' }} />
                <div className="w-1.5 h-1.5 bg-yellow-400 rounded-full animate-bounce" style={{ animationDelay: '400ms', animationDuration: '1s' }} />
              </div>
              <span className="text-xs text-yellow-300/70">
                {getToolStatusDescription(block.command)}
              </span>
            </div>
          )}

          {/* Output */}
          {block.output && (
            <>
              <div className="bg-gray-900/50 px-3 py-1 border-t border-green-500/20">
                <span className="text-xs text-gray-400">Output:</span>
              </div>
              <div className="bg-black/60 px-3 py-2 max-h-64 overflow-auto">
                <pre className="text-xs text-gray-300 font-mono whitespace-pre-wrap break-all">
                  {block.output}
                </pre>
              </div>
            </>
          )}
        </>
      )}
    </div>
  );
};

/**
 * Display SYSTEM.EXEC command blocks with their outputs.
 * Shows an animated "executing" state when output is null (tool still running).
 */
export const CommandExecBlock: React.FC<CommandExecBlockProps> = ({ blocks }) => {
  if (blocks.length === 0) return null;

  return (
    <div className="space-y-3">
      {blocks.map((block, index) => (
        <CommandBlock key={`exec-${index}`} block={block} index={index} />
      ))}
    </div>
  );
};
