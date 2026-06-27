import {
  Unplug,
  Activity,
  SlidersHorizontal,
  ScrollText,
  X,
  Menu,
  Globe,
  Bot,
  ChevronDown,
  Loader2,
  PowerOff,
  GitBranch,
  SquareTerminal,
  type LucideIcon,
} from 'lucide-react';
import React, { useRef, useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';

import { useAgentContext } from '../../contexts/AgentContext';
import { useChatContext } from '../../contexts/ChatContext';
import { useModelContext } from '../../contexts/ModelContext';
import { useUIContext } from '../../hooks/useUIContext';
import type { Agent, ViewMode } from '../../types';
import { getProviderLabel } from '../../utils/providerLabels';

import { ModelSelector } from './ModelSelector';

const VIEW_MODES = [
  { value: 'markdown' as ViewMode, label: 'MD', title: 'Markdown view' },
  { value: 'text' as ViewMode, label: 'TXT', title: 'Plain text view' },
  { value: 'raw' as ViewMode, label: 'RAW', title: 'Raw model output (no parsing)' },
];

const ViewModeToggle = ({
  viewMode,
  onChange,
}: {
  viewMode: ViewMode;
  onChange: (m: ViewMode) => void;
}) => {
  return (
    <div className="flex items-center rounded-md bg-muted p-0.5">
      {VIEW_MODES.map(({ value, label, title }) => (
        <button
          key={value}
          onClick={() => onChange(value)}
          className={`rounded px-2.5 py-1 text-xs font-medium transition-colors ${
            viewMode === value
              ? 'bg-card text-foreground'
              : 'text-muted-foreground hover:text-foreground'
          }`}
          title={title}
        >
          {label}
        </button>
      ))}
    </div>
  );
};

interface ChatHeaderProps {
  onModelUnload: () => void;
  onForceUnload: () => void;
  showAgentSelector: boolean;
}

/** Trailing icon button on a dropdown row that unloads the agent or its loaded model. */
const RowUnloadButton = ({
  icon: Icon,
  title,
  onClick,
}: {
  icon: LucideIcon;
  title: string;
  onClick: (e: React.MouseEvent) => void;
}) => (
  <button
    onClick={onClick}
    className="flex-shrink-0 px-2 py-1.5 text-muted-foreground transition-colors hover:text-destructive"
    title={title}
  >
    <Icon className="size-3" />
  </button>
);

/** One selectable agent row in the picker dropdown, with its trailing unload control. */
const AgentRow = ({
  agent,
  isActive,
  isRunning,
  isLoadedModel,
  dotCls,
  dotTitle,
  onSelect,
  onStopAgent,
  onUnloadModel,
}: {
  agent: Agent;
  isActive: boolean;
  isRunning: boolean;
  isLoadedModel: boolean;
  dotCls: string;
  dotTitle: string;
  onSelect: () => void;
  onStopAgent: (e: React.MouseEvent) => void;
  onUnloadModel: (e: React.MouseEvent) => void;
}) => (
  <div className="flex items-center">
    <button
      onClick={onSelect}
      className={`flex min-w-0 flex-1 items-center gap-2 px-3 py-1.5 text-xs transition-colors hover:bg-muted ${isActive ? 'font-medium text-foreground' : 'text-muted-foreground'}`}
    >
      <span className={`size-2 flex-shrink-0 rounded-full ${dotCls}`} title={dotTitle} />
      <span className="truncate">{agent.name}</span>
    </button>
    {!!isRunning && (
      <RowUnloadButton icon={PowerOff} title="Unload agent from memory" onClick={onStopAgent} />
    )}
    {!isRunning && !!isLoadedModel && (
      <RowUnloadButton icon={Unplug} title="Unload model from memory" onClick={onUnloadModel} />
    )}
  </div>
);

/** Inline agent picker dropdown shown in the chat header. */
const AgentPicker = ({ onModelUnload }: { onModelUnload: () => void }) => {
  const { t } = useTranslation();
  const {
    agents,
    agentStatuses,
    conversationAgent,
    stagedAgent,
    activateAgent,
    stopAgent,
    setConversationAgent,
    setStagedAgent,
    fetchAgentStatuses,
  } = useAgentContext();
  const { currentConversationId, clearMessages } = useChatContext();
  const { isLoading: isModelLoading, status: modelStatus } = useModelContext();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  // Close on outside click
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  // For existing conversations, only the assigned conversationAgent is active.
  // stagedAgent is only the fallback when starting a brand-new chat (no conversation yet).
  const activeAgent = currentConversationId
    ? conversationAgent
    : (conversationAgent ?? stagedAgent);

  const handleSelect = useCallback(
    async (agent: Agent | null) => {
      setOpen(false);
      if (busy) return;
      // No-op if already selected
      if ((agent?.id ?? null) === (activeAgent?.id ?? null)) return;

      setBusy(true);
      try {
        if (!currentConversationId) {
          // No conversation yet — clear state, stage, and activate
          clearMessages();
          setStagedAgent(agent);
          if (agent) await activateAgent(agent.id).catch(() => {});
          return;
        }
        // Assign to conversation (backend auto-deactivates old agent if unused)
        await setConversationAgent(currentConversationId, agent?.id ?? null);
        if (agent) await activateAgent(agent.id).catch(() => {});
        await fetchAgentStatuses();
      } finally {
        setBusy(false);
      }
    },
    [
      busy,
      activeAgent,
      currentConversationId,
      clearMessages,
      setStagedAgent,
      setConversationAgent,
      activateAgent,
      fetchAgentStatuses,
    ],
  );

  const dotClass = (agentId: string) => {
    const s = agentStatuses[agentId]?.status ?? 'idle';
    if (s === 'generating') return 'bg-amber-400 animate-pulse';
    if (s === 'active') return 'bg-emerald-400';
    // Show green dot if this agent's model is currently loaded in VRAM (even if idle)
    const agent = agents.find((a) => a.id === agentId);
    if (
      agent?.provider_id === 'local' &&
      agent.model_path &&
      modelStatus.loaded &&
      modelStatus.model_path === agent.model_path
    ) {
      return 'bg-emerald-400';
    }
    return 'bg-muted-foreground/30';
  };
  const dotTitle = (agentId: string) => {
    const s = agentStatuses[agentId]?.status ?? 'idle';
    if (s === 'generating') return 'Agent is generating';
    if (s === 'active') return 'Agent is running';
    return 'Agent is idle';
  };

  // Show the "Unload model" control on the dropdown row whose model is actually loaded.
  const loadedModelPath = modelStatus.loaded ? (modelStatus.model_path ?? null) : null;
  const baseName = (p?: string | null) => p?.split(/[/\\]/).pop()?.toLowerCase() ?? '';
  const isLoadedModelFor = (agent: Agent) =>
    !!loadedModelPath &&
    !!agent.model_path &&
    baseName(loadedModelPath) === baseName(agent.model_path);

  const pickerTitle = activeAgent ? `Agent: ${activeAgent.name}` : 'Select agent';
  const pickerIcon = busy ? (
    <Loader2 className="size-3.5 flex-shrink-0 animate-spin" />
  ) : (
    <Bot className="size-3.5 flex-shrink-0" />
  );
  const pickerLabel = busy ? 'Loading…' : (activeAgent?.name ?? t('common.noAgent'));

  return (
    <div ref={ref} className="relative hidden sm:block">
      <button
        onClick={() => setOpen((v) => !v)}
        disabled={busy || isModelLoading}
        className={`flex max-w-[180px] items-center gap-1.5 rounded-md border px-2 py-1 text-xs transition-colors hover:bg-muted disabled:opacity-50 ${activeAgent ? 'border-border/80 bg-muted/50 text-foreground' : 'border-border/60 bg-muted/35 text-muted-foreground hover:text-foreground'}`}
        title={pickerTitle}
      >
        {pickerIcon}
        <span className="truncate">{pickerLabel}</span>
        <ChevronDown className="ml-0.5 size-3 flex-shrink-0" />
      </button>

      {!!open && (
        <div className="absolute left-0 top-full z-50 mt-1 min-w-[160px] rounded-md border border-border bg-popover py-1 shadow-md">
          {/* No agent option */}
          <button
            onClick={() => handleSelect(null)}
            className={`flex w-full items-center gap-2 px-3 py-1.5 text-xs transition-colors hover:bg-muted ${!activeAgent ? 'font-medium text-foreground' : 'text-muted-foreground'}`}
          >
            <span className="size-2 flex-shrink-0 rounded-full bg-muted-foreground/30" />
            {t('common.noAgent')}
          </button>
          {agents.length > 0 && <div className="my-1 border-t border-border/50" />}
          {agents.map((agent) => {
            const status = agentStatuses[agent.id]?.status ?? 'idle';
            const isRunning =
              agent.provider_id === 'local' && (status === 'active' || status === 'generating');
            return (
              <AgentRow
                key={agent.id}
                agent={agent}
                isActive={activeAgent?.id === agent.id}
                isRunning={isRunning}
                isLoadedModel={isLoadedModelFor(agent)}
                dotCls={dotClass(agent.id)}
                dotTitle={dotTitle(agent.id)}
                onSelect={() => handleSelect(agent)}
                onStopAgent={async (e) => {
                  e.stopPropagation();
                  await stopAgent(agent.id);
                  await fetchAgentStatuses();
                  // Deselect if this was the active agent
                  if (activeAgent?.id === agent.id) {
                    await handleSelect(null);
                  }
                }}
                onUnloadModel={(e) => {
                  e.stopPropagation();
                  setOpen(false);
                  onModelUnload();
                }}
              />
            );
          })}
        </div>
      )}
    </div>
  );
};

const HeaderAgentControls = ({
  onModelUnload,
  onForceUnload,
}: Pick<ChatHeaderProps, 'onModelUnload' | 'onForceUnload'>) => {
  const { status, isLoading, loadingAction, hasStatusError, activeProvider, activeProviderModel } =
    useModelContext();
  const { openAgentSelector } = useUIContext();

  const modelLoaded = status.loaded || activeProvider !== 'local';
  const remoteProviderLabel = getProviderLabel(activeProvider);

  const currentModelPath =
    activeProvider !== 'local' ? `${remoteProviderLabel} (${activeProviderModel})` : undefined;

  return (
    <>
      <ModelSelector
        currentModelPath={currentModelPath}
        isLoading={isLoading}
        loadingAction={loadingAction}
        loadingProgress={status.loading_progress}
        onOpen={openAgentSelector}
      />
      <AgentPicker onModelUnload={onModelUnload} />
      {!!isLoading && loadingAction === 'loading' && (
        <button
          onClick={onForceUnload}
          className="rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-muted"
          title="Cancel model loading"
          aria-label="Cancel model loading"
        >
          <X className="size-3.5" />
        </button>
      )}
      {!isLoading && !modelLoaded && !!hasStatusError && (
        <button
          onClick={onForceUnload}
          className="rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
          title="Force unload"
        >
          <Unplug className="size-3.5" />
        </button>
      )}
    </>
  );
};

export const ChatHeader = React.memo(
  ({ onModelUnload, onForceUnload, showAgentSelector }: ChatHeaderProps) => {
    const { status: modelStatus, isLoading: isModelLoading, activeProvider } = useModelContext();
    const {
      viewMode,
      setViewMode,
      isRightSidebarOpen,
      toggleRightSidebar,
      isModelConfigOpen,
      openModelConfig,
      isEventLogOpen,
      toggleEventLog,
      toggleMobileSidebar,
      isBrowserViewOpen,
      toggleBrowserView,
      closeBrowserView,
      browserViewUrl,
      isGitGraphOpen,
      toggleGitGraph,
      isTerminalOpen,
      toggleTerminal,
    } = useUIContext();

    // When user clicks MD/TXT/RAW, close browser view to show the chat
    const handleSetViewMode = (mode: ViewMode) => {
      if (isBrowserViewOpen) closeBrowserView();
      setViewMode(mode);
    };

    const modelLoaded = modelStatus.loaded || activeProvider !== 'local';

    return (
      <div
        className="sticky top-0 z-20 flex items-center justify-between border-b border-border bg-background px-4 py-2"
        data-testid="chat-header"
      >
        {/* Left: hamburger (mobile) + model selector + unload */}
        <div className="flex min-w-0 items-center gap-1">
          <button
            onClick={toggleMobileSidebar}
            className="rounded-md p-2 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground md:hidden"
            title="Toggle sidebar"
            aria-label="Toggle sidebar"
          >
            <Menu className="size-6" />
          </button>
          {!!showAgentSelector && (
            <HeaderAgentControls onModelUnload={onModelUnload} onForceUnload={onForceUnload} />
          )}
        </div>

        {/* Right: browser (always) + view toggle + monitor (when loaded) */}
        <div className="flex items-center gap-1.5 md:gap-3">
          <div className="hidden md:block">
            <ViewModeToggle viewMode={viewMode} onChange={handleSetViewMode} />
          </div>

          <button
            onClick={toggleBrowserView}
            className={`btn-icon ${isBrowserViewOpen ? 'active' : ''} ${browserViewUrl && !isBrowserViewOpen ? 'animate-pulse text-foreground' : ''}`}
            title="Toggle browser view"
          >
            <Globe className="size-3.5" />
          </button>

          <button
            onClick={toggleGitGraph}
            className={`btn-icon ${isGitGraphOpen ? 'active' : ''}`}
            title="Toggle git graph"
            aria-label="Toggle git graph"
          >
            <GitBranch className="size-3.5" />
          </button>

          <button
            onClick={toggleTerminal}
            className={`btn-icon ${isTerminalOpen ? 'active' : ''}`}
            title="Toggle terminal"
            aria-label="Toggle terminal"
          >
            <SquareTerminal className="size-3.5" />
          </button>

          {!!modelLoaded && (
            <>
              <button
                onClick={toggleEventLog}
                className={`btn-icon ${isEventLogOpen ? 'active' : ''}`}
                title="Event log"
              >
                <ScrollText className="size-3.5" />
              </button>

              <button
                onClick={openModelConfig}
                disabled={isModelLoading}
                className={`btn-icon ${isModelConfigOpen ? 'active' : ''} ${isModelLoading ? 'cursor-not-allowed opacity-30' : ''}`}
                title="Model settings"
              >
                <SlidersHorizontal className="size-3.5" />
              </button>
            </>
          )}

          <button
            onClick={toggleRightSidebar}
            className={`btn-icon ${isRightSidebarOpen ? 'active' : ''}`}
            title="Toggle system monitor"
          >
            <Activity className="size-3.5" />
          </button>
        </div>
      </div>
    );
  },
);
ChatHeader.displayName = 'ChatHeader';
