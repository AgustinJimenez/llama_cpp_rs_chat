/* eslint-disable max-lines */
import { Database, Loader2, Square, PackageOpen, X } from 'lucide-react';
import { useState, useEffect, useRef, useCallback } from 'react';
import { toast } from 'react-hot-toast';

import { useChatContext } from '../../contexts/ChatContext';
import { useModelContext } from '../../contexts/ModelContext';
import type { TimingInfo, TokenBreakdown } from '../../utils/chatTransport';
import { compactConversation } from '../../utils/tauriCommands';

import { MessageStatistics } from './messages/MessageStatistics';

const STATUS_POLL_INTERVAL_MS = 2000;
const CONTEXT_WARNING_THRESHOLD_PCT = 90;

const LiveTokenCounter = ({
  tokensUsed,
  maxTokens,
  pct,
}: {
  tokensUsed: number;
  maxTokens: number;
  pct: number;
}) => {
  const [showModal, setShowModal] = useState(false);
  const fmt = (n: number) => n.toLocaleString('en-US').replace(/,/g, '.');
  return (
    <>
      <button
        type="button"
        onClick={() => setShowModal(true)}
        className={`inline-flex items-center gap-1 cursor-pointer hover:text-foreground transition-colors ${pct > CONTEXT_WARNING_THRESHOLD_PCT ? 'text-yellow-400' : ''}`}
        title="Click for token breakdown"
      >
        <Database className="h-3 w-3" />
        {fmt(tokensUsed)}/{fmt(maxTokens)}
      </button>
      {!!showModal && (
        <TokenBreakdownModal onClose={() => setShowModal(false)} modelContextSize={maxTokens} />
      )}
    </>
  );
};

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
  // eslint-disable-next-line react-hooks/purity
  const startRef = useRef(Date.now());
  const firstTokensUsedRef = useRef<number | null>(null);
  const lastTokensRef = useRef<number>(0);
  const genTimeRef = useRef(0); // accumulated generation-only time (ms)
  // eslint-disable-next-line react-hooks/purity
  const lastTickRef = useRef(Date.now());
  const pct = tokensUsed && maxTokens ? Math.round((tokensUsed / maxTokens) * 100) : 0;

  useEffect(() => {
    startRef.current = Date.now();
    lastTickRef.current = Date.now();
    genTimeRef.current = 0;
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setTokenCount(0);
    // eslint-disable-next-line react-hooks/set-state-in-effect
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
      // eslint-disable-next-line react-hooks/set-state-in-effect
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

  const rawStatus = streamStatus || polledStatus;
  // Compaction progress is shown by CompactButton — filter it out of the inline status line.
  const displayStatus = rawStatus?.includes('Compacting') ? undefined : rawStatus;
  const hasContext = tokensUsed !== undefined && maxTokens !== undefined;
  // Use generation-only tok/s (excludes tool execution time)
  const tokPerSec = liveTokPerSec > 0 ? liveTokPerSec.toFixed(1) : null;
  void elapsed; // elapsed time display moved to LoadingIndicator

  // tokenCount === 0 means we're still in prompt eval (LiveStreamingStats only mounts while loading)
  const isPromptEval = tokenCount === 0 && !displayStatus;

  if (!hasContext && !displayStatus && !isPromptEval) return null;
  return (
    <div className="flex items-center gap-3 text-xs text-muted-foreground font-mono">
      {!!displayStatus && (
        <span className="inline-flex items-center gap-1 text-cyan-400">
          <Loader2 className="h-3 w-3 animate-spin" />
          {displayStatus}
        </span>
      )}
      {!displayStatus && !!isPromptEval && (
        <span className="inline-flex items-center gap-1 text-muted-foreground/60">
          <Loader2 className="h-3 w-3 animate-spin" />
          Evaluating…
        </span>
      )}
      {tokenCount > 0 && (
        <span className="inline-flex items-center gap-1" title="Tokens generated this turn">
          # {tokenCount.toLocaleString()}
        </span>
      )}
      {!!tokPerSec && (
        <span
          className="inline-flex items-center gap-1"
          title="Generation speed (excluding tool execution time)"
        >
          {tokPerSec} tok/s
        </span>
      )}
      {/* Elapsed time removed — shown by LoadingIndicator below the chat bubble */}
      {!!hasContext && (
        <LiveTokenCounter tokensUsed={tokensUsed ?? 0} maxTokens={maxTokens ?? 0} pct={pct} />
      )}
    </div>
  );
};

const CompactButton = ({
  ctxPct,
  conversationId,
  streamStatus,
}: {
  ctxPct: number;
  conversationId: string;
  streamStatus?: string;
}) => {
  const [isCompacting, setIsCompacting] = useState(false);
  const [elapsedSec, setElapsedSec] = useState(0);
  const [polledProgress, setPolledProgress] = useState<string | null>(null);
  const startRef = useRef<number>(0);

  // Auto-compaction is happening when the generation stream carries a Compacting status.
  const isAutoCompacting = !!streamStatus?.includes('Compacting');
  const compacting = isCompacting || isAutoCompacting;

  useEffect(() => {
    if (!compacting) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setElapsedSec(0);
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setPolledProgress(null);
      return;
    }
    startRef.current = Date.now();
    const id = setInterval(
      () => setElapsedSec(Math.floor((Date.now() - startRef.current) / 1000)),
      1000,
    );
    // Poll server progress only for manual compaction; auto uses streamStatus directly.
    if (!isAutoCompacting) {
      const pollId = setInterval(async () => {
        try {
          const resp = await fetch('/api/model/status');
          if (resp.ok) {
            const data = await resp.json();
            if (data.status_message) setPolledProgress(data.status_message);
          }
        } catch {
          /* ignore */
        }
      }, 1000);
      return () => {
        clearInterval(id);
        clearInterval(pollId);
      };
    }
    return () => clearInterval(id);
  }, [compacting, isAutoCompacting]);

  const fmtElapsed = (s: number) =>
    s < 60 ? `${s}s` : `${Math.floor(s / 60)}m${s % 60 < 10 ? '0' : ''}${s % 60}s`;

  const handleCompact = useCallback(async () => {
    if (compacting) return;
    setIsCompacting(true);
    try {
      window.dispatchEvent(new CustomEvent('conversation-compacting'));
      await compactConversation(conversationId);
      toast.success('Conversation compacted', { duration: 2000 });
      window.dispatchEvent(new CustomEvent('conversation-compacted'));
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Compaction failed');
    } finally {
      setIsCompacting(false);
    }
  }, [conversationId, compacting]);

  // Extract progress % — from streamStatus for auto, polled status for manual.
  const pctSource = isAutoCompacting ? streamStatus : polledProgress;
  const pctMatch = pctSource?.match(/\((\d+)%\)/);
  const pct = pctMatch ? pctMatch[1] : null;

  const compactIcon = compacting ? (
    <Loader2 className="h-3 w-3 animate-spin" />
  ) : (
    <PackageOpen className="h-3 w-3" />
  );
  const compactLabel = compacting
    ? `Compacting${pct ? ` ${pct}%` : '…'} ${fmtElapsed(elapsedSec)}`
    : 'Compact';
  return (
    <button
      type="button"
      onClick={handleCompact}
      disabled={compacting}
      className="flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-medium bg-muted hover:bg-accent text-foreground transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
      title={`Summarize old messages to free context (${ctxPct}% used)`}
    >
      {compactIcon}
      {compactLabel}
    </button>
  );
};

const CHARS_PER_TOKEN = 4;

function extractToolResponseChars(content: string): number {
  let count = 0;
  const re1 = /<tool_response>[\s\S]*?<\/tool_response>/g;
  const re2 = /\[TOOL_RESULTS\][\s\S]*?\[\/TOOL_RESULTS\]/g;
  let m: RegExpExecArray | null;
  // biome-ignore lint/suspicious/noAssignInExpressions: intentional loop pattern
  while ((m = re1.exec(content)) !== null) count += m[0].length;
  // biome-ignore lint/suspicious/noAssignInExpressions: intentional loop pattern
  while ((m = re2.exec(content)) !== null) count += m[0].length;
  return count;
}
const fmt = (n: number) => n.toLocaleString('en-US');
const fmtK = (n: number) => `${(n / 1000).toFixed(1)}K`;
const CTX_DANGER_PCT = 90;
const CTX_WARN_PCT = 70;

const BreakdownRow = ({
  label,
  value,
  sub,
  highlight,
}: {
  label: string;
  value: string;
  sub?: string;
  highlight?: boolean;
}) => (
  <div className={`flex items-baseline justify-between py-1 ${highlight ? 'font-semibold' : ''}`}>
    <span className="text-muted-foreground">{label}</span>
    <span className="font-mono tabular-nums">
      {value}
      {!!sub && <span className="text-muted-foreground text-[10px] ml-1">{sub}</span>}
    </span>
  </div>
);

function barColor(pct: number) {
  if (pct > CTX_DANGER_PCT) return 'bg-red-500';
  if (pct > CTX_WARN_PCT) return 'bg-yellow-400';
  return 'bg-primary';
}

// eslint-disable-next-line complexity
const TokenBreakdownModal = ({
  onClose,
  modelContextSize,
}: {
  onClose: () => void;
  modelContextSize: number;
}) => {
  const { messages } = useChatContext();
  const { status } = useModelContext();

  const systemPromptTokens = status.system_prompt_tokens ?? 0;
  const toolTokens = status.tool_definitions_tokens ?? 0;

  let summaryChars = 0;
  let activeMsgChars = 0;
  let activeToolChars = 0;
  let compactedChars = 0;
  let lastPromptTokens: number | null = null;
  let lastGenTokens: number | null = null;
  let lastTokenBreakdown: TokenBreakdown | null = null;

  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (lastPromptTokens === null && m.timings?.promptTokens && m.timings?.genTokens) {
      lastPromptTokens = m.timings.promptTokens;
      lastGenTokens = m.timings.genTokens;
      lastTokenBreakdown = m.timings.tokenBreakdown ?? null;
      break;
    }
  }

  for (const m of messages) {
    const chars = m.content?.length ?? 0;
    if (m.role === 'system' && m.content?.startsWith('[Conversation summary')) {
      summaryChars += chars;
    } else if (m.compacted) {
      compactedChars += chars;
    } else if (m.role !== 'system') {
      const toolChars = extractToolResponseChars(m.content ?? '');
      activeToolChars += toolChars;
      activeMsgChars += chars - toolChars;
    }
  }

  const summaryEst = Math.round(summaryChars / CHARS_PER_TOKEN);
  const activeMsgEst = Math.round(activeMsgChars / CHARS_PER_TOKEN);
  const activeToolEst = Math.round(activeToolChars / CHARS_PER_TOKEN);
  const compactedEst = Math.round(compactedChars / CHARS_PER_TOKEN);
  const measuredTotal =
    lastPromptTokens != null && lastGenTokens != null ? lastPromptTokens + lastGenTokens : null;
  const estimatedTotal =
    systemPromptTokens + toolTokens + summaryEst + activeMsgEst + activeToolEst;
  const displayTotal = measuredTotal ?? estimatedTotal;
  const freeSpace = modelContextSize - displayTotal;
  const usedPct = Math.round((displayTotal / modelContextSize) * 100);

  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
      onClick={onClose}
      onKeyDown={(e) => e.key === 'Escape' && onClose()}
    >
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions */}
      <div
        role="document"
        className="bg-background border border-border rounded-xl shadow-2xl w-80 p-4 text-sm"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-3">
          <span className="font-semibold flex items-center gap-1.5">
            <Database className="h-3.5 w-3.5" />
            Context breakdown
          </span>
          <button
            type="button"
            onClick={onClose}
            className="text-muted-foreground hover:text-foreground"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="w-full h-2 rounded-full bg-muted mb-3 overflow-hidden">
          <div
            className={`h-full rounded-full ${barColor(usedPct)}`}
            style={{ width: `${Math.min(usedPct, 100)}%` }}
          />
        </div>

        <div className="divide-y divide-border">
          <div className="pb-2 space-y-0.5">
            <BreakdownRow label="System prompt" value={fmt(systemPromptTokens)} sub="tokens" />
            <BreakdownRow label="Tool definitions" value={fmt(toolTokens)} sub="tokens" />
            {summaryEst > 0 && (
              <BreakdownRow label="Compaction summary" value={`~${fmt(summaryEst)}`} sub="est." />
            )}
            <BreakdownRow label="Messages" value={`~${fmt(activeMsgEst)}`} sub="est." />
            {lastTokenBreakdown?.tool_calls_and_results != null && (
              <BreakdownRow
                label="Tool output"
                value={fmt(lastTokenBreakdown.tool_calls_and_results)}
                sub="measured"
              />
            )}
            {lastTokenBreakdown?.tool_calls_and_results == null && (
              <BreakdownRow label="Tool output" value={`~${fmt(activeToolEst)}`} sub="est." />
            )}
            {lastTokenBreakdown?.tool_calls_and_results != null && activeToolEst > 0 && (
              <BreakdownRow
                label="Tool output (raw est.)"
                value={`~${fmt(activeToolEst)}`}
                sub={`${Math.round((1 - lastTokenBreakdown.tool_calls_and_results / activeToolEst) * 100)}% RTK`}
              />
            )}
            {compactedEst > 0 && (
              <BreakdownRow
                label="Compacted history"
                value={`~${fmt(compactedEst)}`}
                sub="not in ctx"
              />
            )}
          </div>
          <div className="py-2 space-y-0.5">
            {measuredTotal != null && (
              <BreakdownRow
                label="Last measured total"
                value={fmt(measuredTotal)}
                sub="actual"
                highlight
              />
            )}
            {measuredTotal == null && (
              <BreakdownRow
                label="Estimated total"
                value={`~${fmt(estimatedTotal)}`}
                sub="est."
                highlight
              />
            )}
            <BreakdownRow
              label="Free space"
              value={`~${fmtK(freeSpace)}`}
              sub={`${100 - usedPct}%`}
            />
            <BreakdownRow label="Context window" value={fmtK(modelContextSize)} />
          </div>
        </div>

        {measuredTotal != null && (
          <p className="text-[10px] text-muted-foreground mt-2">
            Measured total = prompt + response tokens from last generation.
          </p>
        )}
      </div>
    </div>
  );
};

const ContextUsageInfo = ({
  estimatedConvTokens,
  modelContextSize,
  ctxPct,
}: {
  estimatedConvTokens: number;
  modelContextSize: number;
  ctxPct: number;
}) => {
  const [showModal, setShowModal] = useState(false);
  const CONTEXT_HIGH_PCT = 70;
  const ctxSpanClass = ctxPct > CONTEXT_HIGH_PCT ? 'text-yellow-400' : '';
  return (
    <>
      <button
        type="button"
        onClick={() => setShowModal(true)}
        className="flex items-center gap-1.5 text-xs text-muted-foreground font-mono hover:text-foreground transition-colors cursor-pointer"
        title="Click for token breakdown"
      >
        <Database className="h-3 w-3" />
        <span className={ctxSpanClass}>
          ~{(estimatedConvTokens / 1000).toFixed(1)}K / {(modelContextSize / 1000).toFixed(1)}K
        </span>
        {ctxPct > CONTEXT_HIGH_PCT && (
          <span className="text-yellow-400 text-[10px]">({ctxPct}%)</span>
        )}
      </button>
      {!!showModal && (
        <TokenBreakdownModal
          onClose={() => setShowModal(false)}
          modelContextSize={modelContextSize}
        />
      )}
    </>
  );
};

const StatsLeft = ({
  timings,
  tokensUsed,
  maxTokens,
  streamStatus,
  isLoading,
  estimatedConvTokens,
  modelContextSize,
  ctxPct,
}: {
  timings?: TimingInfo;
  tokensUsed?: number;
  maxTokens?: number;
  streamStatus?: string;
  isLoading: boolean;
  estimatedConvTokens?: number;
  modelContextSize?: number;
  ctxPct: number;
}) => (
  <div className="flex-1 flex items-center gap-3 flex-wrap">
    {!!timings?.genTokPerSec && !isLoading && <MessageStatistics timings={timings} />}
    {!!isLoading && (
      <LiveStreamingStats
        tokensUsed={tokensUsed}
        maxTokens={maxTokens}
        streamStatus={streamStatus}
      />
    )}
    {!isLoading && !!estimatedConvTokens && !!modelContextSize && (
      <ContextUsageInfo
        estimatedConvTokens={estimatedConvTokens}
        modelContextSize={modelContextSize}
        ctxPct={ctxPct}
      />
    )}
  </div>
);

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
  const { currentConversationId } = useChatContext();

  const isGenerating =
    timings?.genTokPerSec || disabled || (tokensUsed !== undefined && maxTokens !== undefined);
  const hasContextInfo = estimatedConvTokens && modelContextSize;
  if (!isGenerating && !hasContextInfo) return null;
  const ctxPct = hasContextInfo ? Math.round((estimatedConvTokens / modelContextSize) * 100) : 0;
  const COMPACT_THRESHOLD_PCT = 50;
  const isAutoCompacting = !!streamStatus?.includes('Compacting');
  // Show compact button when idle + context is full, OR when auto-compaction is active mid-gen.
  const showCompact =
    ((!isLoading && hasContextInfo && ctxPct >= COMPACT_THRESHOLD_PCT) || isAutoCompacting) &&
    !!currentConversationId;
  const statsLeftEstTokens = hasContextInfo ? estimatedConvTokens : undefined;
  const statsLeftCtxSize = hasContextInfo ? modelContextSize : undefined;

  return (
    <div className="flex items-center justify-between mb-1">
      <StatsLeft
        timings={timings}
        tokensUsed={tokensUsed}
        maxTokens={maxTokens}
        streamStatus={streamStatus}
        isLoading={isLoading}
        estimatedConvTokens={statsLeftEstTokens}
        modelContextSize={statsLeftCtxSize}
        ctxPct={ctxPct}
      />
      <div className="flex items-center gap-2">
        {!!showCompact && (
          <CompactButton
            ctxPct={ctxPct}
            conversationId={currentConversationId}
            streamStatus={streamStatus}
          />
        )}
        {!!isLoading && !!stopGeneration && (
          <button
            type="button"
            onClick={stopGeneration}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-medium bg-muted hover:bg-accent text-foreground transition-colors"
            data-testid="stop-button"
            title="Stop generation"
          >
            <Square className="h-3 w-3 fill-current" />
            Stop
          </button>
        )}
      </div>
    </div>
  );
};
