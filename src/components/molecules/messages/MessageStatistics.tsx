import { Gauge, Hash, Clock, Database } from 'lucide-react';
import type { TimingInfo } from '../../../utils/chatTransport';
import { TokenBreakdownPopover } from './TokenBreakdownPopover';

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
        <span className="inline-flex items-center gap-1 text-yellow-400" title="Generation was cut off by max_tokens limit">
          truncated
        </span>
      ) : null}
    </div>
  );
}
