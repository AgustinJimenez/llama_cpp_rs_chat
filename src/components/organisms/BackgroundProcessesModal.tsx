import { Terminal, X, Trash2, ChevronDown, ChevronRight } from 'lucide-react';
import React, { useState, useEffect, useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';

const SECONDS_PER_HOUR = 3600;
const MODAL_POLL_INTERVAL_MS = 3000;
const OUTPUT_POLL_INTERVAL_MS = 2000;
const SCROLL_BOTTOM_THRESHOLD_PX = 40;

import { getBackgroundProcesses, killBackgroundProcess } from '../../utils/tauriCommands';
import type { BackgroundProcessInfo } from '../../utils/tauriCommands';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogDescription,
  DialogTitle,
} from '../atoms/dialog';

interface BackgroundProcessesModalProps {
  isOpen: boolean;
  onClose: () => void;
}

function formatElapsed(startedAt: number): string {
  const now = Date.now();
  const secs = Math.max(0, Math.floor((now - startedAt) / 1000));
  if (secs >= SECONDS_PER_HOUR) {
    const h = Math.floor(secs / SECONDS_PER_HOUR);
    const m = Math.floor((secs % SECONDS_PER_HOUR) / 60);
    return `${h}h ${m}m`;
  }
  if (secs >= 60) {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}m ${s}s`;
  }
  return `${secs}s`;
}

function truncateCommand(cmd: string, maxLen = 60): string {
  return cmd.length > maxLen ? `${cmd.slice(0, maxLen)}…` : cmd;
}

const ProcessOutputPanel = ({ pid, alive }: { pid: number; alive: boolean }) => {
  const { t } = useTranslation();
  const [lines, setLines] = useState<string[]>([]);
  const bottomRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);

  const fetchOutput = useCallback(async () => {
    try {
      const res = await fetch(`/api/system/processes/${pid}/output`);
      if (!res.ok) return;
      const data = (await res.json()) as { lines: string[] };
      setLines(data.lines ?? []);
    } catch {
      // silent
    }
  }, [pid]);

  useEffect(() => {
    fetchOutput();
    if (!alive) return;
    const id = setInterval(fetchOutput, OUTPUT_POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [fetchOutput, alive]);

  useEffect(() => {
    if (autoScrollRef.current) {
      bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
  }, [lines]);

  const handleScroll = (e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < SCROLL_BOTTOM_THRESHOLD_PX;
    autoScrollRef.current = atBottom;
  };

  if (lines.length === 0) {
    return (
      <div className="mt-2 rounded bg-black/40 px-3 py-2 text-xs text-muted-foreground">
        {t('backgroundProcesses.noOutputCaptured')}
      </div>
    );
  }

  return (
    <div
      className="mt-2 max-h-52 overflow-y-auto rounded bg-black/40 px-3 py-2 font-mono text-xs text-green-300"
      onScroll={handleScroll}
    >
      {/* eslint-disable react/no-array-index-key */}
      {lines.map((line, i) => (
        <div key={`line-${i}`} className="whitespace-pre-wrap break-all leading-relaxed">
          {line}
        </div>
      ))}
      {/* eslint-enable react/no-array-index-key */}
      <div ref={bottomRef} />
    </div>
  );
};

export const BackgroundProcessesModal: React.FC<BackgroundProcessesModalProps> = ({
  isOpen,
  onClose,
}) => {
  const { t } = useTranslation();
  const [processes, setProcesses] = useState<BackgroundProcessInfo[]>([]);
  const [killing, setKilling] = useState<Set<number>>(new Set());
  const [expanded, setExpanded] = useState<Set<number>>(new Set());

  const refresh = useCallback(async () => {
    try {
      const procs = await getBackgroundProcesses();
      setProcesses(procs);
    } catch {
      // silent
    }
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    refresh();
    const id = setInterval(refresh, MODAL_POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [isOpen, refresh]);

  const handleKill = async (pid: number) => {
    setKilling((prev) => new Set(prev).add(pid));
    try {
      await killBackgroundProcess(pid);
      await refresh();
    } catch {
      // silent
    } finally {
      setKilling((prev) => {
        const next = new Set(prev);
        next.delete(pid);
        return next;
      });
    }
  };

  const handleKillAll = async () => {
    const pids: number[] = [];
    for (const p of processes) {
      if (p.alive) pids.push(p.pid);
    }
    setKilling(new Set(pids));
    try {
      await Promise.all(pids.map((pid) => killBackgroundProcess(pid)));
      await refresh();
    } finally {
      setKilling(new Set());
    }
  };

  const toggleExpanded = (pid: number) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(pid)) next.delete(pid);
      else next.add(pid);
      return next;
    });
  };

  const aliveCount = processes.filter((p) => p.alive).length;

  const processList =
    processes.length > 0
      ? processes.map((proc) => {
          const isExpanded = expanded.has(proc.pid);
          const expandTitle = isExpanded
            ? t('backgroundProcesses.hideOutput')
            : t('backgroundProcesses.showOutput');
          const expandIcon = isExpanded ? (
            <ChevronDown className="size-3.5" />
          ) : (
            <ChevronRight className="size-3.5" />
          );
          const statusBadge = proc.alive ? (
            <span className="inline-flex items-center gap-1 text-green-400">
              <span className="size-1.5 animate-pulse rounded-full bg-green-400" />
              {t('backgroundProcesses.alive')}
            </span>
          ) : (
            <span className="text-red-400">{t('backgroundProcesses.exited')}</span>
          );
          const killButton = proc.alive ? (
            <button
              onClick={() => handleKill(proc.pid)}
              disabled={killing.has(proc.pid)}
              className="flex-shrink-0 rounded-md p-1.5 text-foreground transition-colors hover:bg-accent disabled:opacity-50"
              title={t('backgroundProcesses.killProcess')}
            >
              <X className="size-4" />
            </button>
          ) : null;
          const outputPanel = isExpanded ? (
            <ProcessOutputPanel pid={proc.pid} alive={proc.alive} />
          ) : null;
          return (
            <div key={proc.pid} className="rounded-lg bg-muted/50 px-3 py-2 text-sm">
              <div className="flex items-center gap-3">
                <button
                  onClick={() => toggleExpanded(proc.pid)}
                  className="flex-shrink-0 text-muted-foreground hover:text-foreground"
                  title={expandTitle}
                >
                  {expandIcon}
                </button>
                <div className="min-w-0 flex-1">
                  <div className="truncate font-mono text-xs" title={proc.command}>
                    {truncateCommand(proc.command)}
                  </div>
                  <div className="mt-0.5 flex items-center gap-2 text-xs text-muted-foreground">
                    <span>PID {proc.pid}</span>
                    <span>·</span>
                    <span>{formatElapsed(proc.startedAt)}</span>
                    {statusBadge}
                  </div>
                </div>
                {killButton}
              </div>
              {outputPanel}
            </div>
          );
        })
      : null;

  const killAllButton =
    aliveCount > 1 ? (
      <div className="flex justify-end pt-1">
        <button
          onClick={handleKillAll}
          className="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium text-destructive transition-colors hover:bg-destructive/10"
        >
          <Trash2 className="size-3.5" />
          {t('backgroundProcesses.killAll', { count: aliveCount })}
        </button>
      </div>
    ) : null;

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Terminal className="size-5" />
            {t('backgroundProcesses.title')}
          </DialogTitle>
          <DialogDescription className="sr-only">
            {t('backgroundProcesses.viewManage')}
          </DialogDescription>
        </DialogHeader>

        <div className="max-h-[70vh] space-y-2 overflow-y-auto py-2">
          {processes.length === 0 && (
            <p className="py-6 text-center text-sm text-muted-foreground">
              {t('backgroundProcesses.noProcessesRunning')}
            </p>
          )}
          {processList}
        </div>

        {killAllButton}
      </DialogContent>
    </Dialog>
  );
};
