import { useState, useEffect } from 'react';
import { X, Cpu, Cloud, Loader2 } from 'lucide-react';

interface Provider {
  id: string;
  name: string;
  available: boolean;
  description: string;
  version?: string;
  models?: string[];
}

interface ProviderSelectorProps {
  isOpen: boolean;
  onClose: () => void;
  onSelectLocal: () => void;
  onSelectClaude: (model: string) => void;
  currentProvider?: string;
}

export function ProviderSelector({ isOpen, onClose, onSelectLocal, onSelectClaude, currentProvider }: ProviderSelectorProps) {
  const [providers, setProviders] = useState<Provider[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!isOpen) return;
    setLoading(true);
    fetch('/api/providers')
      .then(r => r.json())
      .then(data => setProviders(data.providers || []))
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [isOpen]);

  if (!isOpen) return null;

  const localProvider = providers.find(p => p.id === 'local');
  const claudeProvider = providers.find(p => p.id === 'claude_code');

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60" onClick={onClose}>
      <div className="bg-zinc-900 border border-zinc-700 rounded-lg shadow-2xl w-[500px] max-w-[90vw] flex flex-col" onClick={e => e.stopPropagation()}>
        <div className="flex items-center justify-between px-5 py-4 border-b border-zinc-700">
          <h3 className="text-base font-medium text-zinc-100">Choose Provider</h3>
          <button onClick={onClose} className="p-1 rounded hover:bg-zinc-700 text-zinc-400 hover:text-zinc-200 transition-colors">
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="p-5 space-y-3">
          {loading ? (
            <div className="flex items-center justify-center py-8">
              <Loader2 className="h-6 w-6 animate-spin text-zinc-500" />
            </div>
          ) : (
            <>
              {/* Local Model */}
              <button
                onClick={onSelectLocal}
                className={`w-full text-left p-4 rounded-lg border transition-colors ${
                  currentProvider === 'local'
                    ? 'border-primary bg-primary/10'
                    : 'border-zinc-700 hover:border-zinc-500 hover:bg-zinc-800/50'
                }`}
              >
                <div className="flex items-start gap-3">
                  <Cpu className="h-5 w-5 text-emerald-400 mt-0.5 flex-shrink-0" />
                  <div>
                    <div className="font-medium text-zinc-100">{localProvider?.name || 'Local Model (llama.cpp)'}</div>
                    <div className="text-xs text-zinc-500 mt-1">{localProvider?.description || 'Run models locally on your GPU'}</div>
                  </div>
                </div>
              </button>

              {/* Claude Code */}
              <div className={`rounded-lg border ${
                currentProvider === 'claude_code'
                  ? 'border-primary bg-primary/10'
                  : 'border-zinc-700'
              }`}>
                <div className="p-4">
                  <div className="flex items-start gap-3">
                    <Cloud className="h-5 w-5 text-cyan-400 mt-0.5 flex-shrink-0" />
                    <div className="flex-1">
                      <div className="flex items-center gap-2">
                        <span className="font-medium text-zinc-100">{claudeProvider?.name || 'Claude Code'}</span>
                        {claudeProvider?.available ? (
                          <span className="text-[10px] px-1.5 py-0.5 rounded bg-emerald-500/20 text-emerald-400">connected</span>
                        ) : (
                          <span className="text-[10px] px-1.5 py-0.5 rounded bg-red-500/20 text-red-400">not installed</span>
                        )}
                      </div>
                      <div className="text-xs text-zinc-500 mt-1">
                        {claudeProvider?.description || 'Use your Claude Code subscription'}
                        {claudeProvider?.version ? ` (v${claudeProvider.version.split(' ')[0]})` : ''}
                      </div>
                    </div>
                  </div>
                </div>

                {claudeProvider?.available ? (
                  <div className="border-t border-zinc-700/50 px-4 py-3 flex gap-2">
                    {(claudeProvider.models || ['opus', 'sonnet', 'haiku']).map(model => (
                      <button
                        key={model}
                        onClick={() => onSelectClaude(model)}
                        className="flex-1 py-2 px-3 rounded-md text-xs font-medium transition-colors bg-zinc-800 hover:bg-zinc-700 text-zinc-300 hover:text-zinc-100 border border-zinc-700 hover:border-zinc-500"
                      >
                        {model.charAt(0).toUpperCase() + model.slice(1)}
                      </button>
                    ))}
                  </div>
                ) : (
                  <div className="border-t border-zinc-700/50 px-4 py-3">
                    <p className="text-xs text-zinc-600">Install Claude Code CLI: <code className="text-zinc-500">npm i -g @anthropic-ai/claude-code</code></p>
                  </div>
                )}
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
