import { X, Cpu, Cloud, Zap, Loader2 } from 'lucide-react';
import { useState, useEffect } from 'react';

import { isTauriEnv } from '@/utils/tauri';

interface Provider {
  id: string;
  name: string;
  available: boolean;
  description: string;
  version?: string;
  models?: string[];
}

// CLI-backed providers (need local CLI installed)
const CLI_PROVIDERS = ['claude_code', 'codex'];
// Non-cloud provider IDs (everything else is OpenAI-compatible / custom)
const NON_CLOUD = new Set(['local', 'claude_code', 'codex']);

interface ProviderSelectorProps {
  isOpen: boolean;
  onClose: () => void;
  onSelectLocal: () => void;
  onSelectRemote: (provider: string, model: string) => void;
  currentProvider?: string;
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
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!isOpen) return;
    setLoading(true);
    (async () => {
      try {
        let data;
        if (isTauriEnv()) {
          const { invoke } = await import('@tauri-apps/api/core');
          data = await invoke('list_providers');
        } else {
          const r = await fetch('/api/providers');
          data = await r.json();
        }
        setProviders(data.providers || []);
      } catch {
        // intentionally empty — providers will remain empty
      }
      setLoading(false);
    })();
  }, [isOpen]);

  if (!isOpen) return null;

  const localProvider = providers.find((p) => p.id === 'local');
  const cliProviders = providers.filter((p) => CLI_PROVIDERS.includes(p.id));
  const openaiProviders = providers.filter((p) => !NON_CLOUD.has(p.id));

  const cliInstallHints: Record<string, string> = {
    claude_code: 'npm i -g @anthropic-ai/claude-code',
    codex: 'npm i -g @openai/codex',
  };

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
      {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions -- inner div only prevents propagation */}
      <div
        className="bg-card border border-border rounded-lg shadow-2xl w-[560px] max-w-[90vw] max-h-[85vh] flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 py-4 border-b border-border">
          <h3 className="text-base font-medium text-foreground">Choose Provider</h3>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="p-5 space-y-3 overflow-y-auto">
          {loading ? (
            <div className="flex items-center justify-center py-8">
              <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
            </div>
          ) : (
            <>
              {/* Local Model */}
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

              {/* CLI-backed providers (Claude Code, Codex) */}
              {cliProviders.map((provider) => (
                <div
                  key={provider.id}
                  className={`rounded-lg border ${
                    currentProvider === provider.id
                      ? 'border-primary bg-primary/10'
                      : 'border-border'
                  }`}
                >
                  <div className="p-4">
                    <div className="flex items-start gap-3">
                      <Cloud className="h-5 w-5 text-cyan-400 mt-0.5 flex-shrink-0" />
                      <div className="flex-1">
                        <div className="flex items-center gap-2">
                          <span className="font-medium text-foreground">{provider.name}</span>
                          {provider.available ? (
                            <span className="text-[10px] px-1.5 py-0.5 rounded bg-emerald-500/20 text-emerald-400">
                              connected
                            </span>
                          ) : (
                            <span className="text-[10px] px-1.5 py-0.5 rounded bg-red-500/20 text-red-400">
                              not installed
                            </span>
                          )}
                        </div>
                        <div className="text-xs text-muted-foreground mt-1">
                          {provider.description}
                          {provider.version ? ` (v${provider.version.split(' ')[0]})` : ''}
                        </div>
                      </div>
                    </div>
                  </div>

                  {provider.available ? (
                    <div className="border-t border-border/50 px-4 py-3 flex gap-2">
                      {(provider.models || ['default']).map((model) => (
                        <button
                          key={`${provider.id}:${model}`}
                          onClick={() => onSelectRemote(provider.id, model)}
                          className="flex-1 py-2 px-3 rounded-md text-xs font-medium transition-colors bg-muted hover:bg-accent text-foreground/80 hover:text-foreground border border-border hover:border-primary"
                        >
                          {model.charAt(0).toUpperCase() + model.slice(1)}
                        </button>
                      ))}
                    </div>
                  ) : (
                    <div className="border-t border-border/50 px-4 py-3">
                      <p className="text-xs text-muted-foreground">
                        Install CLI:{' '}
                        <code className="text-muted-foreground">
                          {cliInstallHints[provider.id] || ''}
                        </code>
                      </p>
                    </div>
                  )}
                </div>
              ))}

              {/* OpenAI-Compatible Cloud Providers */}
              {openaiProviders.length > 0 && (
                <>
                  <div className="flex items-center gap-2 pt-2">
                    <Zap className="h-3.5 w-3.5 text-amber-400" />
                    <span className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
                      OpenAI-Compatible Providers
                    </span>
                  </div>

                  {openaiProviders.map((provider) => (
                    <div
                      key={provider.id}
                      className={`rounded-lg border ${
                        currentProvider === provider.id
                          ? 'border-primary bg-primary/10'
                          : 'border-border'
                      }`}
                    >
                      <div className="p-4">
                        <div className="flex items-start gap-3">
                          <Zap className="h-5 w-5 text-amber-400 mt-0.5 flex-shrink-0" />
                          <div className="flex-1">
                            <div className="flex items-center gap-2">
                              <span className="font-medium text-foreground">{provider.name}</span>
                              {provider.available ? (
                                <span className="text-[10px] px-1.5 py-0.5 rounded bg-emerald-500/20 text-emerald-400">
                                  API key set
                                </span>
                              ) : (
                                <span className="text-[10px] px-1.5 py-0.5 rounded bg-muted text-muted-foreground">
                                  no API key
                                </span>
                              )}
                            </div>
                            <div className="text-xs text-muted-foreground mt-1">
                              {provider.description}
                            </div>
                          </div>
                        </div>
                      </div>

                      {(() => {
                        if (provider.available && (provider.models || []).length > 0) {
                          return (
                            <div className="border-t border-border/50 px-4 py-3 flex flex-wrap gap-2">
                              {(provider.models || []).map((model) => (
                                <button
                                  key={`${provider.id}:${model}`}
                                  onClick={() => onSelectRemote(provider.id, model)}
                                  className="py-2 px-3 rounded-md text-xs font-medium transition-colors bg-muted hover:bg-accent text-foreground/80 hover:text-foreground border border-border hover:border-primary"
                                >
                                  {model}
                                </button>
                              ))}
                            </div>
                          );
                        }
                        if (!provider.available) {
                          return (
                            <div className="border-t border-border/50 px-4 py-3">
                              <p className="text-xs text-muted-foreground">
                                Set API key in Settings (provider_api_keys)
                              </p>
                            </div>
                          );
                        }
                        return null;
                      })()}
                    </div>
                  ))}
                </>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
};
