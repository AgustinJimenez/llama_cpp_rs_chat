import { X, Zap, Play, RotateCcw } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';

import { useModelContext } from '../../contexts/ModelContext';

interface HeartbeatConfig {
  enabled: boolean;
  interval_minutes: number;
  prompt: string;
  last_fired_at: number;
  last_result: string | null;
  has_unread: boolean;
}

const DEFAULT_PROMPT =
  'You are running a background heartbeat check. Review the conversation so far ' +
  'and any ongoing tasks or items you were working on. If something needs ' +
  "the user's attention, report it concisely. " +
  'If nothing requires attention, respond with exactly: IDLE';

const DEFAULT_INTERVAL_MINUTES = 30;
const HEALTH_CHECK_TIMEOUT_MS = 3000;

interface Props {
  isOpen: boolean;
  onClose: () => void;
  conversationId: string | null;
}

// eslint-disable-next-line max-lines-per-function
export const HeartbeatModal = ({ isOpen, onClose, conversationId }: Props) => {
  const { status: modelStatus, activeProvider } = useModelContext();
  const modelLoaded = modelStatus.loaded || activeProvider !== 'local';

  const api = conversationId ? `/api/conversations/${conversationId}/heartbeat` : null;

  const [cfg, setCfg] = useState<HeartbeatConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [firing, setFiring] = useState(false);

  // Local edit state
  const [interval, setInterval] = useState(DEFAULT_INTERVAL_MINUTES);
  const [prompt, setPrompt] = useState('');

  const load = useCallback(async () => {
    if (!api) return;
    try {
      const res = await fetch(api);
      if (!res.ok) return;
      const data: HeartbeatConfig = await res.json();
      setCfg(data);
      setInterval(data.interval_minutes);
      setPrompt(data.prompt || DEFAULT_PROMPT);
      // Clear unread badge on open
      if (data.has_unread) {
        await fetch(`${api}/clear`, { method: 'POST' });
      }
    } catch (_) {
      // ignore
    }
  }, [api]);

  useEffect(() => {
    if (isOpen) {
      void load();
    }
  }, [isOpen, load]);

  const save = async (patch: Partial<HeartbeatConfig>) => {
    if (!api) return;
    setSaving(true);
    try {
      const res = await fetch(api, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(patch),
      });
      if (res.ok) {
        const data: HeartbeatConfig = await res.json();
        setCfg(data);
      }
    } finally {
      setSaving(false);
    }
  };

  const toggleEnabled = () => {
    if (!cfg) return;
    void save({ enabled: !cfg.enabled });
    setCfg({ ...cfg, enabled: !cfg.enabled });
  };

  const saveSettings = () => {
    void save({ interval_minutes: interval, prompt });
  };

  const fireNow = async () => {
    if (!api) return;
    setFiring(true);
    try {
      await fetch(`${api}/fire`, { method: 'POST' });
      // Poll for result after a short delay
      setTimeout(() => {
        void load();
      }, HEALTH_CHECK_TIMEOUT_MS);
    } finally {
      setFiring(false);
    }
  };

  const resetLastResult = () => {
    void save({
      last_fired_at: 0,
      last_result: null,
      has_unread: false,
    } as Partial<HeartbeatConfig>);
    if (cfg) setCfg({ ...cfg, last_fired_at: 0, last_result: null, has_unread: false });
  };

  const formatTime = (ts: number) => {
    if (!ts) return 'Never';
    return new Date(ts * 1000).toLocaleTimeString();
  };

  if (!isOpen) return null;

  const fireNowTitle = !modelLoaded ? 'Load a model or provider first' : undefined;

  return (
    <>
      <div
        className="fixed inset-0 z-40 bg-black/50"
        role="button"
        tabIndex={0}
        onClick={onClose}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') onClose();
        }}
      />
      <div className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center p-4">
        {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions */}
        <div
          className="pointer-events-auto w-full max-w-lg rounded-xl border border-border bg-card shadow-2xl"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div className="flex items-center justify-between border-b border-border px-5 py-4">
            <div className="flex items-center gap-2">
              <Zap className="h-4 w-4 text-yellow-500" />
              <h2 className="text-base font-semibold">Agent Heartbeat</h2>
              {!!cfg?.enabled && (
                <span className="rounded-full bg-green-500/20 px-2 py-0.5 text-xs text-green-400">
                  Active
                </span>
              )}
            </div>
            <button onClick={onClose} className="rounded-lg p-1.5 transition-colors hover:bg-muted">
              <X className="h-4 w-4" />
            </button>
          </div>

          {!conversationId && (
            <div className="p-8 text-center text-sm text-muted-foreground">
              Select a conversation to configure its heartbeat.
            </div>
          )}
          {!!conversationId && cfg === null && (
            <div className="p-8 text-center text-sm text-muted-foreground">Loading…</div>
          )}
          {!!conversationId && cfg !== null && (
            <div className="space-y-5 p-5">
              {/* Enable toggle */}
              <div className="flex items-center justify-between">
                <div>
                  <div className="text-sm font-medium">Enable heartbeat</div>
                  <div className="mt-0.5 text-xs text-muted-foreground">
                    Fires a message into this conversation on a timer
                  </div>
                </div>
                <button
                  onClick={toggleEnabled}
                  disabled={saving}
                  className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors focus:outline-none ${
                    cfg.enabled ? 'bg-green-500' : 'bg-muted'
                  }`}
                >
                  <span
                    className={`inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform ${
                      cfg.enabled ? 'translate-x-6' : 'translate-x-1'
                    }`}
                  />
                </button>
              </div>

              {/* Interval */}
              <div>
                <label htmlFor="heartbeat-interval" className="mb-1.5 block text-sm font-medium">
                  Interval (minutes)
                </label>
                <div className="flex items-center gap-2">
                  <input
                    id="heartbeat-interval"
                    type="number"
                    min={1}
                    max={120}
                    value={interval}
                    onChange={(e) =>
                      setInterval(Math.max(1, parseInt(e.target.value) || DEFAULT_INTERVAL_MINUTES))
                    }
                    className="w-24 rounded-lg border border-border bg-muted px-3 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-primary"
                  />
                  <span className="text-xs text-muted-foreground">
                    Fires every {interval} min when model is loaded and idle
                  </span>
                </div>
              </div>

              {/* Prompt */}
              <div>
                <label htmlFor="heartbeat-prompt" className="mb-1.5 block text-sm font-medium">
                  Heartbeat prompt
                </label>
                <textarea
                  id="heartbeat-prompt"
                  value={prompt}
                  onChange={(e) => setPrompt(e.target.value)}
                  rows={4}
                  className="w-full resize-none rounded-lg border border-border bg-muted px-3 py-2 font-mono text-sm focus:outline-none focus:ring-1 focus:ring-primary"
                />
                <p className="mt-1 text-xs text-muted-foreground">
                  Model responds <code className="rounded bg-muted px-1">IDLE</code> to stay silent,
                  or any other text to trigger a notification.
                </p>
              </div>

              {/* Save button */}
              <button
                onClick={saveSettings}
                disabled={saving}
                className="w-full rounded-lg bg-primary py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
              >
                {!!saving && 'Saving…'}
                {!saving && 'Save settings'}
              </button>

              {/* Status */}
              <div className="space-y-3 border-t border-border pt-4">
                <div className="flex items-center justify-between text-xs text-muted-foreground">
                  <span>Last fired: {formatTime(cfg.last_fired_at)}</span>
                  {!!cfg.last_result && (
                    <button
                      onClick={resetLastResult}
                      className="flex items-center gap-1 transition-colors hover:text-foreground"
                      title="Clear last result"
                    >
                      <RotateCcw className="h-3 w-3" />
                      Clear result
                    </button>
                  )}
                </div>

                {!!cfg.last_result && (
                  <div className="rounded-lg border border-border bg-muted/50 p-3 text-xs">
                    <div className="mb-1 font-medium text-muted-foreground">Last report:</div>
                    <div className="whitespace-pre-wrap">{cfg.last_result}</div>
                  </div>
                )}

                <button
                  onClick={fireNow}
                  disabled={firing || !modelLoaded}
                  title={fireNowTitle}
                  className="flex w-full items-center justify-center gap-2 rounded-lg border border-border py-2 text-sm transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <Play className="h-3.5 w-3.5" />
                  {!!firing && 'Firing…'}
                  {!firing && 'Fire now'}
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </>
  );
};
