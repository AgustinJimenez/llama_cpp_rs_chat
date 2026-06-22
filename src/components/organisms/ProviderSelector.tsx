/* eslint-disable max-lines */
import {
  X,
  Cpu,
  Cloud,
  Zap,
  Loader2,
  Search,
  ChevronDown,
  ChevronUp,
  Check,
  PlayCircle,
  RefreshCw,
} from 'lucide-react';
import React, { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';

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
  <button onClick={onToggle} className="group flex w-full items-center gap-2 py-1 pt-1 text-left">
    {icon}
    <span className="text-xs font-medium uppercase tracking-wider text-muted-foreground transition-colors group-hover:text-foreground">
      {label}
    </span>
    {!!open && <ChevronUp className="ml-auto size-3.5 text-muted-foreground" />}
    {!open && <ChevronDown className="ml-auto size-3.5 text-muted-foreground" />}
  </button>
);

/* eslint-disable max-lines-per-function, react-doctor/no-giant-component, react-doctor/prefer-useReducer -- single cohesive provider selection modal */
export const ProviderSelector = ({
  isOpen,
  onClose,
  onSelectLocal,
  onSelectRemote,
  currentProvider,
}: ProviderSelectorProps) => {
  const { t } = useTranslation();
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
        const [configured, config] = await Promise.all([
          invoke<{ providers?: Provider[] }>('list_configured_providers'),
          invoke<{ provider_api_keys?: string }>('get_config'),
        ]);
        if (!cancelled.v) {
          setProviders((current) => {
            const merged = new Map(current.map((p) => [p.id, p]));
            for (const p of configured.providers || []) merged.set(p.id, p);
            return Array.from(merged.values());
          });
          setApiKeyInputs(parseApiKeys(config.provider_api_keys));
        }
      } else {
        const [configuredRes, keysRes] = await Promise.all([
          fetch('/api/providers/configured'),
          fetch('/api/config/provider-keys'),
        ]);
        const [data, keysData] = await Promise.all([
          configuredRes.json(),
          keysRes.json().catch(() => ({})),
        ]);
        if (!cancelled.v) {
          setProviders((current) => {
            const merged = new Map(current.map((p) => [p.id, p]));
            for (const p of (data.providers || []) as Provider[]) merged.set(p.id, p);
            return Array.from(merged.values());
          });
          // Populate existing API key inputs (keys are masked but base_url is useful)
          const keyMap: ApiKeyMap = {};
          for (const [id, val] of Object.entries(
            keysData as Record<string, { api_key?: string; base_url?: string }>,
          )) {
            keyMap[id] = { api_key: '', base_url: val.base_url || '' };
          }
          setApiKeyInputs(keyMap);
        }
      }
    } catch {
      /* ignore */
    }
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
    } catch {
      /* ignore */
    }
  }, []);

  // Multiple setState calls for independent UI state — genuinely separate concerns
  // eslint-disable-next-line react-doctor/no-cascading-set-state -- separate concerns, same init trigger
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

    return () => {
      cancelled.v = true;
    };
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
        await invoke('save_config', {
          config: { ...config, provider_api_keys: JSON.stringify(keys) },
        });
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
      // eslint-disable-next-line @typescript-eslint/no-magic-numbers
      setTimeout(() => setSavedProvider(null), 2000);
    } catch {
      /* ignore */
    } finally {
      setSavingProvider(null);
    }
  };

  if (!isOpen) return null;

  const localProvider = providers.find((p) => p.id === 'local');
  const cliProviders = providers.filter((p) => CLI_PROVIDERS.includes(p.id));
  const searchLower = providerSearch.toLowerCase();
  const openaiProviders = providers
    .filter((p) => !NON_CLOUD.has(p.id) && p.name.toLowerCase().includes(searchLower))
    .sort((a, b) => a.name.localeCompare(b.name));

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      role="button"
      tabIndex={0}
      onClick={onClose}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') onClose();
      }}
    >
      {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions */}
      <div
        className="flex max-h-[85vh] w-[560px] max-w-[90vw] flex-col rounded-lg border border-border bg-card shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-border px-5 py-4">
          <h3 className="text-base font-medium text-foreground">{t('provider.title')}</h3>
          <button
            onClick={onClose}
            className="rounded p-1 text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>

        <div className="space-y-3 overflow-y-auto p-5">
          {/* \u2500\u2500 Local Model \u2500\u2500 */}
          <button
            onClick={onSelectLocal}
            className={`w-full rounded-lg border p-4 text-left transition-colors ${
              currentProvider === 'local'
                ? 'border-primary bg-primary/10'
                : 'border-border hover:border-primary hover:bg-muted/50'
            }`}
          >
            <div className="flex items-start gap-3">
              <Cpu className="mt-0.5 size-5 flex-shrink-0 text-emerald-400" />
              <div>
                <div className="font-medium text-foreground">
                  {localProvider?.name || t('provider.localModelName')}
                </div>
                <div className="mt-1 text-xs text-muted-foreground">
                  {localProvider?.description || t('provider.localModelDescription')}
                </div>
              </div>
            </div>
          </button>

          {/* \u2500\u2500 CLI Providers \u2500\u2500 */}
          <SectionHeader
            label={t('provider.cliProviders')}
            icon={<Cloud className="size-3.5 text-cyan-400" />}
            isOpen={cliSectionOpen}
            onToggle={() => setCliSectionOpen((v) => !v)}
          />

          {!!cliSectionOpen && (
            <div className="space-y-2">
              <div className="flex justify-end">
                <button
                  onClick={refreshCli}
                  disabled={refreshingCli || loadingCli}
                  title={t('provider.refreshCli')}
                  className="flex items-center gap-1.5 rounded px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-40"
                >
                  <RefreshCw className={`size-3 ${refreshingCli ? 'animate-spin' : ''}`} />
                  {t('common.refresh')}
                </button>
              </div>
              {!!loadingCli &&
                cliProviders.length === 0 &&
                [{ name: 'Claude Code' }, { name: 'Codex CLI' }, { name: 'Gemini CLI' }].map(
                  (p) => (
                    <div key={p.name} className="rounded-lg border border-border opacity-60">
                      <div className="flex items-center gap-3 p-4">
                        <Cloud className="size-5 flex-shrink-0 text-muted-foreground" />
                        <div className="flex items-center gap-2">
                          <span className="font-medium text-foreground">{p.name}</span>
                          <Loader2 className="size-3 animate-spin text-muted-foreground" />
                        </div>
                      </div>
                    </div>
                  ),
                )}
              {(!loadingCli || cliProviders.length > 0) &&
                cliProviders.map((provider) => {
                  const availabilityBadge = provider.available ? (
                    <span className="rounded bg-emerald-500/20 px-1.5 py-0.5 text-[10px] text-emerald-400">
                      {t('provider.connected')}
                    </span>
                  ) : (
                    <span className="rounded bg-red-500/20 px-1.5 py-0.5 text-[10px] text-red-400">
                      {t('provider.notDetected')}
                    </span>
                  );
                  const versionSuffix = provider.version
                    ? ` (v${provider.version.split(' ')[0]})`
                    : '';
                  let borderClass = provider.available
                    ? 'border-border'
                    : 'border-border opacity-60';
                  if (currentProvider === provider.id) borderClass = 'border-primary bg-primary/10';
                  // eslint-disable-next-line react-doctor/no-prevent-default -- SPA form submission
                  const handleCliFormSubmit = (e: React.FormEvent) => {
                    e.preventDefault();
                    const m = customModels[provider.id]?.trim();
                    if (m && provider.available) onSelectRemote(provider.id, m);
                  };
                  /* eslint-enable max-lines-per-function, react-doctor/no-giant-component, react-doctor/prefer-useReducer */

                  return (
                    <div
                      key={provider.id}
                      className={`rounded-lg border transition-colors ${borderClass}`}
                    >
                      <div className="p-4">
                        <div className="flex items-start gap-3">
                          <Cloud
                            className={`mt-0.5 size-5 flex-shrink-0 ${provider.available ? 'text-cyan-400' : 'text-muted-foreground'}`}
                          />
                          <div className="flex-1">
                            <div className="flex items-center gap-2">
                              <span className="font-medium text-foreground">{provider.name}</span>
                              {availabilityBadge}
                            </div>
                            <div className="mt-1 text-xs text-muted-foreground">
                              {provider.description}
                              {versionSuffix}
                            </div>
                          </div>
                        </div>
                      </div>
                      <div className="space-y-2 border-t border-border/50 px-4 py-3">
                        <div className="flex gap-2">
                          {(provider.models || ['default']).map((model) => (
                            <button
                              key={`${provider.id}:${model}`}
                              disabled={!provider.available}
                              onClick={() =>
                                provider.available && onSelectRemote(provider.id, model)
                              }
                              className={`flex-1 rounded-md border px-3 py-2 text-xs font-medium transition-colors ${
                                provider.available
                                  ? 'cursor-pointer border-border bg-muted text-foreground/80 hover:border-primary hover:bg-accent hover:text-foreground'
                                  : 'cursor-not-allowed border-border/40 bg-muted/50 text-muted-foreground/40'
                              }`}
                            >
                              {model.charAt(0).toUpperCase() + model.slice(1)}
                            </button>
                          ))}
                        </div>
                        <form onSubmit={handleCliFormSubmit} className="flex gap-2">
                          <input
                            type="text"
                            placeholder={t('provider.customModelPlaceholder')}
                            disabled={!provider.available}
                            value={customModels[provider.id] || ''}
                            onChange={(e) =>
                              setCustomModels((prev) => ({
                                ...prev,
                                [provider.id]: e.target.value,
                              }))
                            }
                            className={`flex-1 rounded-md border bg-muted px-3 py-2 font-mono text-xs transition-colors placeholder:text-muted-foreground/50 focus:border-primary focus:outline-none ${
                              provider.available
                                ? 'border-border text-foreground'
                                : 'cursor-not-allowed border-border/40 text-muted-foreground/40'
                            }`}
                          />
                          <button
                            type="submit"
                            disabled={!provider.available || !customModels[provider.id]?.trim()}
                            className="rounded-md border border-border bg-muted p-2 transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-30"
                          >
                            <PlayCircle className="size-4 text-emerald-400" />
                          </button>
                        </form>
                      </div>
                    </div>
                  );
                })}
            </div>
          )}

          {/* \u2500\u2500 OpenAI-Compatible Providers \u2500\u2500 */}
          <SectionHeader
            label={t('provider.openaiProviders')}
            icon={<Zap className="size-3.5 text-amber-400" />}
            isOpen={openaiSectionOpen}
            onToggle={() => setOpenaiSectionOpen((v) => !v)}
          />

          {!!openaiSectionOpen && (
            <div className="space-y-2">
              <div className="relative">
                <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                <input
                  type="text"
                  placeholder={t('provider.searchPlaceholder')}
                  value={providerSearch}
                  onChange={(e) => setProviderSearch(e.target.value)}
                  className="w-full rounded-md border border-border bg-muted py-1.5 pl-8 pr-3 text-xs text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none"
                />
              </div>

              {/* eslint-disable-next-line complexity, max-lines-per-function */}
              {openaiProviders.map((provider) => {
                const isExpanded = expandedProvider === provider.id;
                const input = apiKeyInputs[provider.id] || {};
                const isSaving = savingProvider === provider.id;
                const justSaved = savedProvider === provider.id;
                const apiKeyBadge = provider.available ? (
                  <span className="rounded bg-emerald-500/20 px-1.5 py-0.5 text-[10px] text-emerald-400">
                    {t('provider.apiKeySet')}
                  </span>
                ) : (
                  <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                    {t('provider.noApiKey')}
                  </span>
                );
                const chevronIcon = isExpanded ? (
                  <ChevronUp className="size-4" />
                ) : (
                  <ChevronDown className="size-4" />
                );
                let saveIcon = null;
                if (isSaving) saveIcon = <Loader2 className="size-3 animate-spin" />;
                else if (justSaved) saveIcon = <Check className="size-3" />;
                const useTitle = provider.available
                  ? t('provider.useProviderTitle', { provider: provider.name })
                  : t('provider.setApiKeyFirst');
                const saveLabel = justSaved ? t('common.saved') : t('common.save');

                let providerBorderClass = isExpanded ? 'border-border/80' : 'border-border';
                if (currentProvider === provider.id) {
                  providerBorderClass = 'border-primary bg-primary/10';
                }
                // eslint-disable-next-line react-doctor/no-prevent-default -- SPA form submission
                const handleOpenAiFormSubmit = (e: React.FormEvent) => {
                  e.preventDefault();
                  const m = customModels[provider.id]?.trim();
                  if (m) onSelectRemote(provider.id, m);
                };
                return (
                  <div
                    key={provider.id}
                    className={`rounded-lg border transition-colors ${providerBorderClass}`}
                  >
                    <div className="flex items-center">
                      <button
                        onClick={() => setExpandedProvider(isExpanded ? null : provider.id)}
                        className="flex-1 rounded-tl-lg p-4 text-left transition-colors hover:bg-muted/50"
                      >
                        <div className="flex items-center gap-3">
                          <Zap className="size-5 flex-shrink-0 text-amber-400" />
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-2">
                              <span className="font-medium text-foreground">{provider.name}</span>
                              {apiKeyBadge}
                            </div>
                            <div className="mt-0.5 text-xs text-muted-foreground">
                              {provider.description}
                            </div>
                          </div>
                        </div>
                      </button>

                      <div className="flex flex-shrink-0 items-center gap-1 pr-3">
                        <button
                          disabled={!provider.available}
                          onClick={() =>
                            provider.available &&
                            onSelectRemote(
                              provider.id,
                              selectedModels[provider.id] || provider.models?.[0] || 'default',
                            )
                          }
                          title={useTitle}
                          className={`rounded-md p-1.5 transition-colors ${
                            provider.available
                              ? 'text-emerald-400 hover:bg-emerald-400/10 hover:text-emerald-300'
                              : 'cursor-not-allowed text-muted-foreground/30'
                          }`}
                        >
                          <PlayCircle className="size-5" />
                        </button>
                        <button
                          onClick={() => setExpandedProvider(isExpanded ? null : provider.id)}
                          className="rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-muted/50"
                        >
                          {chevronIcon}
                        </button>
                      </div>
                    </div>

                    {!!isExpanded && (
                      <div className="space-y-3 border-t border-border/50 px-4 py-3">
                        <div className="space-y-1">
                          {/* eslint-disable-next-line jsx-a11y/label-has-associated-control */}
                          <label className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                            {t('provider.apiKeyLabel')}
                          </label>
                          <input
                            type="password"
                            placeholder={t('provider.apiKeyPlaceholder')}
                            value={input.api_key || ''}
                            onChange={(e) =>
                              setApiKeyInputs((prev) => ({
                                ...prev,
                                [provider.id]: { ...prev[provider.id], api_key: e.target.value },
                              }))
                            }
                            className="w-full rounded-md border border-border bg-muted px-3 py-1.5 font-mono text-xs text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none"
                          />
                        </div>
                        <div className="space-y-1">
                          {/* eslint-disable-next-line jsx-a11y/label-has-associated-control */}
                          <label className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                            {t('provider.baseUrlLabel')}{' '}
                            <span className="font-normal normal-case">
                              {t('provider.baseUrlOptional')}
                            </span>
                          </label>
                          <input
                            type="text"
                            placeholder={provider.default_base_url || 'https://api.example.com/v1'}
                            value={input.base_url || ''}
                            onChange={(e) =>
                              setApiKeyInputs((prev) => ({
                                ...prev,
                                [provider.id]: { ...prev[provider.id], base_url: e.target.value },
                              }))
                            }
                            className="w-full rounded-md border border-border bg-muted px-3 py-1.5 font-mono text-xs text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none"
                          />
                        </div>
                        <button
                          onClick={() => saveApiKey(provider.id)}
                          disabled={isSaving}
                          className="flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
                        >
                          {saveIcon}
                          {saveLabel}
                        </button>
                        {!!provider.available && (provider.models || []).length > 0 && (
                          <div className="border-t border-border/40 pt-1">
                            <p className="mb-2 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                              {t('provider.selectModel')}
                            </p>
                            <select
                              value={selectedModels[provider.id] || provider.models?.[0] || ''}
                              onChange={(e) =>
                                setSelectedModels((prev) => ({
                                  ...prev,
                                  [provider.id]: e.target.value,
                                }))
                              }
                              className="w-full rounded-md border border-border bg-muted px-3 py-1.5 font-mono text-xs text-foreground focus:border-primary focus:outline-none"
                            >
                              {(provider.models || []).map((model) => (
                                <option key={model} value={model}>
                                  {model}
                                </option>
                              ))}
                            </select>
                            <form onSubmit={handleOpenAiFormSubmit} className="mt-1 flex gap-2">
                              <input
                                type="text"
                                placeholder={t('provider.orTypeModelName')}
                                value={customModels[provider.id] || ''}
                                onChange={(e) =>
                                  setCustomModels((prev) => ({
                                    ...prev,
                                    [provider.id]: e.target.value,
                                  }))
                                }
                                className="flex-1 rounded-md border border-border bg-muted px-3 py-1.5 font-mono text-xs text-foreground placeholder:text-muted-foreground/50 focus:border-primary focus:outline-none"
                              />
                              <button
                                type="submit"
                                disabled={!customModels[provider.id]?.trim()}
                                className="rounded-md border border-border bg-muted p-1.5 transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-30"
                              >
                                <PlayCircle className="size-4 text-emerald-400" />
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
