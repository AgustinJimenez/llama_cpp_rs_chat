import React, { useState, useEffect } from 'react';
import { Settings } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogDescription,
  DialogTitle,
} from '../atoms/dialog';
import { useSettings } from '../../hooks/useSettings';
import { McpSettingsSection } from './McpSettingsSection';
import { toast } from 'react-hot-toast';
import type { SamplerConfig } from '../../types';

const CLOUD_PROVIDERS = [
  { id: 'groq', name: 'Groq', envHint: 'GROQ_API_KEY' },
  { id: 'gemini', name: 'Gemini', envHint: 'GEMINI_API_KEY' },
  { id: 'sambanova', name: 'SambaNova', envHint: 'SAMBANOVA_API_KEY' },
  { id: 'cerebras', name: 'Cerebras', envHint: 'CEREBRAS_API_KEY' },
  { id: 'openrouter', name: 'OpenRouter', envHint: 'OPENROUTER_API_KEY' },
  { id: 'together', name: 'Together AI', envHint: 'TOGETHER_API_KEY' },
  { id: 'deepseek', name: 'DeepSeek', envHint: 'DEEPSEEK_API_KEY' },
  { id: 'custom_openai', name: 'Custom OpenAI', envHint: '', hasBaseUrl: true },
];

function ProviderApiKeysSection({ providerApiKeys, onChange }: { providerApiKeys: string; onChange: (json: string) => void }) {
  let keys: Record<string, { api_key?: string; base_url?: string }> = {};
  try {
    const parsed = JSON.parse(providerApiKeys || '{}');
    // Normalize: accept both {"groq": "key"} and {"groq": {"api_key": "key"}}
    for (const [k, v] of Object.entries(parsed)) {
      if (typeof v === 'string') {
        keys[k] = { api_key: v };
      } else if (typeof v === 'object' && v !== null) {
        keys[k] = v as { api_key?: string; base_url?: string };
      }
    }
  } catch {
    keys = {};
  }

  const updateKey = (providerId: string, field: 'api_key' | 'base_url', value: string) => {
    const updated = { ...keys };
    if (!updated[providerId]) updated[providerId] = {};
    updated[providerId][field] = value;
    // Clean empty entries
    if (!updated[providerId].api_key && !updated[providerId].base_url) {
      delete updated[providerId];
    }
    onChange(JSON.stringify(updated));
  };

  return (
    <div className="space-y-2">
      <label className="text-sm font-medium text-foreground">
        Cloud Provider API Keys
      </label>
      <p className="text-xs text-muted-foreground">
        Set API keys for OpenAI-compatible cloud providers. Keys are stored locally in your database.
        Alternatively, set environment variables (e.g. GROQ_API_KEY).
      </p>
      <div className="space-y-3">
        {CLOUD_PROVIDERS.map(({ id, name, envHint, hasBaseUrl }) => (
          <div key={id} className="space-y-1">
            <label className="text-xs font-medium text-muted-foreground">{name}</label>
            <input
              type="password"
              autoComplete="off"
              placeholder={envHint ? `API Key (or set ${envHint})` : 'API Key'}
              className="w-full px-3 py-1.5 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground"
              value={keys[id]?.api_key || ''}
              onChange={(e) => updateKey(id, 'api_key', e.target.value)}
            />
            {hasBaseUrl && (
              <input
                type="text"
                autoComplete="off"
                placeholder="Base URL (e.g. http://localhost:11434/v1)"
                className="w-full px-3 py-1.5 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground"
                value={keys[id]?.base_url || ''}
                onChange={(e) => updateKey(id, 'base_url', e.target.value)}
              />
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

interface AppSettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export const AppSettingsModal: React.FC<AppSettingsModalProps> = ({ isOpen, onClose }) => {
  const { config, updateConfig } = useSettings();
  const [localConfig, setLocalConfig] = useState<SamplerConfig | null>(null);

  useEffect(() => {
    if (config) setLocalConfig(config);
  }, [config]);

  const handleSave = async () => {
    if (localConfig) {
      await updateConfig(localConfig);
      toast.success('Settings saved');
      onClose();
    }
  };

  const provider = localConfig?.web_search_provider || 'DuckDuckGo';
  const apiKey = localConfig?.web_search_api_key || '';
  const browserBackend = localConfig?.web_browser_backend || 'chrome';

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Settings className="h-5 w-5" />
            App Settings
          </DialogTitle>
          <DialogDescription className="sr-only">
            Application settings
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2 max-h-[60vh] overflow-y-auto">
          {/* Web Search Provider */}
          <div className="space-y-2">
            <label htmlFor="web-search-provider" className="text-sm font-medium text-foreground">
              Web Search Provider
            </label>
            <p className="text-xs text-muted-foreground">
              Choose which search engine the model uses for web_search tool calls.
            </p>
            <select
              id="web-search-provider"
              className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm text-foreground"
              value={provider}
              onChange={(e) =>
                setLocalConfig(prev =>
                  prev ? { ...prev, web_search_provider: e.target.value } : prev
                )
              }
            >
              <option value="DuckDuckGo">DuckDuckGo (API + HTML scraping)</option>
              <option value="Brave">Brave (API key required)</option>
              <option value="Google">Google (via headless Chrome)</option>
            </select>
          </div>

          {provider === 'Brave' ? (
            <div className="space-y-2">
              <label htmlFor="brave-api-key" className="text-sm font-medium text-foreground">
                Brave API Key
              </label>
              <p className="text-xs text-muted-foreground">
                Stored in your local database and used only for Brave web_search.
              </p>
              <input
                id="brave-api-key"
                type="password"
                autoComplete="off"
                className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm text-foreground"
                value={apiKey}
                placeholder="BRAVE_SEARCH_API_KEY"
                onChange={(e) =>
                  setLocalConfig(prev =>
                    prev ? { ...prev, web_search_api_key: e.target.value } : prev
                  )
                }
              />
            </div>
          ) : null}

          {/* Browser Backend */}
          <div className="space-y-2">
            <label htmlFor="web-browser-backend" className="text-sm font-medium text-foreground">
              Browser Backend
            </label>
            <p className="text-xs text-muted-foreground">
              Browser engine used for web_fetch and Chrome-based web_search. Lighter backends use less RAM.
            </p>
            <select
              id="web-browser-backend"
              className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm text-foreground"
              value={browserBackend}
              onChange={(e) =>
                setLocalConfig(prev =>
                  prev ? { ...prev, web_browser_backend: e.target.value } : prev
                )
              }
            >
              <option value="chrome">Chrome (standard headless)</option>
              <option value="chrome-headless-shell">Chrome Headless Shell (lightweight)</option>
              <option value="agent-browser">Agent Browser (Playwright-based)</option>
              <option value="none">None (HTTP-only, no JS rendering)</option>
            </select>
          </div>

          {/* Separator */}
          <div className="border-t border-border my-2" />

          {/* Telegram Notifications */}
          <div className="space-y-2">
            <label className="text-sm font-medium text-foreground">
              Telegram Notifications
            </label>
            <p className="text-xs text-muted-foreground">
              Let the model send you Telegram messages (task completion, errors). Create a bot via @BotFather.
            </p>
            <input
              type="text"
              placeholder="Bot Token (from @BotFather)"
              className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground"
              value={localConfig?.telegram_bot_token || ''}
              onChange={(e) =>
                setLocalConfig(prev =>
                  prev ? { ...prev, telegram_bot_token: e.target.value } : prev
                )
              }
            />
            <input
              type="text"
              placeholder="Chat ID (send /start to your bot, then check)"
              className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm text-foreground placeholder:text-muted-foreground"
              value={localConfig?.telegram_chat_id || ''}
              onChange={(e) =>
                setLocalConfig(prev =>
                  prev ? { ...prev, telegram_chat_id: e.target.value } : prev
                )
              }
            />
          </div>

          {/* Separator */}
          <div className="border-t border-border my-2" />

          {/* Cloud Provider API Keys */}
          <ProviderApiKeysSection
            providerApiKeys={localConfig?.provider_api_keys || '{}'}
            onChange={(json) =>
              setLocalConfig(prev =>
                prev ? { ...prev, provider_api_keys: json } : prev
              )
            }
          />

          {/* Separator */}
          <div className="border-t border-border my-2" />

          {/* MCP Servers */}
          <McpSettingsSection />
        </div>

        <DialogFooter>
          <button className="flat-button bg-muted px-6 py-2" onClick={onClose}>
            Cancel
          </button>
          <button
            className="flat-button bg-primary text-white px-6 py-2"
            onClick={handleSave}
          >
            Save
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
