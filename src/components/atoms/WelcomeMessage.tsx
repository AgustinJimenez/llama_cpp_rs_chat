import { Download, Zap, Loader2, X, CheckCircle } from 'lucide-react';
import React, { useCallback, useEffect, useState } from 'react';

import { useModelContext } from '../../contexts/ModelContext';
import { useUIContext } from '../../hooks/useUIContext';
import { getProviderLabel } from '../../utils/providerLabels';
import { getAvailableBackends } from '../../utils/tauriCommands';

interface WelcomeMessageProps {
  children?: React.ReactNode;
}

type InstallState = 'idle' | 'installing' | 'done' | 'error';

// eslint-disable-next-line complexity, max-lines-per-function
export const WelcomeMessage: React.FC<WelcomeMessageProps> = ({ children }) => {
  const {
    status,
    isLoading,
    loadingAction,
    modelName,
    forceUnload,
    activeProvider,
    activeProviderModel,
  } = useModelContext();
  const { openProviderSelector } = useUIContext();
  const remoteProviderLabel = getProviderLabel(activeProvider);
  const remoteHeading = `${remoteProviderLabel} (${activeProviderModel})`;

  // Detect NVIDIA GPU without CUDA for the welcome screen hint
  const [cudaBanner, setCudaBanner] = useState(false);
  const [installState, setInstallState] = useState<InstallState>('idle');
  const [installProgress, setInstallProgress] = useState(0);
  const [installError, setInstallError] = useState('');

  useEffect(() => {
    getAvailableBackends()
      .then((resp) => {
        if (resp.nvidia_gpu_detected && !resp.cuda_backend_loaded) {
          setCudaBanner(true);
        }
      })
      .catch(() => {});
  }, []);

  const handleInstallGpu = useCallback(() => {
    setInstallState('installing');
    setInstallProgress(0);
    setInstallError('');

    fetch('/api/backends/install', { method: 'POST' })
      .then((resp) => {
        if (!resp.body) throw new Error('No response body');
        const reader = resp.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';

        const read = (): Promise<void> =>
          reader.read().then(({ done, value }) => {
            if (done) return;
            buffer += decoder.decode(value, { stream: true });
            const lines = buffer.split('\n');
            buffer = lines.pop() || '';
            for (const line of lines) {
              if (!line.startsWith('data: ')) continue;
              try {
                const SSE_PREFIX_LEN = 6; // "data: ".length
                const evt = JSON.parse(line.slice(SSE_PREFIX_LEN));
                if (evt.type === 'progress') {
                  setInstallProgress(evt.percent || 0);
                } else if (evt.type === 'done') {
                  setInstallState('done');
                } else if (evt.type === 'error') {
                  setInstallState('error');
                  setInstallError(evt.message || 'Download failed');
                }
              } catch {
                /* ignore parse errors */
              }
            }
            return read();
          });

        return read();
      })
      .catch((e) => {
        setInstallState('error');
        setInstallError(String(e));
      });
  }, []);

  // Show loading here only when the header is hidden (model not yet loaded).
  // When status.loaded is true, the header is visible and its ModelSelector handles loading/unloading state — only one indicator at a time.
  if (isLoading && !status.loaded) {
    const progress = status.loading_progress;
    const isWarmup = loadingAction === 'loading' && progress != null && progress > 100;
    const hasProgress =
      loadingAction === 'loading' && progress != null && progress > 0 && !isWarmup;
    let text = 'Loading model...';
    if (loadingAction === 'unloading') {
      text = 'Unloading model...';
    } else if (isWarmup) {
      text = 'Loading system prompt...';
    } else if (hasProgress) {
      text = `Loading model... ${progress}%`;
    }
    return (
      <div className="flex-1 flex flex-col items-center justify-center">
        {hasProgress || isWarmup ? (
          <div className="w-48 h-1.5 bg-foreground/20 rounded-full overflow-hidden">
            <div
              className={`h-full bg-foreground rounded-full ${isWarmup ? 'animate-pulse' : 'transition-all duration-300 ease-out'}`}
              style={{ width: isWarmup ? '100%' : `${progress}%` }}
            />
          </div>
        ) : (
          <Loader2 className="h-6 w-6 text-foreground animate-spin" />
        )}
        <p className="text-foreground text-sm mt-3">{text}</p>
        {loadingAction === 'loading' ? (
          <button
            type="button"
            onClick={forceUnload}
            className="mt-4 flex items-center gap-1.5 px-3 py-1.5 text-sm text-foreground hover:bg-muted rounded-md transition-colors"
            aria-label="Cancel model loading"
          >
            <X className="h-3.5 w-3.5" />
            Cancel
          </button>
        ) : null}
      </div>
    );
  }

  if ((status.loaded && modelName) || activeProvider !== 'local') {
    return (
      <div className="flex-1 flex flex-col items-center justify-center">
        <h2 className="text-xl font-semibold mb-6">
          {activeProvider !== 'local' ? remoteHeading : modelName}
        </h2>
        {children}
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col items-center justify-center gap-3">
      <button
        type="button"
        onClick={openProviderSelector}
        className="flat-card flex flex-col items-center gap-3 px-10 py-8 bg-muted/50 hover:bg-muted transition-colors cursor-pointer"
      >
        <Zap className="h-8 w-8 text-foreground" />
        <span className="text-sm font-medium text-foreground">
          Choose a provider to start chatting
        </span>
      </button>
      {cudaBanner ? (
        <div className="max-w-sm p-3 rounded-lg bg-primary/10 border border-primary/30 text-center space-y-2">
          <p className="text-xs text-foreground">
            NVIDIA GPU detected but CUDA acceleration is not installed.
          </p>
          {installState === 'idle' ? (
            <button
              type="button"
              onClick={handleInstallGpu}
              className="inline-flex items-center gap-1.5 text-xs font-medium px-3 py-1.5 rounded bg-primary hover:bg-primary/90 text-primary-foreground transition-colors"
            >
              <Download className="h-3.5 w-3.5" />
              Install GPU Acceleration
            </button>
          ) : null}
          {installState === 'installing' ? (
            <div className="space-y-1.5">
              <div className="flex items-center justify-center gap-2">
                <Loader2 className="h-3.5 w-3.5 animate-spin text-primary" />
                <span className="text-xs text-foreground">Downloading... {installProgress}%</span>
              </div>
              <div className="w-full h-1.5 bg-foreground/20 rounded-full overflow-hidden">
                <div
                  className="h-full bg-primary rounded-full transition-all duration-300 ease-out"
                  style={{ width: `${installProgress}%` }}
                />
              </div>
            </div>
          ) : null}
          {installState === 'done' ? (
            <div className="flex items-center justify-center gap-1.5">
              <CheckCircle className="h-3.5 w-3.5 text-green-500" />
              <span className="text-xs text-green-500">
                Installed! Restart the app to activate.
              </span>
            </div>
          ) : null}
          {installState === 'error' ? (
            <div className="space-y-1">
              <p className="text-xs text-red-400">{installError}</p>
              <button
                type="button"
                onClick={handleInstallGpu}
                className="text-xs text-primary hover:underline"
              >
                Retry
              </button>
            </div>
          ) : null}
          {installState === 'idle' ? (
            <p className="text-[10px] text-muted-foreground">
              Downloads ~170MB GPU backend. Requires CUDA Toolkit from{' '}
              <a
                href="https://developer.nvidia.com/cuda-downloads"
                target="_blank"
                rel="noopener noreferrer"
                className="text-primary hover:underline"
              >
                nvidia.com
              </a>
              .
            </p>
          ) : null}
        </div>
      ) : null}
    </div>
  );
};
