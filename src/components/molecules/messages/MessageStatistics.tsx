import { useState, useEffect, useCallback } from 'react';
import { Gauge, Hash, Clock, Database, Terminal } from 'lucide-react';
import type { TimingInfo } from '../../../utils/chatTransport';
import { TokenBreakdownPopover } from './TokenBreakdownPopover';
import { getBackgroundProcesses } from '../../../utils/tauriCommands';
import type { BackgroundProcessInfo } from '../../../utils/tauriCommands';
import { BackgroundProcessesModal } from '../../organisms/BackgroundProcessesModal';

interface MessageStatisticsProps {
  timings: TimingInfo;
  tokensUsed?: number;
  maxTokens?: number;
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

function formatNumber(n: number): string {
  return n.toLocaleString('en-US').replace(/,/g, '.');
}

export function MessageStatistics({ timings, tokensUsed, maxTokens }: MessageStatisticsProps) {
  const { genTokPerSec, genTokens, genEvalMs, promptEvalMs } = timings;
  const [bgProcesses, setBgProcesses] = useState<BackgroundProcessInfo[]>([]);
  const [modalOpen, setModalOpen] = useState(false);

  const refreshProcesses = useCallback(async () => {
    try {
      const procs = await getBackgroundProcesses();
      setBgProcesses(procs.filter(p => p.alive));
    } catch {
      // silent
    }
  }, []);

  useEffect(() => {
    refreshProcesses();
    const id = setInterval(refreshProcesses, 5000);
    return () => clearInterval(id);
  }, [refreshProcesses]);

  if (!genTokPerSec) return null;

  const totalMs = (promptEvalMs || 0) + (genEvalMs || 0);

  return (
    <div className="flex items-center gap-3 text-xs text-white font-mono">
      {genTokens ? (
        <span className="inline-flex items-center gap-1" title="Tokens generated">
          <Hash className="h-3 w-3" />
          {formatNumber(genTokens)} tokens
        </span>
      ) : null}
      {totalMs ? (
        <span className="inline-flex items-center gap-1" title={`Total: ${formatDuration(totalMs)} (prompt: ${formatDuration(promptEvalMs || 0)}, gen: ${formatDuration(genEvalMs || 0)})`}>
          <Clock className="h-3 w-3" />
          {formatDuration(totalMs)}
        </span>
      ) : null}
      <span className="inline-flex items-center gap-1" title="Generation speed">
        <Gauge className="h-3 w-3" />
        {genTokPerSec.toFixed(1)} tok/s
      </span>
      {timings.costUsd ? (
        <span className="inline-flex items-center gap-1 text-emerald-400" title="Cost (from subscription)">
          ${timings.costUsd.toFixed(3)}
        </span>
      ) : null}
      {tokensUsed !== undefined && maxTokens !== undefined ? (
        timings.tokenBreakdown ? (
          <TokenBreakdownPopover
            breakdown={timings.tokenBreakdown}
            tokensUsed={tokensUsed}
            maxTokens={maxTokens}
            formatNumber={formatNumber}
          />
        ) : (
          <span className="inline-flex items-center gap-1" title="Context usage">
            <Database className="h-3 w-3" />
            {formatNumber(tokensUsed)}/{formatNumber(maxTokens)}
          </span>
        )
      ) : null}
      {timings.finishReason === 'length' ? (
        <span className="inline-flex items-center gap-1 text-yellow-400" title="Generation was cut off by context limit">
          truncated
        </span>
      ) : null}
      {timings.finishReason === 'yn_continue' ? (
        <span className="inline-flex items-center gap-1 text-cyan-400" title="Y/N check detected incomplete task — auto-continuing">
          task incomplete
        </span>
      ) : null}
      {timings.finishReason === 'error' ? (
        <span className="inline-flex items-center gap-1 text-red-400" title="Generation stopped due to an error (repetition loop, stall, or decode failure)">
          stopped
        </span>
      ) : null}
      {timings.finishReason === 'max_continues' ? (
        <span className="inline-flex items-center gap-1 text-orange-400" title="Maximum auto-continue attempts reached — the model could not complete the task within the context limit">
          max retries reached
        </span>
      ) : null}
      {timings.finishReason === 'infinite_loop' ? (
        <span className="inline-flex items-center gap-1 text-red-400" title="Model got stuck repeating the same tool call — generation force-stopped">
          infinite loop
        </span>
      ) : null}
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
}
