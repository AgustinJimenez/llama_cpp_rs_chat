import { X, Cpu, Cloud, Zap, Loader2, Search, ChevronDown, ChevronUp, Check, PlayCircle, RefreshCw } from 'lucide-react';
import { useState, useEffect, useCallback } from 'react';

import { ProviderConfigSection } from '@/components/molecules/ProviderConfigSection';
import { isTauriEnv } from '@/utils/tauri';

interface Provider {
  id: string;
  name: string;
  available: boolean;
  description: string;
  version?: string;
  models?: string[];
  default_base_url?: string;
}

const CLI_PROVIDERS = ['claude_code', 'codex', 'gemini_cli'];
const NON_CLOUD = new Set(['local', 'claude_code', 'codex', 'gemini_cli']);

interface ProviderSelectorProps {
  isOpen: boolean;
  onClose: () => void;
  onSelectLocal: () => void;
  onSelectRemote: (provider: string, model: string) => void;
  currentProvider?: string;
}

type ApiKeyMap = Record<string, { api_key?: string; base_url?: string }>;

function parseApiKeys(raw: string | undefined | null): ApiKeyMap {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const result: ApiKeyMap = {};
    for (const [k, v] of Object.entries(parsed)) {
      if (typeof v === 'string') {
        result[k] = { api_key: v };
      } else if (v && typeof v === 'object') {
        const obj = v as Record<string, unknown>;
        result[k] = {
          api_key: typeof obj.api_key === 'string' ? obj.api_key : '',
          base_url: typeof obj.base_url === 'string' ? obj.base_url : '',
        };
      }
    }
    return result;
  } catch {
    return {};
  }
}

// eslint-disable-next-line max-lines-per-function -- single cohesive provider selection modal
export const ProviderSelector = ({
  isOpen,
  onClose,
  onSelectLocal,
  onSelectRemote,
  currentProvider,
}: ProviderSelectorProps) => {
  const [providers, setProviders] = useState<Provider[]>([]);
  const [loadingCli, setLoadingCli] = useState(false);
  const [refreshingCli, setRefreshingCli] = useState(false);
  const [providerSearch, setProviderSearch] = useState('');
  const [cliSectionOpen, setCliSectionOpen] = useState(false);
  const [openaiSectionOpen, setOpenaiSectionOpen] = useState(false);
  const [expandedProvider, setExpandedProvider] = useState<string | null>(null);
  const [customModels, setCustomModels] = useState<Record<string, string>>({});
  const [selectedModels, setSelectedModels] = useState<Record<string, string>>({});
  const [apiKeyInputs, setApiKeyInputs] = useState<ApiKeyMap>({});
  const [savingProvider, setSavingProvider] = useState<string | null>(null);
  const [savedProvider, setSavedProvider] = useState<string | null>(null);

  const fetchConfiguredProviders = useCallback(async (cancelled: { v: boolean }) => {
    try {
      if (isTauriEnv()) {
        const { invoke } = await import('@tauri-apps/api/core');
        const configured = await invoke<{ providers?: Provider[] }>('list_configured_providers');
        if (!cancelled.v) {
          setProviders((current) => {
            const merged = new Map(current.map((p) => [p.id, p]));
            for (const p of configured.providers || []) merged.set(p.id, p);
            return Array.from(merged.values());
          });
        }
        const config = await invoke<{ provider_api_keys?: string }>('get_config');
        if (!cancelled.v) setApiKeyInputs(parseApiKeys(config.provider_api_keys));
      } else {
        const [configuredRes, keysRes] = await Promise.all([
          fetch('/api/providers/configured'),
          fetch('/api/config/provider-keys'),
        ]);
        const data = await configuredRes.json();
        const keysData = await keysRes.json().catch(() => ({}));
        if (!cancelled.v) {
          setProviders((current) => {
            const merged = new Map(current.map((p) => [p.id, p]));
            for (const p of (data.providers || []) as Provider[]) merged.set(p.id, p);
            return Array.from(merged.values());
          });
          // Populate existing API key inputs (keys are masked but base_url is useful)
          const keyMap: ApiKeyMap = {};
          for (const [id, val] of Object.entries(keysData as Record<string, {api_key?: string; base_url?: string}>)) {
            keyMap[id] = { api_key: '', base_url: val.base_url || '' };
          }
          setApiKeyInputs(keyMap);
        }
      }
    } catch { /* ignore */ }
  }, []);

  const fetchCliProviders = useCallback(async (cancelled: { v: boolean }) => {
    try {
      if (isTauriEnv()) {
        const { invoke } = await import('@tauri-apps/api/core');
        const cli = await invoke<{ providers?: Provider[] }>('list_cli_providers');
        if (!cancelled.v) {
          setProviders((current) => {
            const merged = new Map(current.map((p) => [p.id, p]));
            for (const p of cli.providers || []) merged.set(p.id, p);
            // Replace old CLI entries with fresh data
            return Array.from(merged.values());
          });
        }
      } else {
        const r = await fetch('/api/providers/cli-status');
        const data = await r.json();
        if (!cancelled.v) {
          setProviders((current) => {
            const merged = new Map(current.map((p) => [p.id, p]));
            for (const p of (data.providers || []) as Provider[]) merged.set(p.id, p);
            return Array.from(merged.values());
          });
        }
      }
    } catch { /* ignore */ }
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    const cancelled = { v: false };
    setProviders([]);
    setLoadingCli(true);
    setProviderSearch('');
    setExpandedProvider(null);

    (async () => {
      await fetchConfiguredProviders(cancelled);
      await fetchCliProviders(cancelled);
      if (!cancelled.v) setLoadingCli(false);
    })();

    return () => { cancelled.v = true; };
  }, [isOpen, fetchConfiguredProviders, fetchCliProviders]);

  const refreshCli = async () => {
    setRefreshingCli(true);
    const cancelled = { v: false };
    await fetchCliProviders(cancelled);
    setRefreshingCli(false);
  };

  const saveApiKey = async (providerId: string) => {
    setSavingProvider(providerId);
    try {
      const input = apiKeyInputs[providerId] || {};
      if (isTauriEnv()) {
        const { invoke } = await import('@tauri-apps/api/core');
        const config = await invoke<Record<string, unknown>>('get_config');
        const keys = parseApiKeys(config.provider_api_keys as string | undefined);
        keys[providerId] = {
          ...keys[providerId],
          api_key: input.api_key || '',
          ...(input.base_url ? { base_url: input.base_url } : {}),
        };
        await invoke('save_config', { config: { ...config, provider_api_keys: JSON.stringify(keys) } });
        const configured = await invoke<{ providers?: Provider[] }>('list_configured_providers');
        setProviders((current) => {
          const merged = new Map(current.map((p) => [p.id, p]));
          for (const p of configured.providers || []) merged.set(p.id, p);
          return Array.from(merged.values());
        });
      } else {
        await fetch('/api/config/provider-keys', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            provider: providerId,
            api_key: input.api_key || '',
            ...(input.base_url ? { base_url: input.base_url } : {}),
          }),
        });
        // Refresh configured providers to update availability
        const r = await fetch('/api/providers/configured');
        const data = await r.json();
        setProviders((current) => {
          const merged = new Map(current.map((p) => [p.id, p]));
          for (const p of (data.providers || []) as Provider[]) merged.set(p.id, p);
          return Array.from(merged.values());
        });
      }

      setSavedProvider(providerId);
      setTimeout(() => setSavedProvider(null), 2000);
    } catch { /* ignore */ }
    finally { setSavingProvider(null); }
  };

  if (!isOpen) return null;

  const localProvider = providers.find((p) => p.id === 'local');
  const cliProviders = providers.filter((p) => CLI_PROVIDERS.includes(p.id));
  const openaiProviders = providers
    .filter((p) => !NON_CLOUD.has(p.id))
    .filter((p) => p.name.toLowerCase().includes(providerSearch.toLowerCase()))
    .sort((a, b) => a.name.localeCompare(b.name));

  const SectionHeader = ({
    label,
    icon,
    isOpen: open,
    onToggle,
  }: {
    label: string;
    icon: React.ReactNode;
    isOpen: boolean;
    onToggle: () => void;
  }) => (
    <button
      onClick={onToggle}
      className="flex items-center gap-2 w-full text-left py-1 pt-1 group"
    >
      {icon}
      <span className="text-xs font-medium text-muted-foreground uppercase tracking-wider group-hover:text-foreground transition-colors">
        {label}
      </span>
      {open
        ? <ChevronUp className="h-3.5 w-3.5 text-muted-foreground ml-auto" />
        : <ChevronDown className="h-3.5 w-3.5 text-muted-foreground ml-auto" />
      }
    </button>
  );

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      role="button"
      tabIndex={0}
      onClick={onClose}
      onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') onClose(); }}
    >
      {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions */}
      <div
        className="bg-card border border-border rounded-lg shadow-2xl w-[560px] max-w-[90vw] max-h-[85vh] flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 py-4 border-b border-border">
          <h3 className="text-base font-medium text-foreground">Select Provider</h3>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="p-5 space-y-3 overflow-y-auto">

          {/* ── Local Model ── */}
          <button
            onClick={onSelectLocal}
            className={`w-full text-left p-4 rounded-lg border transition-colors ${
              currentProvider === 'local'
                ? 'border-primary bg-primary/10'
                : 'border-border hover:border-primary hover:bg-muted/50'
            }`}
          >
            <div className="flex items-start gap-3">
              <Cpu className="h-5 w-5 text-emerald-400 mt-0.5 flex-shrink-0" />
              <div>
                <div className="font-medium text-foreground">
                  {localProvider?.name || 'Local Model (llama.cpp)'}
                </div>
                <div className="text-xs text-muted-foreground mt-1">
                  {localProvider?.description || 'Run models locally on your GPU'}
                </div>
              </div>
            </div>
          </button>

          {/* ── CLI Providers ── */}
          <SectionHeader
            label="CLI Providers"
            icon={<Cloud className="h-3.5 w-3.5 text-cyan-400" />}
            isOpen={cliSectionOpen}
            onToggle={() => setCliSectionOpen((v) => !v)}
          />

          {cliSectionOpen && (
            <div className="space-y-2">
              <div className="flex justify-end">
                <button
                  onClick={refreshCli}
                  disabled={refreshingCli || loadingCli}
                  title="Re-check CLI availability"
                  className="flex items-center gap-1.5 px-2 py-1 rounded text-xs text-muted-foreground hover:text-foreground hover:bg-muted transition-colors disabled:opacity-40"
                >
                  <RefreshCw className={`h-3 w-3 ${refreshingCli ? 'animate-spin' : ''}`} />
                  Refresh
                </button>
              </div>
              {(loadingCli && cliProviders.length === 0)
                ? [
                    { name: 'Claude Code' },
                    { name: 'Codex CLI' },
                    { name: 'Gemini CLI' },
                  ].map((p) => (
                    <div key={p.name} className="rounded-lg border border-border opacity-60">
                      <div className="p-4 flex items-center gap-3">
                        <Cloud className="h-5 w-5 text-muted-foreground flex-shrink-0" />
                        <div className="flex items-center gap-2">
                          <span className="font-medium text-foreground">{p.name}</span>
                          <Loader2 className="h-3 w-3 animate-spin text-muted-foreground" />
                        </div>
                      </div>
                    </div>
                  ))
                : cliProviders.map((provider) => (
                    <div
                      key={provider.id}
                      className={`rounded-lg border transition-colors ${
                        currentProvider === provider.id
                          ? 'border-primary bg-primary/10'
                          : provider.available
                            ? 'border-border'
                            : 'border-border opacity-60'
                      }`}
                    >
                      <div className="p-4">
                        <div className="flex items-start gap-3">
                          <Cloud className={`h-5 w-5 mt-0.5 flex-shrink-0 ${provider.available ? 'text-cyan-400' : 'text-muted-foreground'}`} />
                          <div className="flex-1">
                            <div className="flex items-center gap-2">
                              <span className="font-medium text-foreground">{provider.name}</span>
                              {provider.available ? (
                                <span className="text-[10px] px-1.5 py-0.5 rounded bg-emerald-500/20 text-emerald-400">connected</span>
                              ) : (
                                <span className="text-[10px] px-1.5 py-0.5 rounded bg-red-500/20 text-red-400">not detected</span>
                              )}
                            </div>
                            <div className="text-xs text-muted-foreground mt-1">
                              {provider.description}
                              {provider.version ? ` (v${provider.version.split(' ')[0]})` : ''}
                            </div>
                          </div>
                        </div>
                      </div>
                      <div className="border-t border-border/50 px-4 py-3 space-y-2">
                        <div className="flex gap-2">
                          {(provider.models || ['default']).map((model) => (
                            <button
                              key={`${provider.id}:${model}`}
                              disabled={!provider.available}
                              onClick={() => provider.available && onSelectRemote(provider.id, model)}
                              className={`flex-1 py-2 px-3 rounded-md text-xs font-medium border transition-colors ${
                                provider.available
                                  ? 'bg-muted hover:bg-accent text-foreground/80 hover:text-foreground border-border hover:border-primary cursor-pointer'
                                  : 'bg-muted/50 text-muted-foreground/40 border-border/40 cursor-not-allowed'
                              }`}
                            >
                              {model.charAt(0).toUpperCase() + model.slice(1)}
                            </button>
                          ))}
                        </div>
                        <form
                          onSubmit={(e) => {
                            e.preventDefault();
                            const m = customModels[provider.id]?.trim();
                            if (m && provider.available) onSelectRemote(provider.id, m);
                          }}
                          className="flex gap-2"
                        >
                          <input
                            type="text"
                            placeholder="Type model name…"
                            disabled={!provider.available}
                            value={customModels[provider.id] || ''}
                            onChange={(e) => setCustomModels((prev) => ({ ...prev, [provider.id]: e.target.value }))}
                            className={`flex-1 py-2 px-3 rounded-md text-xs border bg-muted font-mono placeholder:text-muted-foreground/50 focus:outline-none focus:border-primary transition-colors ${
                              provider.available
                                ? 'text-foreground border-border'
                                : 'text-muted-foreground/40 border-border/40 cursor-not-allowed'
                            }`}
                          />
                          <button
                            type="submit"
                            disabled={!provider.available || !customModels[provider.id]?.trim()}
                            className="p-2 rounded-md border border-border bg-muted hover:bg-accent transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
                          >
                            <PlayCircle className="h-4 w-4 text-emerald-400" />
                          </button>
                        </form>
                      </div>
                    </div>
                  ))
              }
            </div>
          )}

          {/* ── OpenAI-Compatible Providers ── */}
          <SectionHeader
            label="OpenAI-Compatible Providers"
            icon={<Zap className="h-3.5 w-3.5 text-amber-400" />}
            isOpen={openaiSectionOpen}
            onToggle={() => setOpenaiSectionOpen((v) => !v)}
          />

          {openaiSectionOpen && (
            <div className="space-y-2">
              <div className="relative">
                <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground pointer-events-none" />
                <input
                  type="text"
                  placeholder="Search providers..."
                  value={providerSearch}
                  onChange={(e) => setProviderSearch(e.target.value)}
                  className="w-full pl-8 pr-3 py-1.5 text-xs bg-muted border border-border rounded-md text-foreground placeholder:text-muted-foreground focus:outline-none focus:border-primary"
                />
              </div>

              {openaiProviders.map((provider) => {
                const isExpanded = expandedProvider === provider.id;
                const input = apiKeyInputs[provider.id] || {};
                const isSaving = savingProvider === provider.id;
                const justSaved = savedProvider === provider.id;

                return (
                  <div
                    key={provider.id}
                    className={`rounded-lg border transition-colors ${
                      currentProvider === provider.id
                        ? 'border-primary bg-primary/10'
                        : isExpanded ? 'border-border/80' : 'border-border'
                    }`}
                  >
                    <div className="flex items-center">
                      <button
                        onClick={() => setExpandedProvider(isExpanded ? null : provider.id)}
                        className="flex-1 text-left p-4 hover:bg-muted/50 transition-colors rounded-tl-lg"
                      >
                        <div className="flex items-center gap-3">
                          <Zap className="h-5 w-5 text-amber-400 flex-shrink-0" />
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2">
                              <span className="font-medium text-foreground">{provider.name}</span>
                              {provider.available ? (
                                <span className="text-[10px] px-1.5 py-0.5 rounded bg-emerald-500/20 text-emerald-400">API key set</span>
                              ) : (
                                <span className="text-[10px] px-1.5 py-0.5 rounded bg-muted text-muted-foreground">no API key</span>
                              )}
                            </div>
                            <div className="text-xs text-muted-foreground mt-0.5">{provider.description}</div>
                          </div>
                        </div>
                      </button>

                      <div className="flex items-center gap-1 pr-3 flex-shrink-0">
                        <button
                          disabled={!provider.available}
                          onClick={() => provider.available && onSelectRemote(provider.id, selectedModels[provider.id] || provider.models?.[0] || 'default')}
                          title={provider.available ? `Use ${provider.name}` : 'Set API key first'}
                          className={`p-1.5 rounded-md transition-colors ${
                            provider.available
                              ? 'text-emerald-400 hover:bg-emerald-400/10 hover:text-emerald-300'
                              : 'text-muted-foreground/30 cursor-not-allowed'
                          }`}
                        >
                          <PlayCircle className="h-5 w-5" />
                        </button>
                        <button
                          onClick={() => setExpandedProvider(isExpanded ? null : provider.id)}
                          className="p-1.5 rounded-md hover:bg-muted/50 transition-colors text-muted-foreground"
                        >
                          {isExpanded ? <ChevronUp className="h-4 w-4" /> : <ChevronDown className="h-4 w-4" />}
                        </button>
                      </div>
                    </div>

                    {isExpanded && (
                      <div className="border-t border-border/50 px-4 py-3 space-y-3">
                        <div className="space-y-1">
                          <label className="text-[10px] font-medium text-muted-foreground uppercase tracking-wider">API Key</label>
                          <input
                            type="password"
                            placeholder="sk-..."
                            value={input.api_key || ''}
                            onChange={(e) => setApiKeyInputs((prev) => ({ ...prev, [provider.id]: { ...prev[provider.id], api_key: e.target.value } }))}
                            className="w-full px-3 py-1.5 text-xs bg-muted border border-border rounded-md text-foreground placeholder:text-muted-foreground focus:outline-none focus:border-primary font-mono"
                          />
                        </div>
                        <div className="space-y-1">
                          <label className="text-[10px] font-medium text-muted-foreground uppercase tracking-wider">
                            Base URL <span className="normal-case font-normal">(optional override)</span>
                          </label>
                          <input
                            type="text"
                            placeholder={provider.default_base_url || 'https://api.example.com/v1'}
                            value={input.base_url || ''}
                            onChange={(e) => setApiKeyInputs((prev) => ({ ...prev, [provider.id]: { ...prev[provider.id], base_url: e.target.value } }))}
                            className="w-full px-3 py-1.5 text-xs bg-muted border border-border rounded-md text-foreground placeholder:text-muted-foreground focus:outline-none focus:border-primary font-mono"
                          />
                        </div>
                        <button
                          onClick={() => saveApiKey(provider.id)}
                          disabled={isSaving}
                          className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium bg-primary text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
                        >
                          {isSaving ? <Loader2 className="h-3 w-3 animate-spin" /> : justSaved ? <Check className="h-3 w-3" /> : null}
                          {justSaved ? 'Saved' : 'Save'}
                        </button>
                        {provider.available && (provider.models || []).length > 0 && (
                          <div className="pt-1 border-t border-border/40">
                            <p className="text-[10px] font-medium text-muted-foreground uppercase tracking-wider mb-2">Select Model</p>
                            <select
                              value={selectedModels[provider.id] || provider.models?.[0] || ''}
                              onChange={(e) => setSelectedModels((prev) => ({ ...prev, [provider.id]: e.target.value }))}
                              className="w-full px-3 py-1.5 text-xs bg-muted border border-border rounded-md text-foreground focus:outline-none focus:border-primary font-mono"
                            >
                              {(provider.models || []).map((model) => (
                                <option key={model} value={model}>{model}</option>
                              ))}
                            </select>
                            <form
                              onSubmit={(e) => {
                                e.preventDefault();
                                const m = customModels[provider.id]?.trim();
                                if (m) onSelectRemote(provider.id, m);
                              }}
                              className="flex gap-2 mt-1"
                            >
                              <input
                                type="text"
                                placeholder="Or type a model name..."
                                value={customModels[provider.id] || ''}
                                onChange={(e) => setCustomModels((prev) => ({ ...prev, [provider.id]: e.target.value }))}
                                className="flex-1 py-1.5 px-3 rounded-md text-xs border bg-muted font-mono placeholder:text-muted-foreground/50 focus:outline-none focus:border-primary text-foreground border-border"
                              />
                              <button
                                type="submit"
                                disabled={!customModels[provider.id]?.trim()}
                                className="p-1.5 rounded-md border border-border bg-muted hover:bg-accent transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
                              >
                                <PlayCircle className="h-4 w-4 text-emerald-400" />
                              </button>
                            </form>
                          </div>
                        )}
                        <ProviderConfigSection providerId={provider.id} />
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
