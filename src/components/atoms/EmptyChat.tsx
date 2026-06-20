import { Download, Bot, Loader2, X, CheckCircle } from 'lucide-react';
import React, { useCallback, useEffect, useState } from 'react';

import { useAgentContext } from '../../contexts/AgentContext';
import { useChatContext } from '../../contexts/ChatContext';
import { useModelContext } from '../../contexts/ModelContext';
import { useUIContext } from '../../hooks/useUIContext';
import { getProviderLabel } from '../../utils/providerLabels';
import { getAvailableBackends } from '../../utils/tauriCommands';

interface WelcomeMessageProps {
  children?: React.ReactNode;
}

type InstallState = 'idle' | 'installing' | 'done' | 'error';

// eslint-disable-next-line complexity, max-lines-per-function
export const EmptyChat: React.FC<WelcomeMessageProps> = ({ children }) => {
  const {
    status,
    isLoading,
    loadingAction,
    modelName,
    forceUnload,
    activeProvider,
    activeProviderModel,
  } = useModelContext();
  const { openAgentSelector } = useUIContext();
  const { currentConversationId } = useChatContext();
  const { conversationAgent, stagedAgent, agentStatuses, activatingAgentId, activateAgent } =
    useAgentContext();
  // Mirror AgentPicker: for an existing conversation use conversationAgent, otherwise stage.
  const activeAgent = currentConversationId
    ? conversationAgent
    : (conversationAgent ?? stagedAgent);
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

  // Hide input whenever loading — covers both initial load and switching models/agents.
  if (isLoading) {
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
    const progressBarClass = isWarmup ? 'animate-pulse' : 'transition-all duration-300 ease-out';
    const progressBarWidth = isWarmup ? '100%' : `${progress}%`;
    const loadingIndicator =
      hasProgress || isWarmup ? (
        <div className="h-1.5 w-48 overflow-hidden rounded-full bg-foreground/20">
          <div
            className={`h-full rounded-full bg-foreground ${progressBarClass}`}
            style={{ width: progressBarWidth }}
          />
        </div>
      ) : (
        <Loader2 className="h-6 w-6 animate-spin text-foreground" />
      );

    return (
      <div className="flex flex-1 flex-col items-center justify-center">
        {loadingIndicator}
        <p className="mt-3 text-sm text-foreground">{text}</p>
        {loadingAction === 'loading' && (
          <button
            type="button"
            onClick={forceUnload}
            className="mt-4 flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm text-foreground transition-colors hover:bg-muted"
            aria-label="Cancel model loading"
          >
            <X className="h-3.5 w-3.5" />
            Cancel
          </button>
        )}
      </div>
    );
  }

  if (activeProvider !== 'local' || (status.loaded && modelName && !status.is_agent_model)) {
    const headingText = activeProvider !== 'local' ? remoteHeading : modelName;
    return (
      <div className="flex flex-1 flex-col items-center justify-center">
        <h2 className="mb-6 text-xl font-semibold">{headingText}</h2>
        {children}
      </div>
    );
  }

  // Remote agent selected — model not needed, show agent name and input
  if (activeAgent && activeAgent.provider_id !== 'local') {
    return (
      <div className="flex flex-1 flex-col items-center justify-center">
        <h2 className="mb-6 text-xl font-semibold">{activeAgent.name}</h2>
        {children}
      </div>
    );
  }

  // Local agent selected — show name with input (disabled until agent worker is ready)
  if (activeAgent) {
    const agentActivating = activatingAgentId === activeAgent.id;
    const agentStatusVal = agentStatuses[activeAgent.id]?.status;
    const agentRunning = agentStatusVal === 'active' || agentStatusVal === 'generating';
    const agentStopped =
      !agentActivating &&
      !agentRunning &&
      activeAgent.provider_id === 'local' &&
      !status.loaded &&
      !isLoading;
    let agentContent;
    if (agentActivating) {
      agentContent = <Loader2 className="h-6 w-6 animate-spin text-foreground" />;
    } else if (agentStopped) {
      agentContent = (
        <button
          type="button"
          onClick={() => activateAgent(activeAgent.id)}
          className="rounded-md border border-border px-4 py-2 text-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          Start agent
        </button>
      );
    } else {
      agentContent = children;
    }
    return (
      <div className="flex flex-1 flex-col items-center justify-center">
        <h2 className="mb-6 text-xl font-semibold">{activeAgent.name}</h2>
        {agentContent}
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3">
      <button
        type="button"
        onClick={openAgentSelector}
        className="flat-card flex cursor-pointer flex-col items-center gap-3 bg-muted/50 px-10 py-8 transition-colors hover:bg-muted"
      >
        <Bot className="h-8 w-8 text-foreground" />
        <span className="text-sm font-medium text-foreground">Select an agent to start</span>
      </button>
      {!!cudaBanner && (
        <div className="max-w-sm space-y-2 rounded-lg border border-primary/30 bg-primary/10 p-3 text-center">
          <p className="text-xs text-foreground">
            NVIDIA GPU detected but CUDA acceleration is not installed.
          </p>
          {installState === 'idle' && (
            <button
              type="button"
              onClick={handleInstallGpu}
              className="inline-flex items-center gap-1.5 rounded bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition-colors hover:bg-primary/90"
            >
              <Download className="h-3.5 w-3.5" />
              Install GPU Acceleration
            </button>
          )}
          {installState === 'installing' && (
            <div className="space-y-1.5">
              <div className="flex items-center justify-center gap-2">
                <Loader2 className="h-3.5 w-3.5 animate-spin text-primary" />
                <span className="text-xs text-foreground">Downloading... {installProgress}%</span>
              </div>
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-foreground/20">
                <div
                  className="h-full rounded-full bg-primary transition-all duration-300 ease-out"
                  style={{ width: `${installProgress}%` }}
                />
              </div>
            </div>
          )}
          {installState === 'done' && (
            <div className="flex items-center justify-center gap-1.5">
              <CheckCircle className="h-3.5 w-3.5 text-green-500" />
              <span className="text-xs text-green-500">
                Installed! Restart the app to activate.
              </span>
            </div>
          )}
          {installState === 'error' && (
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
          )}
          {installState === 'idle' && (
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
          )}
        </div>
      )}
    </div>
  );
};
