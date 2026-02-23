import React from 'react';
import type { SystemExecBlock } from '../../../hooks/useMessageParsing';

interface CommandExecBlockProps {
  blocks: SystemExecBlock[];
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

/**
 * Display SYSTEM.EXEC command blocks with their outputs.
 * Shows an animated "executing" state when output is null (tool still running).
 */
export const CommandExecBlock: React.FC<CommandExecBlockProps> = ({ blocks }) => {
  if (blocks.length === 0) return null;

  return (
    <div className="space-y-3">
      {blocks.map((block, index) => {
        const isExecuting = block.output === null;

        return (
          <div
            key={`exec-${index}`}
            className={`rounded-lg overflow-hidden border ${
              isExecuting ? 'border-yellow-500/30' : 'border-green-500/30'
            }`}
          >
            {/* Command header */}
            <div className={`px-3 py-2 flex items-center gap-2 ${
              isExecuting ? 'bg-yellow-950/70' : 'bg-green-950/70'
            }`}>
              {isExecuting ? (
                <>
                  <span className="inline-block animate-spin text-sm">⏳</span>
                  <span className="text-xs font-medium text-yellow-300">Executing Tool...</span>
                </>
              ) : (
                <>
                  <span className="text-green-400">⚡</span>
                  <span className="text-xs font-medium text-green-300">Command Executed</span>
                </>
              )}
            </div>

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

            {/* Output (when execution complete) */}
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
          </div>
        );
      })}
    </div>
  );
};
