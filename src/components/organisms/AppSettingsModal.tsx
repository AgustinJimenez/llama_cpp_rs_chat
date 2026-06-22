/* eslint-disable max-lines -- single modal with provider config, splitting would fragment cohesive UI */
import { Settings } from 'lucide-react';
import React, { useState, useEffect } from 'react';
import { toast } from 'react-hot-toast';
import { useTranslation } from 'react-i18next';

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
  const { t } = useTranslation();
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
            <Settings className="size-5" />
            {t('appSettings.title')}
          </DialogTitle>
          <DialogDescription className="sr-only">{t('appSettings.title')}</DialogDescription>
        </DialogHeader>

        <div className="mb-4 flex border-b border-border">
          {TABS.map((tab) => (
            <button
              key={tab}
              type="button"
              onClick={() => setActiveTab(tab)}
              className={`border-b-2 px-4 py-2 text-sm font-medium transition-colors ${
                activeTab === tab
                  ? 'border-primary text-foreground'
                  : 'border-transparent text-muted-foreground hover:text-foreground'
              }`}
            >
              {tab === 'General' && t('appSettings.tabGeneral')}
              {tab === 'Notifications' && t('appSettings.tabNotifications')}
              {tab === 'MCP' && t('appSettings.tabMcp')}
              {tab === 'Errors' && t('appSettings.tabErrors')}
            </button>
          ))}
        </div>

        <div className="max-h-[60vh] overflow-y-auto py-2">
          {activeTab === 'General' && (
            <div className="space-y-4">
              {/* Theme Toggle */}
              <div className="space-y-2">
                <span className="text-sm font-medium text-foreground">
                  {t('appSettings.theme')}
                </span>
                <div className="flex gap-2">
                  {(['dark', 'light'] as const).map((tVal) => {
                    const themeLabel =
                      tVal === 'dark' ? t('appSettings.themeDark') : t('appSettings.themeLight');
                    return (
                      <button
                        key={tVal}
                        type="button"
                        onClick={() => {
                          const html = document.documentElement;
                          if (tVal === 'dark') {
                            html.classList.add('dark');
                          } else {
                            html.classList.remove('dark');
                          }
                          localStorage.setItem('theme', tVal);
                        }}
                        className={`rounded-lg border px-4 py-2 text-sm transition-colors ${
                          (typeof window !== 'undefined' &&
                            document.documentElement.classList.contains('dark')) ===
                          (tVal === 'dark')
                            ? 'border-primary bg-primary text-primary-foreground'
                            : 'border-border bg-muted text-muted-foreground hover:text-foreground'
                        }`}
                      >
                        {themeLabel}
                      </button>
                    );
                  })}
                </div>
              </div>

              {/* Max tool calls setting */}
              <div className="space-y-2">
                <label htmlFor="max-tool-calls" className="text-sm font-medium text-foreground">
                  {t('appSettings.maxToolCalls')}
                </label>
                <p className="text-xs text-muted-foreground">
                  {t('appSettings.maxToolCallsDescription')}
                </p>
                <input
                  id="max-tool-calls"
                  type="number"
                  min={1}
                  max={10000}
                  className="w-32 rounded-lg border border-border bg-muted px-3 py-2 text-sm text-foreground"
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
                  {t('appSettings.loopDetection')}
                </label>
                <p className="text-xs text-muted-foreground">
                  {t('appSettings.loopDetectionDescription')}
                </p>
                <input
                  id="loop-detection"
                  type="number"
                  min={3}
                  max={100}
                  className="w-32 rounded-lg border border-border bg-muted px-3 py-2 text-sm text-foreground"
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
                  {t('appSettings.telegramNotifications')}
                </label>
                <p className="text-xs text-muted-foreground">
                  {t('appSettings.telegramNotificationsDescription')}
                </p>
                <input
                  id="telegram-bot-token"
                  type="text"
                  placeholder={t('appSettings.telegramBotTokenPlaceholder')}
                  className="w-full rounded-lg border border-border bg-muted px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground"
                  value={localConfig?.telegram_bot_token || ''}
                  onChange={(e) =>
                    setLocalConfig((prev) =>
                      prev ? { ...prev, telegram_bot_token: e.target.value } : prev,
                    )
                  }
                />
                <input
                  type="text"
                  placeholder={t('appSettings.telegramChatIdPlaceholder')}
                  className="w-full rounded-lg border border-border bg-muted px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground"
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
                  <h3 className="text-sm font-medium text-foreground">
                    {t('appSettings.appErrors')}
                  </h3>
                  <p className="text-xs text-muted-foreground">
                    {t('appSettings.appErrorsDescription')}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={handleClearErrors}
                  className="whitespace-nowrap rounded-lg border border-border bg-muted px-3 py-1.5 text-xs text-foreground hover:bg-muted/80"
                >
                  {t('appSettings.clearErrors')}
                </button>
              </div>

              {!!errorsLoading && (
                <div className="text-sm text-muted-foreground">
                  {t('appSettings.loadingErrors')}
                </div>
              )}
              {!errorsLoading && appErrors.length === 0 && (
                <div className="text-sm text-muted-foreground">{t('appSettings.noErrors')}</div>
              )}
              {!errorsLoading && appErrors.length > 0 && (
                <div className="space-y-2">
                  {appErrors.map((error) => (
                    <div
                      key={error.id}
                      className="space-y-2 rounded-lg border border-border bg-muted/30 p-3"
                    >
                      <div className="flex items-start justify-between gap-3">
                        <div className="min-w-0">
                          <div className="text-xs text-muted-foreground">
                            {formatErrorTime(error.timestamp)}
                          </div>
                          <div className="break-words text-sm font-medium text-foreground">
                            {error.message}
                          </div>
                        </div>
                        <div className="shrink-0 rounded bg-red-500/15 px-2 py-1 text-[10px] uppercase tracking-wide text-red-400">
                          {error.level}
                        </div>
                      </div>
                      <div className="text-[11px] text-muted-foreground">
                        {t('appSettings.errorSource', { source: error.source })}
                      </div>
                      {!!error.details && (
                        <pre className="overflow-x-auto whitespace-pre-wrap break-words rounded bg-background/60 p-2 text-[11px] text-foreground/80">
                          {error.details}
                        </pre>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        <DialogFooter>
          <button className="flat-button bg-muted px-6 py-2" onClick={onClose}>
            {t('common.cancel')}
          </button>
          <button
            className="flat-button bg-primary px-6 py-2 text-primary-foreground"
            onClick={handleSave}
          >
            {t('common.save')}
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
