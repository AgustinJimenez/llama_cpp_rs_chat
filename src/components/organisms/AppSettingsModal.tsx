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

const CLOUD_PROVIDERS = [
  { id: 'groq', name: 'Groq', envHint: 'GROQ_API_KEY', consoleUrl: 'https://console.groq.com' },
  {
    id: 'gemini',
    name: 'Gemini',
    envHint: 'GEMINI_API_KEY',
    consoleUrl: 'https://aistudio.google.com',
  },
  {
    id: 'sambanova',
    name: 'SambaNova',
    envHint: 'SAMBANOVA_API_KEY',
    consoleUrl: 'https://cloud.sambanova.ai',
  },
  {
    id: 'cerebras',
    name: 'Cerebras',
    envHint: 'CEREBRAS_API_KEY',
    consoleUrl: 'https://cloud.cerebras.ai',
  },
  {
    id: 'openrouter',
    name: 'OpenRouter',
    envHint: 'OPENROUTER_API_KEY',
    consoleUrl: 'https://openrouter.ai/settings/keys',
  },
  {
    id: 'together',
    name: 'Together AI',
    envHint: 'TOGETHER_API_KEY',
    consoleUrl: 'https://api.together.ai/settings/api-keys',
  },
  {
    id: 'deepseek',
    name: 'DeepSeek',
    envHint: 'DEEPSEEK_API_KEY',
    consoleUrl: 'https://platform.deepseek.com',
  },
  {
    id: 'mistral',
    name: 'Mistral AI',
    envHint: 'MISTRAL_API_KEY',
    consoleUrl: 'https://console.mistral.ai',
  },
  {
    id: 'fireworks',
    name: 'Fireworks AI',
    envHint: 'FIREWORKS_API_KEY',
    consoleUrl: 'https://fireworks.ai',
  },
  { id: 'xai', name: 'xAI (Grok)', envHint: 'XAI_API_KEY', consoleUrl: 'https://console.x.ai' },
  {
    id: 'nvidia',
    name: 'NVIDIA NIM',
    envHint: 'NVIDIA_API_KEY',
    consoleUrl: 'https://build.nvidia.com',
  },
  {
    id: 'huggingface',
    name: 'Hugging Face',
    envHint: 'HF_TOKEN',
    consoleUrl: 'https://huggingface.co/settings/tokens',
  },
  {
    id: 'cloudflare',
    name: 'Cloudflare Workers AI',
    envHint: 'CLOUDFLARE_API_TOKEN',
    hasBaseUrl: true,
    consoleUrl: 'https://dash.cloudflare.com',
  },
];

// eslint-disable-next-line max-lines-per-function, complexity -- single cohesive form section, splitting would hurt readability
const ProviderApiKeysSection = ({
  providerApiKeys,
  onChange,
}: {
  providerApiKeys: string;
  onChange: (json: string) => void;
}) => {
  const [selectedProvider, setSelectedProvider] = useState('');

  let keys: Record<
    string,
    { api_key?: string; base_url?: string; name?: string; models?: string; custom?: boolean }
  > = {};
  try {
    const parsed = JSON.parse(providerApiKeys || '{}');
    for (const [k, v] of Object.entries(parsed)) {
      if (typeof v === 'string') {
        keys[k] = { api_key: v };
      } else if (typeof v === 'object' && v !== null) {
        keys[k] = v as {
          api_key?: string;
          base_url?: string;
          name?: string;
          models?: string;
          custom?: boolean;
        };
      }
    }
  } catch {
    keys = {};
  }

  const updateKey = (providerId: string, field: 'api_key' | 'base_url', value: string) => {
    const updated = { ...keys };
    if (!updated[providerId]) updated[providerId] = {};
    updated[providerId][field] = value;
    if (
      !updated[providerId].api_key &&
      !updated[providerId].base_url &&
      !updated[providerId].custom
    ) {
      delete updated[providerId];
    }
    onChange(JSON.stringify(updated));
  };

  // Custom providers management
  const customProviders = Object.entries(keys).filter(([, v]) => v.custom);

  const addCustomProvider = () => {
    const id = `custom_${Date.now()}`;
    const updated = {
      ...keys,
      [id]: { custom: true, name: '', base_url: '', api_key: '', models: '' },
    };
    onChange(JSON.stringify(updated));
    setSelectedProvider(id);
  };

  const updateCustom = (id: string, field: string, value: string) => {
    const updated = { ...keys };
    if (!updated[id]) updated[id] = { custom: true };
    (updated[id] as Record<string, unknown>)[field] = value;
    onChange(JSON.stringify(updated));
  };

  const removeCustom = (id: string) => {
    const updated = { ...keys };
    delete updated[id];
    onChange(JSON.stringify(updated));
    setSelectedProvider('');
  };

  const selected = CLOUD_PROVIDERS.find((p) => p.id === selectedProvider);
  const selectedCustom = customProviders.find(([id]) => id === selectedProvider);
  const configuredCount = CLOUD_PROVIDERS.filter((p) => keys[p.id]?.api_key).length;

  return (
    <div className="space-y-3">
      <div className="space-y-2">
        <label htmlFor="cloud-provider-select" className="text-sm font-medium text-foreground">
          Cloud Provider API Keys
        </label>
        <p className="text-xs text-muted-foreground">
          {configuredCount}/{CLOUD_PROVIDERS.length} providers configured. Keys stored locally.
        </p>

        {/* Provider selector dropdown */}
        <div className="flex gap-2">
          <select
            id="cloud-provider-select"
            value={selectedProvider}
            onChange={(e) => setSelectedProvider(e.target.value)}
            className="flex-1 px-3 py-1.5 rounded-lg bg-muted border border-border text-sm text-foreground"
          >
            <option value="">Select a provider to configure...</option>
            <optgroup label="Built-in Providers">
              {CLOUD_PROVIDERS.map(({ id, name }) => (
                <option key={id} value={id}>
                  {name} {keys[id]?.api_key ? ' ✓' : ''}
                </option>
              ))}
            </optgroup>
            {customProviders.length > 0 && (
              <optgroup label="Custom Providers">
                {customProviders.map(([id, data]) => (
                  <option key={id} value={id}>
                    {data.name || 'Unnamed'} {data.api_key ? ' ✓' : ''}
                  </option>
                ))}
              </optgroup>
            )}
          </select>
          <button
            type="button"
            onClick={addCustomProvider}
            className="text-xs px-3 py-1.5 rounded-lg bg-muted hover:bg-muted/80 text-foreground border border-border whitespace-nowrap"
          >
            + Custom
          </button>
        </div>

        {/* Configured providers badges */}
        {configuredCount > 0 && !selectedProvider && (
          <div className="flex flex-wrap gap-1.5">
            {CLOUD_PROVIDERS.filter((p) => keys[p.id]?.api_key).map(({ id, name }) => (
              <button
                key={id}
                type="button"
                onClick={() => setSelectedProvider(id)}
                className="text-[10px] px-2 py-0.5 rounded-full bg-emerald-500/15 text-emerald-400 hover:bg-emerald-500/25 transition-colors"
              >
                {name}
              </button>
            ))}
            {customProviders
              .filter(([, d]) => d.api_key || d.base_url)
              .map(([id, data]) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => setSelectedProvider(id)}
                  className="text-[10px] px-2 py-0.5 rounded-full bg-blue-500/15 text-blue-400 hover:bg-blue-500/25 transition-colors"
                >
                  {data.name || 'Custom'}
                </button>
              ))}
          </div>
        )}
      </div>

      {/* Selected built-in provider config */}
      {selected ? (
        <div className="space-y-2 p-3 rounded-lg bg-muted/30 border border-border">
          <div className="flex items-center justify-between">
            <span className="text-sm font-medium text-foreground">{selected.name}</span>
            {selected.consoleUrl ? (
              <a
                href={selected.consoleUrl}
                target="_blank"
                rel="noopener noreferrer"
                className="text-xs text-blue-400 hover:text-blue-300 hover:underline"
              >
                Get API key &rarr;
              </a>
            ) : null}
          </div>
          <input
            type="password"
            autoComplete="off"
            placeholder={selected.envHint ? `API Key (or set ${selected.envHint})` : 'API Key'}
            className="w-full px-3 py-1.5 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground"
            value={keys[selected.id]?.api_key || ''}
            onChange={(e) => updateKey(selected.id, 'api_key', e.target.value)}
          />
          {selected.hasBaseUrl ? (
            <input
              type="text"
              autoComplete="off"
              placeholder="Base URL (e.g. http://localhost:11434/v1)"
              className="w-full px-3 py-1.5 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground"
              value={keys[selected.id]?.base_url || ''}
              onChange={(e) => updateKey(selected.id, 'base_url', e.target.value)}
            />
          ) : null}
        </div>
      ) : null}

      {/* Selected custom provider config */}
      {selectedCustom ? (
        <div className="space-y-2 p-3 rounded-lg bg-muted/30 border border-border">
          <div className="flex items-center justify-between">
            <span className="text-sm font-medium text-foreground">Custom Provider</span>
            <button
              type="button"
              onClick={() => removeCustom(selectedCustom[0])}
              className="text-xs px-2 py-1 rounded text-red-400 hover:text-red-300 hover:bg-red-400/10"
            >
              Remove
            </button>
          </div>
          <input
            type="text"
            autoComplete="off"
            placeholder="Provider name (e.g. My Ollama)"
            className="w-full px-3 py-1.5 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground"
            value={selectedCustom[1].name || ''}
            onChange={(e) => updateCustom(selectedCustom[0], 'name', e.target.value)}
          />
          <input
            type="text"
            autoComplete="off"
            placeholder="Base URL (e.g. http://localhost:11434/v1)"
            className="w-full px-3 py-1.5 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground"
            value={selectedCustom[1].base_url || ''}
            onChange={(e) => updateCustom(selectedCustom[0], 'base_url', e.target.value)}
          />
          <input
            type="password"
            autoComplete="off"
            placeholder="API Key (optional for local servers)"
            className="w-full px-3 py-1.5 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground"
            value={selectedCustom[1].api_key || ''}
            onChange={(e) => updateCustom(selectedCustom[0], 'api_key', e.target.value)}
          />
          <input
            type="text"
            autoComplete="off"
            placeholder="Models (comma-separated, e.g. llama3,mistral)"
            className="w-full px-3 py-1.5 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground"
            value={selectedCustom[1].models || ''}
            onChange={(e) => updateCustom(selectedCustom[0], 'models', e.target.value)}
          />
        </div>
      ) : null}
    </div>
  );
};

interface AppSettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
}

const TABS = ['General', 'Providers', 'Notifications', 'MCP', 'Errors'] as const;
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

              {/* Web browsing uses the built-in Tauri WebView — no external
                  browser or API key configuration needed. */}
            </div>
          )}

          {activeTab === 'Providers' && (
            <div className="space-y-4">
              <ProviderApiKeysSection
                providerApiKeys={localConfig?.provider_api_keys || '{}'}
                onChange={(json) =>
                  setLocalConfig((prev) => (prev ? { ...prev, provider_api_keys: json } : prev))
                }
              />
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
