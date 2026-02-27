import { Gauge, Hash, Clock, Database } from 'lucide-react';
import type { TimingInfo } from '../../../utils/chatTransport';

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
  const { genTokPerSec, genTokens, genEvalMs } = timings;

  if (!genTokPerSec) return null;

  return (
    <div className="flex items-center gap-3 mb-1 text-xs text-white font-mono">
      {genTokens ? (
        <span className="inline-flex items-center gap-1" title="Tokens generated">
          <Hash className="h-3 w-3" />
          {formatNumber(genTokens)} tokens
        </span>
      ) : null}
      {genEvalMs ? (
        <span className="inline-flex items-center gap-1" title="Generation time">
          <Clock className="h-3 w-3" />
          {formatDuration(genEvalMs)}
        </span>
      ) : null}
      <span className="inline-flex items-center gap-1" title="Generation speed">
        <Gauge className="h-3 w-3" />
        {genTokPerSec.toFixed(1)} tok/s
      </span>
      {tokensUsed !== undefined && maxTokens !== undefined ? (
        <span className="inline-flex items-center gap-1" title="Context usage">
          <Database className="h-3 w-3" />
          {formatNumber(tokensUsed)}/{formatNumber(maxTokens)}
        </span>
      ) : null}
    </div>
  );
}
