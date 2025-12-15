import React from 'react';
import type { SystemExecBlock } from '../../../hooks/useMessageParsing';

interface CommandExecBlockProps {
  blocks: SystemExecBlock[];
}

/**
 * Display SYSTEM.EXEC command blocks with their outputs.
 */
export const CommandExecBlock: React.FC<CommandExecBlockProps> = ({ blocks }) => {
  if (blocks.length === 0) return null;

  return (
    <div className="space-y-3">
      {blocks.map((block, index) => (
        <div
          key={`exec-${index}`}
          className="rounded-lg overflow-hidden border border-green-500/30"
        >
          {/* Command header */}
          <div className="bg-green-950/70 px-3 py-2 flex items-center gap-2">
            <span className="text-green-400">⚡</span>
            <span className="text-xs font-medium text-green-300">Command Executed</span>
          </div>

          {/* Command content */}
          <div className="bg-black/40 px-3 py-2">
            <code className="text-sm text-green-200 font-mono">
              {block.command}
            </code>
          </div>

          {/* Output (if present) */}
          {block.output && (
            <>
              <div className="bg-gray-900/50 px-3 py-1 border-t border-green-500/20">
                <span className="text-xs text-gray-400">Output:</span>
              </div>
              <div className="bg-black/60 px-3 py-2 max-h-64 overflow-auto">
                <pre className="text-xs text-gray-300 font-mono whitespace-pre-wrap">
                  {block.output}
                </pre>
              </div>
            </>
          )}
        </div>
      ))}
    </div>
  );
};
