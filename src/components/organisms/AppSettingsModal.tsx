/* eslint-disable max-lines -- single modal with provider config, splitting would fragment cohesive UI */
import { Settings } from 'lucide-react';
import React, { useState, useEffect } from 'react';
import { toast } from 'react-hot-toast';

import { useSettings } from '../../hooks/useSettings';
import type { SamplerConfig } from '../../types';
import { clearAppErrors, getAppErrors, type AppErrorEntry } from '../../utils/tauriCommands';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogDescription,
  DialogTitle,
} from '../atoms/dialog';

import { McpSettingsSection } from './McpSettingsSection';

interface AppSettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
}

const TABS = ['General', 'Notifications', 'MCP', 'Errors'] as const;
type Tab = (typeof TABS)[number];

function formatErrorTime(timestamp: number) {
  return new Date(timestamp).toLocaleString();
}

// eslint-disable-next-line max-lines-per-function -- single modal component, splitting tabs would fragment readability
export const AppSettingsModal: React.FC<AppSettingsModalProps> = ({ isOpen, onClose }) => {
  const { config, updateConfig } = useSettings();
  const [localConfig, setLocalConfig] = useState<SamplerConfig | null>(null);
  const [activeTab, setActiveTab] = useState<Tab>('General');
  const [appErrors, setAppErrors] = useState<AppErrorEntry[]>([]);
  const [errorsLoading, setErrorsLoading] = useState(false);

  useEffect(() => {
    if (config) setLocalConfig(config);
  }, [config]);

  useEffect(() => {
    if (!isOpen || activeTab !== 'Errors') return;
    let cancelled = false;
    setErrorsLoading(true);
    const MAX_ERRORS = 150;
    getAppErrors(MAX_ERRORS)
      .then((errors) => {
        if (!cancelled) setAppErrors(errors);
      })
      .catch((err) => {
        if (!cancelled) {
          toast.error(err instanceof Error ? err.message : 'Failed to load app errors');
        }
      })
      .finally(() => {
        if (!cancelled) setErrorsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activeTab, isOpen]);

  const handleSave = async () => {
    if (localConfig) {
      await updateConfig(localConfig);
      toast.success('Settings saved');
      onClose();
    }
  };

  const handleClearErrors = async () => {
    try {
      await clearAppErrors();
      setAppErrors([]);
      toast.success('Error log cleared');
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to clear app errors');
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Settings className="h-5 w-5" />
            App Settings
          </DialogTitle>
          <DialogDescription className="sr-only">Application settings</DialogDescription>
        </DialogHeader>

        <div className="flex border-b border-border mb-4">
          {TABS.map((tab) => (
            <button
              key={tab}
              type="button"
              onClick={() => setActiveTab(tab)}
              className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
                activeTab === tab
                  ? 'border-primary text-foreground'
                  : 'border-transparent text-muted-foreground hover:text-foreground'
              }`}
            >
              {tab}
            </button>
          ))}
        </div>

        <div className="py-2 max-h-[60vh] overflow-y-auto">
          {activeTab === 'General' && (
            <div className="space-y-4">
              {/* Theme Toggle */}
              <div className="space-y-2">
                <span className="text-sm font-medium text-foreground">Theme</span>
                <div className="flex gap-2">
                  {(['dark', 'light'] as const).map((t) => (
                    <button
                      key={t}
                      type="button"
                      onClick={() => {
                        const html = document.documentElement;
                        if (t === 'dark') {
                          html.classList.add('dark');
                        } else {
                          html.classList.remove('dark');
                        }
                        localStorage.setItem('theme', t);
                      }}
                      className={`px-4 py-2 text-sm rounded-lg border transition-colors ${
                        (typeof window !== 'undefined' &&
                          document.documentElement.classList.contains('dark')) ===
                        (t === 'dark')
                          ? 'bg-primary text-primary-foreground border-primary'
                          : 'bg-muted border-border text-muted-foreground hover:text-foreground'
                      }`}
                    >
                      {t === 'dark' ? 'Dark' : 'Light'}
                    </button>
                  ))}
                </div>
              </div>

              {/* Max tool calls setting */}
              <div className="space-y-2">
                <label htmlFor="max-tool-calls" className="text-sm font-medium text-foreground">
                  Max tool calls per turn
                </label>
                <p className="text-xs text-muted-foreground">
                  Safety limit for agentic tool call loops. The model will stop after this many tool
                  calls and ask you to continue.
                </p>
                <input
                  id="max-tool-calls"
                  type="number"
                  min={1}
                  max={10000}
                  className="w-32 px-3 py-2 rounded-lg bg-muted border border-border text-sm text-foreground"
                  // eslint-disable-next-line @typescript-eslint/no-magic-numbers
                  value={localConfig?.max_tool_calls ?? 2000}
                  onChange={(e) =>
                    setLocalConfig((prev) =>
                      prev
                        ? {
                            ...prev, // eslint-disable-next-line @typescript-eslint/no-magic-numbers
                            max_tool_calls: parseInt(e.target.value, 10) || 2000,
                          }
                        : prev,
                    )
                  }
                />
              </div>

              {/* Loop detection limit */}
              <div className="space-y-2">
                <label htmlFor="loop-detection" className="text-sm font-medium text-foreground">
                  Loop detection limit
                </label>
                <p className="text-xs text-muted-foreground">
                  Stop if the same tool call is repeated this many times in a row. A warning is
                  injected at n-1 to give the model a chance to change approach.
                </p>
                <input
                  id="loop-detection"
                  type="number"
                  min={3}
                  max={100}
                  className="w-32 px-3 py-2 rounded-lg bg-muted border border-border text-sm text-foreground"
                  // eslint-disable-next-line @typescript-eslint/no-magic-numbers
                  value={localConfig?.loop_detection_limit ?? 15}
                  onChange={(e) =>
                    setLocalConfig((prev) =>
                      prev
                        ? {
                            ...prev, // eslint-disable-next-line @typescript-eslint/no-magic-numbers
                            loop_detection_limit: parseInt(e.target.value, 10) || 15,
                          }
                        : prev,
                    )
                  }
                />
              </div>

              {/* Web browsing uses the built-in Tauri WebView — no external
                  browser or API key configuration needed. */}
            </div>
          )}

          {activeTab === 'Notifications' && (
            <div className="space-y-4">
              {/* Telegram Notifications */}
              <div className="space-y-2">
                <label htmlFor="telegram-bot-token" className="text-sm font-medium text-foreground">
                  Telegram Notifications
                </label>
                <p className="text-xs text-muted-foreground">
                  Let the model send you Telegram messages (task completion, errors). Create a bot
                  via @BotFather.
                </p>
                <input
                  id="telegram-bot-token"
                  type="text"
                  placeholder="Bot Token (from @BotFather)"
                  className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground"
                  value={localConfig?.telegram_bot_token || ''}
                  onChange={(e) =>
                    setLocalConfig((prev) =>
                      prev ? { ...prev, telegram_bot_token: e.target.value } : prev,
                    )
                  }
                />
                <input
                  type="text"
                  placeholder="Chat ID (send /start to your bot, then check)"
                  className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground"
                  value={localConfig?.telegram_chat_id || ''}
                  onChange={(e) =>
                    setLocalConfig((prev) =>
                      prev ? { ...prev, telegram_chat_id: e.target.value } : prev,
                    )
                  }
                />
              </div>
            </div>
          )}

          {activeTab === 'MCP' && (
            <div className="space-y-4">
              <McpSettingsSection />
            </div>
          )}

          {activeTab === 'Errors' && (
            <div className="space-y-4">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <h3 className="text-sm font-medium text-foreground">App Errors</h3>
                  <p className="text-xs text-muted-foreground">
                    Persisted frontend runtime errors and unhandled rejections. Stored in SQLite so
                    they survive app restarts.
                  </p>
                </div>
                <button
                  type="button"
                  onClick={handleClearErrors}
                  className="text-xs px-3 py-1.5 rounded-lg bg-muted hover:bg-muted/80 text-foreground border border-border whitespace-nowrap"
                >
                  Clear Errors
                </button>
              </div>

              {/* eslint-disable-next-line no-nested-ternary */}
              {errorsLoading ? (
                <div className="text-sm text-muted-foreground">Loading errors...</div>
              ) : appErrors.length === 0 ? (
                <div className="text-sm text-muted-foreground">No app errors recorded.</div>
              ) : (
                <div className="space-y-2">
                  {appErrors.map((error) => (
                    <div
                      key={error.id}
                      className="rounded-lg border border-border bg-muted/30 p-3 space-y-2"
                    >
                      <div className="flex items-start justify-between gap-3">
                        <div className="min-w-0">
                          <div className="text-xs text-muted-foreground">
                            {formatErrorTime(error.timestamp)}
                          </div>
                          <div className="text-sm font-medium text-foreground break-words">
                            {error.message}
                          </div>
                        </div>
                        <div className="shrink-0 text-[10px] uppercase tracking-wide rounded px-2 py-1 bg-red-500/15 text-red-400">
                          {error.level}
                        </div>
                      </div>
                      <div className="text-[11px] text-muted-foreground">
                        Source: {error.source}
                      </div>
                      {error.details ? (
                        <pre className="whitespace-pre-wrap break-words text-[11px] text-foreground/80 bg-background/60 rounded p-2 overflow-x-auto">
                          {error.details}
                        </pre>
                      ) : null}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        <DialogFooter>
          <button className="flat-button bg-muted px-6 py-2" onClick={onClose}>
            Cancel
          </button>
          <button className="flat-button bg-primary text-white px-6 py-2" onClick={handleSave}>
            Save
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
