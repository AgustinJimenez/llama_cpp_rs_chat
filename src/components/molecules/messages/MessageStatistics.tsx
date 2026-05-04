import { Gauge, Hash, Database, Terminal } from 'lucide-react';
import { useState, useEffect, useCallback } from 'react';

const PROCESS_POLL_INTERVAL_MS = 5000;

import type { TimingInfo } from '../../../utils/chatTransport';
import { getBackgroundProcesses } from '../../../utils/tauriCommands';
import type { BackgroundProcessInfo } from '../../../utils/tauriCommands';
import { BackgroundProcessesModal } from '../../organisms/BackgroundProcessesModal';

import { TokenBreakdownPopover } from './TokenBreakdownPopover';

interface MessageStatisticsProps {
  timings: TimingInfo;
  tokensUsed?: number;
  maxTokens?: number;
}

function formatNumber(n: number): string {
  return n.toLocaleString('en-US').replace(/,/g, '.');
}

const FINISH_REASON_BADGES: Record<string, { color: string; title: string; label: string }> = {
  length: {
    color: 'text-yellow-400',
    title: 'Generation was cut off by context limit',
    label: 'truncated',
  },
  yn_continue: {
    color: 'text-cyan-400',
    title: 'Y/N check detected incomplete task — auto-continuing',
    label: 'task incomplete',
  },
  tool_continue: {
    color: 'text-sky-400',
    title: 'Tool result was committed and generation resumed on a fresh context',
    label: 'continuing after tool',
  },
  error: {
    color: 'text-red-400',
    title: 'Generation stopped due to an error (repetition loop, stall, or decode failure)',
    label: 'stopped',
  },
  max_continues: {
    color: 'text-orange-400',
    title:
      'Maximum auto-continue attempts reached — the model could not complete the task within the context limit',
    label: 'max retries reached',
  },
  infinite_loop: {
    color: 'text-red-400',
    title: 'Model got stuck repeating the same tool call — generation force-stopped',
    label: 'infinite loop',
  },
};

const FinishReasonBadge: React.FC<{ finishReason?: string }> = ({ finishReason }) => {
  if (!finishReason) return null;
  const badge = FINISH_REASON_BADGES[finishReason];
  if (!badge) return null;
  return (
    <span className={`inline-flex items-center gap-1 ${badge.color}`} title={badge.title}>
      {badge.label}
    </span>
  );
};

export const MessageStatistics = ({ timings, tokensUsed, maxTokens }: MessageStatisticsProps) => {
  const { genTokPerSec, genTokens, promptTokens } = timings;
  const [bgProcesses, setBgProcesses] = useState<BackgroundProcessInfo[]>([]);
  const [modalOpen, setModalOpen] = useState(false);

  const refreshProcesses = useCallback(async () => {
    try {
      const procs = await getBackgroundProcesses();
      setBgProcesses(procs.filter((p) => p.alive));
    } catch {
      // silent
    }
  }, []);

  useEffect(() => {
    refreshProcesses();
    const id = setInterval(refreshProcesses, PROCESS_POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [refreshProcesses]);

  if (!genTokPerSec) return null;

  return (
    <div className="flex items-center gap-3 text-xs text-foreground font-mono">
      {genTokens ? (
        <span
          className="inline-flex items-center gap-1"
          title={
            promptTokens
              ? `Input: ${formatNumber(promptTokens)}, Output: ${formatNumber(genTokens)}`
              : 'Tokens generated'
          }
        >
          <Hash className="h-3 w-3" />
          {promptTokens
            ? `in: ${formatNumber(promptTokens)}  out: ${formatNumber(genTokens)}`
            : `${formatNumber(genTokens)} tokens`}
        </span>
      ) : null}
      {/* Elapsed time removed — shown by LoadingIndicator below the chat bubble */}
      <span className="inline-flex items-center gap-1" title="Generation speed">
        <Gauge className="h-3 w-3" />
        {genTokPerSec.toFixed(1)} tok/s
      </span>
      {tokensUsed !== undefined && maxTokens !== undefined && timings.tokenBreakdown ? (
        <TokenBreakdownPopover
          breakdown={timings.tokenBreakdown}
          tokensUsed={tokensUsed}
          maxTokens={maxTokens}
          formatNumber={formatNumber}
        />
      ) : null}
      {tokensUsed !== undefined && maxTokens !== undefined && !timings.tokenBreakdown ? (
        <span className="inline-flex items-center gap-1" title="Context usage">
          <Database className="h-3 w-3" />
          {formatNumber(tokensUsed)}/{formatNumber(maxTokens)}
        </span>
      ) : null}
      <FinishReasonBadge finishReason={timings.finishReason} />
      {bgProcesses.length > 0 ? (
        <>
          <button
            onClick={() => setModalOpen(true)}
            className="inline-flex items-center gap-1 text-emerald-400 hover:text-emerald-300 transition-colors cursor-pointer"
            title="Click to manage background processes"
          >
            <Terminal className="h-3 w-3" />
            {bgProcesses.length} {bgProcesses.length === 1 ? 'process' : 'processes'}
          </button>
          <BackgroundProcessesModal isOpen={modalOpen} onClose={() => setModalOpen(false)} />
        </>
      ) : null}
    </div>
  );
};
