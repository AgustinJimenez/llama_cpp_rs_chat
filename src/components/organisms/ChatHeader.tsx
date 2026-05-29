import { Unplug, Activity, SlidersHorizontal, ScrollText, X, Menu, Globe } from 'lucide-react';
import React from 'react';

import { useModelContext } from '../../contexts/ModelContext';
import { useUIContext } from '../../hooks/useUIContext';
import type { ViewMode } from '../../types';
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
    <div className="flex items-center bg-muted rounded-md p-0.5">
      {VIEW_MODES.map(({ value, label, title }) => (
        <button
          key={value}
          onClick={() => onChange(value)}
          className={`px-2.5 py-1 text-xs font-medium rounded transition-colors ${
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
    activeProvider !== 'local'
      ? `${remoteProviderLabel} (${activeProviderModel})`
      : (status.model_path ?? undefined);

  return (
    <>
      <ModelSelector
        currentModelPath={currentModelPath}
        isLoading={isLoading}
        loadingAction={loadingAction}
        loadingProgress={status.loading_progress}
        onOpen={openAgentSelector}
      />
      {!!isLoading && loadingAction === 'loading' && (
        <button
          onClick={onForceUnload}
          className="p-1.5 rounded-md text-muted-foreground hover:bg-muted transition-colors"
          title="Cancel model loading"
          aria-label="Cancel model loading"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      )}
      {!isLoading && !!modelLoaded && (
        <button
          onClick={onModelUnload}
          className="p-1.5 rounded-md text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors"
          title="Unload model"
        >
          <Unplug className="h-3.5 w-3.5" />
        </button>
      )}
      {!isLoading && !modelLoaded && !!hasStatusError && (
        <button
          onClick={onForceUnload}
          className="p-1.5 rounded-md text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors"
          title="Force unload"
        >
          <Unplug className="h-3.5 w-3.5" />
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
    } = useUIContext();

    // When user clicks MD/TXT/RAW, close browser view to show the chat
    const handleSetViewMode = (mode: ViewMode) => {
      if (isBrowserViewOpen) closeBrowserView();
      setViewMode(mode);
    };

    const modelLoaded = modelStatus.loaded || activeProvider !== 'local';

    return (
      <div
        className="flex items-center justify-between px-4 py-2 border-b border-border"
        data-testid="chat-header"
      >
        {/* Left: hamburger (mobile) + model selector + unload */}
        <div className="flex items-center gap-1 min-w-0">
          <button
            onClick={toggleMobileSidebar}
            className="p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-muted transition-colors md:hidden"
            title="Toggle sidebar"
            aria-label="Toggle sidebar"
          >
            <Menu className="h-4 w-4" />
          </button>
          {!!showAgentSelector && (
            <HeaderAgentControls onModelUnload={onModelUnload} onForceUnload={onForceUnload} />
          )}
        </div>

        {/* Right: browser (always) + view toggle + monitor (when loaded) */}
        <div className="flex items-center gap-1.5 md:gap-3">
          {!!modelLoaded && (
            <div className="hidden md:block">
              <ViewModeToggle viewMode={viewMode} onChange={handleSetViewMode} />
            </div>
          )}

          <button
            onClick={toggleBrowserView}
            className={`btn-icon ${isBrowserViewOpen ? 'active' : ''} ${browserViewUrl && !isBrowserViewOpen ? 'animate-pulse text-foreground' : ''}`}
            title="Toggle browser view"
          >
            <Globe className="h-3.5 w-3.5" />
          </button>

          {!!modelLoaded && (
            <>
              <button
                onClick={toggleEventLog}
                className={`btn-icon ${isEventLogOpen ? 'active' : ''}`}
                title="Event log"
              >
                <ScrollText className="h-3.5 w-3.5" />
              </button>

              <button
                onClick={openModelConfig}
                disabled={isModelLoading}
                className={`btn-icon ${isModelConfigOpen ? 'active' : ''} ${isModelLoading ? 'opacity-30 cursor-not-allowed' : ''}`}
                title="Model settings"
              >
                <SlidersHorizontal className="h-3.5 w-3.5" />
              </button>
            </>
          )}

          <button
            onClick={toggleRightSidebar}
            className={`btn-icon ${isRightSidebarOpen ? 'active' : ''}`}
            title="Toggle system monitor"
          >
            <Activity className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>
    );
  },
);
ChatHeader.displayName = 'ChatHeader';
