import { Database, Loader2, Square } from 'lucide-react';
import { useState, useEffect, useRef } from 'react';

import type { TimingInfo } from '../../utils/chatTransport';

import { MessageStatistics } from './messages/MessageStatistics';

const STATUS_POLL_INTERVAL_MS = 2000;
const CONTEXT_WARNING_THRESHOLD_PCT = 90;

export const LiveStreamingStats = ({
  tokensUsed,
  maxTokens,
  streamStatus,
}: {
  tokensUsed?: number;
  maxTokens?: number;
  streamStatus?: string;
}) => {
  const [polledStatus, setPolledStatus] = useState<string | undefined>(undefined);
  const [elapsed, setElapsed] = useState(0);
  const [tokenCount, setTokenCount] = useState(0);
  const [liveTokPerSec, setLiveTokPerSec] = useState(0);
  const startRef = useRef(Date.now());
  const firstTokensUsedRef = useRef<number | null>(null);
  const lastTokensRef = useRef<number>(0);
  const genTimeRef = useRef(0); // accumulated generation-only time (ms)
  const lastTickRef = useRef(Date.now());
  const fmt = (n: number) => n.toLocaleString('en-US').replace(/,/g, '.');
  const pct = tokensUsed && maxTokens ? Math.round((tokensUsed / maxTokens) * 100) : 0;

  useEffect(() => {
    startRef.current = Date.now();
    lastTickRef.current = Date.now();
    genTimeRef.current = 0;
    setTokenCount(0);
    setLiveTokPerSec(0);
    firstTokensUsedRef.current = null;
    lastTokensRef.current = 0;
    const id = setInterval(() => {
      const now = Date.now();
      setElapsed(now - startRef.current);
      // Only count time as "generation time" if tokens changed since last tick
      const currentTokens = lastTokensRef.current;
      if (currentTokens > 0 && genTimeRef.current > 0) {
        setLiveTokPerSec(currentTokens / (genTimeRef.current / 1000));
      }
      lastTickRef.current = now;
    }, 1000);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    if (tokensUsed === undefined) return;
    if (firstTokensUsedRef.current === null) {
      firstTokensUsedRef.current = tokensUsed;
    }
    const newCount = tokensUsed - firstTokensUsedRef.current;
    // If token count increased, this tick was generation (not tool execution)
    if (newCount > lastTokensRef.current) {
      genTimeRef.current += Date.now() - lastTickRef.current;
      lastTickRef.current = Date.now();
    }
    lastTokensRef.current = newCount;
    setTokenCount(newCount);
  }, [tokensUsed]);

  useEffect(() => {
    if (streamStatus) {
      setPolledStatus(undefined);
      return;
    }
    const poll = async () => {
      try {
        const resp = await fetch('/api/model/status');
        if (resp.ok) {
          const data = await resp.json();
          setPolledStatus(data.status_message || undefined);
        }
      } catch {
        /* ignore */
      }
    };
    poll();
    const id = setInterval(poll, STATUS_POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [streamStatus]);

  const displayStatus = streamStatus || polledStatus;
  const hasContext = tokensUsed !== undefined && maxTokens !== undefined;
  // Use generation-only tok/s (excludes tool execution time)
  const tokPerSec = liveTokPerSec > 0 ? liveTokPerSec.toFixed(1) : null;
  const genSecs = Math.round(genTimeRef.current / 1000);
  const totalSecs = Math.floor(elapsed / 1000);

  if (!hasContext && !displayStatus) return null;
  return (
    <div className="flex items-center gap-3 text-xs text-muted-foreground font-mono">
      {displayStatus ? (
        <span className="inline-flex items-center gap-1 text-cyan-400">
          <Loader2 className="h-3 w-3 animate-spin" />
          {displayStatus}
        </span>
      ) : null}
      {tokenCount > 0 ? (
        <span className="inline-flex items-center gap-1" title="Tokens generated this turn">
          # {tokenCount.toLocaleString()}
        </span>
      ) : null}
      {tokPerSec ? (
        <span
          className="inline-flex items-center gap-1"
          title="Generation speed (excluding tool execution time)"
        >
          {tokPerSec} tok/s
        </span>
      ) : null}
      {totalSecs > 0 ? (
        <span
          className="inline-flex items-center gap-1"
          title={`Generation: ${genSecs}s, Total: ${totalSecs}s`}
        >
          {genSecs > 0 && genSecs < totalSecs ? `${genSecs}s / ${totalSecs}s` : `${totalSecs}s`}
        </span>
      ) : null}
      {hasContext ? (
        <span
          className={`inline-flex items-center gap-1 ${pct > CONTEXT_WARNING_THRESHOLD_PCT ? 'text-yellow-400' : ''}`}
          title={`Context: ${pct}% used`}
        >
          <Database className="h-3 w-3" />
          {fmt(tokensUsed ?? 0)}/{fmt(maxTokens ?? 0)}
        </span>
      ) : null}
    </div>
  );
};

export const StatsBar = ({
  timings,
  tokensUsed,
  maxTokens,
  streamStatus,
  disabled,
  isLoading,
  stopGeneration,
  estimatedConvTokens,
  modelContextSize,
}: {
  timings?: TimingInfo;
  tokensUsed?: number;
  maxTokens?: number;
  streamStatus?: string;
  disabled: boolean;
  isLoading: boolean;
  stopGeneration?: (() => void) | null;
  estimatedConvTokens?: number;
  modelContextSize?: number;
}) => {
  const isGenerating =
    timings?.genTokPerSec || disabled || (tokensUsed !== undefined && maxTokens !== undefined);
  const hasContextInfo = estimatedConvTokens && modelContextSize;
  if (!isGenerating && !hasContextInfo) return null;
  const ctxPct = hasContextInfo ? Math.round((estimatedConvTokens / modelContextSize) * 100) : 0;
  const CONTEXT_HIGH_PCT = 70;
  return (
    <div className="flex items-center justify-between mb-1">
      <div className="flex-1">
        {timings?.genTokPerSec ? (
          <MessageStatistics timings={timings} tokensUsed={tokensUsed} maxTokens={maxTokens} />
        ) : null}
        {!timings?.genTokPerSec && (tokensUsed !== undefined || isLoading || streamStatus) ? (
          <LiveStreamingStats
            tokensUsed={tokensUsed}
            maxTokens={maxTokens}
            streamStatus={streamStatus}
          />
        ) : null}
        {!isGenerating && hasContextInfo ? (
          <div className="flex items-center gap-2 text-xs text-muted-foreground font-mono">
            <Database className="h-3 w-3" />
            <span
              className={ctxPct > CONTEXT_HIGH_PCT ? 'text-yellow-400' : ''}
              title={`Estimated conversation: ~${estimatedConvTokens.toLocaleString()} tokens / ${modelContextSize.toLocaleString()} context`}
            >
              ~{(estimatedConvTokens / 1000).toFixed(1)}K / {(modelContextSize / 1000).toFixed(1)}K
            </span>
            {ctxPct > CONTEXT_HIGH_PCT ? (
              <span className="text-yellow-400 text-[10px]">({ctxPct}% used)</span>
            ) : null}
          </div>
        ) : null}
      </div>
      {disabled ? (
        <button
          type="button"
          onClick={stopGeneration ?? undefined}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-medium bg-muted hover:bg-accent text-foreground transition-colors"
          data-testid="stop-button"
          title="Stop generation"
        >
          <Square className="h-3 w-3 fill-current" />
          Stop
        </button>
      ) : null}
    </div>
  );
};
