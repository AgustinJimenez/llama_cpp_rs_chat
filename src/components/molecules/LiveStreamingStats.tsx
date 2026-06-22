/* eslint-disable max-lines */
import { Database, Loader2, Square, PackageOpen, X } from 'lucide-react';
import { useState, useEffect, useRef, useCallback } from 'react';
import { toast } from 'react-hot-toast';
import { useTranslation } from 'react-i18next';

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
  const { t } = useTranslation();
  const [showModal, setShowModal] = useState(false);
  const fmt = (n: number) => n.toLocaleString('en-US').replace(/,/g, '.');
  return (
    <>
      <button
        type="button"
        onClick={() => setShowModal(true)}
        className={`inline-flex cursor-pointer items-center gap-1 transition-colors hover:text-foreground ${pct > CONTEXT_WARNING_THRESHOLD_PCT ? 'text-yellow-400' : ''}`}
        title={t('stats.tokenBreakdown')}
      >
        <Database className="size-3" />
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
  const { t } = useTranslation();
  const [polledStatus, setPolledStatus] = useState<string | undefined>(undefined);
  const [tokenStats, setTokenStats] = useState({ count: 0, tokPerSec: 0 });
  const liveTokPerSec = tokenStats.tokPerSec;
  const tokenCount = tokenStats.count;
  const elapsedRef = useRef(0);
  const startRef = useRef(Date.now());
  const firstTokensUsedRef = useRef<number | null>(null);
  const lastTokensRef = useRef<number>(0);
  const genTimeRef = useRef(0); // accumulated generation-only time (ms)
  const lastTickRef = useRef(Date.now());
  const pct = tokensUsed && maxTokens ? Math.round((tokensUsed / maxTokens) * 100) : 0;

  // eslint-disable-next-line react-doctor/no-cascading-set-state -- reset + periodic timer, separate concerns
  useEffect(() => {
    startRef.current = Date.now();
    lastTickRef.current = Date.now();
    genTimeRef.current = 0;
    setTokenStats({ count: 0, tokPerSec: 0 });
    firstTokensUsedRef.current = null;
    lastTokensRef.current = 0;
    const id = setInterval(() => {
      const now = Date.now();
      elapsedRef.current = now - startRef.current;
      const currentTokens = lastTokensRef.current;
      if (currentTokens > 0 && genTimeRef.current > 0) {
        setTokenStats((prev) => ({
          ...prev,
          tokPerSec: currentTokens / (genTimeRef.current / 1000),
        }));
      }
      lastTickRef.current = now;
    }, 1000);
    return () => clearInterval(id);
  }, []);

  // eslint-disable-next-line react-doctor/no-cascading-set-state -- single setTokenStats per token update
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
    setTokenStats((prev) => ({ ...prev, count: newCount }));
  }, [tokensUsed]);

  /* eslint-disable react-doctor/no-cascading-set-state, react-doctor/no-fetch-in-effect -- setPolledStatus in branches */
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
  /* eslint-enable react-doctor/no-cascading-set-state, react-doctor/no-fetch-in-effect */

  const rawStatus = streamStatus || polledStatus;
  // Compaction progress is shown by CompactButton — filter it out of the inline status line.
  const displayStatus = rawStatus?.includes('Compacting') ? undefined : rawStatus;
  const hasContext = tokensUsed !== undefined && maxTokens !== undefined;
  // Use generation-only tok/s (excludes tool execution time)
  const tokPerSec = liveTokPerSec > 0 ? liveTokPerSec.toFixed(1) : null;
  void elapsedRef; // elapsed time display moved to LoadingIndicator

  // Prompt-eval ("Evaluating…") is intentionally not shown here — the LoadingIndicator's
  // spinner + elapsed timer above the bubble already conveys that state. We only render
  // this line for an explicit worker status, generation progress, or the context counter.
  if (!hasContext && !displayStatus && tokenCount === 0) return null;
  return (
    <div className="flex items-center gap-3 font-mono text-xs text-muted-foreground">
      {!!displayStatus && (
        <span className="inline-flex items-center gap-1 text-cyan-400">
          <Loader2 className="size-3 animate-spin" />
          {displayStatus}
        </span>
      )}
      {tokenCount > 0 && (
        <span className="inline-flex items-center gap-1" title={t('stats.tokensGenerated')}>
          # {tokenCount.toLocaleString()}
        </span>
      )}
      {!!tokPerSec && (
        <span className="inline-flex items-center gap-1" title={t('stats.generationSpeed')}>
          {t('stats.tokPerSec', { speed: tokPerSec })}
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
  const { t } = useTranslation();
  const [isCompacting, setIsCompacting] = useState(false);
  const [compactState, setCompactState] = useState({
    elapsedSec: 0,
    polledProgress: null as string | null,
  });
  const startRef = useRef<number>(0);

  // Auto-compaction is happening when the generation stream carries a Compacting status.
  const isAutoCompacting = !!streamStatus?.includes('Compacting');
  const compacting = isCompacting || isAutoCompacting;

  // Related compaction display state — reset together
  // eslint-disable-next-line react-doctor/no-cascading-set-state, react-doctor/no-fetch-in-effect -- reset + interval, separate concerns
  useEffect(() => {
    if (!compacting) {
      setCompactState({ elapsedSec: 0, polledProgress: null });
      return;
    }
    startRef.current = Date.now();
    const id = setInterval(
      () =>
        setCompactState((s) => ({
          ...s,
          elapsedSec: Math.floor((Date.now() - startRef.current) / 1000),
        })),
      1000,
    );
    // Poll server progress only for manual compaction; auto uses streamStatus directly.
    if (!isAutoCompacting) {
      const pollId = setInterval(async () => {
        try {
          const resp = await fetch('/api/model/status');
          if (resp.ok) {
            const data = await resp.json();
            if (data.status_message)
              setCompactState((s) => ({ ...s, polledProgress: data.status_message }));
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
      toast.success(t('toast.conversationCompacted'), { duration: 2000 });
      window.dispatchEvent(new CustomEvent('conversation-compacted'));
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t('toast.compactionFailed'));
    } finally {
      setIsCompacting(false);
    }
  }, [conversationId, compacting, t]);

  // Extract progress % — from streamStatus for auto, polled status for manual.
  const pctSource = isAutoCompacting ? streamStatus : compactState.polledProgress;
  const pctMatch = pctSource?.match(/\((\d+)%\)/);
  const pct = pctMatch ? pctMatch[1] : null;

  const compactIcon = compacting ? (
    <Loader2 className="size-3 animate-spin" />
  ) : (
    <PackageOpen className="size-3" />
  );
  const compactLabel = compacting
    ? t('stats.compactingText', {
        pct: pct ? `${pct}%` : '…',
        elapsed: fmtElapsed(compactState.elapsedSec),
      })
    : t('stats.compact');
  return (
    <button
      type="button"
      onClick={handleCompact}
      disabled={compacting}
      className="flex items-center gap-1.5 rounded-full bg-muted px-3 py-1.5 text-xs font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
      title={t('stats.compactTitle', { pct: ctxPct })}
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
      {!!sub && <span className="ml-1 text-[10px] text-muted-foreground">{sub}</span>}
    </span>
  </div>
);

function barColor(pct: number) {
  if (pct > CTX_DANGER_PCT) return 'bg-red-500';
  if (pct > CTX_WARN_PCT) return 'bg-yellow-400';
  return 'bg-primary';
}

// eslint-disable-next-line complexity, max-lines-per-function
const TokenBreakdownModal = ({
  onClose,
  modelContextSize,
}: {
  onClose: () => void;
  modelContextSize: number;
}) => {
  const { t } = useTranslation();
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
        className="w-80 rounded-xl border border-border bg-background p-4 text-sm shadow-2xl"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.stopPropagation()}
      >
        <div className="mb-3 flex items-center justify-between">
          <span className="flex items-center gap-1.5 font-semibold">
            <Database className="size-3.5" />
            {t('stats.contextBreakdown')}
          </span>
          <button
            type="button"
            onClick={onClose}
            className="text-muted-foreground hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>

        <div className="mb-3 h-2 w-full overflow-hidden rounded-full bg-muted">
          <div
            className={`h-full rounded-full ${barColor(usedPct)}`}
            style={{ width: `${Math.min(usedPct, 100)}%` }}
          />
        </div>

        <div className="divide-y divide-border">
          <div className="space-y-0.5 pb-2">
            <BreakdownRow
              label={t('stats.systemPromptTokens')}
              value={fmt(systemPromptTokens)}
              sub={t('stats.tokensSub')}
            />
            <BreakdownRow
              label={t('stats.toolDefinitions')}
              value={fmt(toolTokens)}
              sub={t('stats.tokensSub')}
            />
            {summaryEst > 0 && (
              <BreakdownRow
                label={t('stats.compactionSummary')}
                value={`~${fmt(summaryEst)}`}
                sub={t('stats.estimatedSub')}
              />
            )}
            <BreakdownRow
              label={t('stats.messages')}
              value={`~${fmt(activeMsgEst)}`}
              sub={t('stats.estimatedSub')}
            />
            {lastTokenBreakdown?.tool_calls_and_results != null && (
              <BreakdownRow
                label={t('stats.toolOutput')}
                value={fmt(lastTokenBreakdown.tool_calls_and_results)}
                sub={t('stats.measuredSub')}
              />
            )}
            {lastTokenBreakdown?.tool_calls_and_results == null && (
              <BreakdownRow
                label={t('stats.toolOutput')}
                value={`~${fmt(activeToolEst)}`}
                sub={t('stats.estimatedSub')}
              />
            )}
            {lastTokenBreakdown?.tool_calls_and_results != null && activeToolEst > 0 && (
              <BreakdownRow
                label={t('stats.toolOutputRawEst')}
                value={`~${fmt(activeToolEst)}`}
                sub={`${Math.round((1 - lastTokenBreakdown.tool_calls_and_results / activeToolEst) * 100)}% RTK`}
              />
            )}
            {compactedEst > 0 && (
              <BreakdownRow
                label={t('stats.compactedHistory')}
                value={`~${fmt(compactedEst)}`}
                sub={t('stats.notInCtxSub')}
              />
            )}
          </div>
          <div className="space-y-0.5 py-2">
            {measuredTotal != null && (
              <BreakdownRow
                label={t('stats.lastMeasuredTotal')}
                value={fmt(measuredTotal)}
                sub={t('stats.actualSub')}
                highlight
              />
            )}
            {measuredTotal == null && (
              <BreakdownRow
                label={t('stats.estimatedTotal')}
                value={`~${fmt(estimatedTotal)}`}
                sub={t('stats.estimatedSub')}
                highlight
              />
            )}
            <BreakdownRow
              label={t('stats.freeSpace')}
              value={`~${fmtK(freeSpace)}`}
              sub={`${100 - usedPct}%`}
            />
            <BreakdownRow label={t('stats.contextWindow')} value={fmtK(modelContextSize)} />
          </div>
        </div>

        {measuredTotal != null && (
          <p className="mt-2 text-[10px] text-muted-foreground">{t('stats.measuredTotal')}</p>
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
  const { t } = useTranslation();
  const [showModal, setShowModal] = useState(false);
  const CONTEXT_HIGH_PCT = 70;
  const ctxSpanClass = ctxPct > CONTEXT_HIGH_PCT ? 'text-yellow-400' : '';
  return (
    <>
      <button
        type="button"
        onClick={() => setShowModal(true)}
        className="flex cursor-pointer items-center gap-1.5 font-mono text-xs text-muted-foreground transition-colors hover:text-foreground"
        title={t('stats.tokenBreakdown')}
      >
        <Database className="size-3" />
        <span className={ctxSpanClass}>
          {/* eslint-disable-next-line i18next/no-literal-string */}~
          {(estimatedConvTokens / 1000).toFixed(1)}K / {(modelContextSize / 1000).toFixed(1)}K
        </span>
        {ctxPct > CONTEXT_HIGH_PCT && (
          <span className="text-[10px] text-yellow-400">({ctxPct}%)</span>
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
  <div className="flex flex-1 flex-wrap items-center gap-3">
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
  const { t } = useTranslation();
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
    <div className="mb-1 flex items-center justify-between">
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
            className="flex items-center gap-1.5 rounded-full bg-muted px-3 py-1.5 text-xs font-medium text-foreground transition-colors hover:bg-accent"
            data-testid="stop-button"
            title={t('chat.stopGeneration')}
          >
            <Square className="size-3 fill-current" />
            {t('common.stop')}
          </button>
        )}
      </div>
    </div>
  );
};
