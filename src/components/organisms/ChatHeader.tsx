import React from 'react';
import { Unplug, Activity, SlidersHorizontal, ScrollText, X } from 'lucide-react';
import { ModelSelector } from './ModelSelector';
import { useModelContext } from '../../contexts/ModelContext';
import { useUIContext } from '../../contexts/UIContext';
import { getProviderLabel } from '../../utils/providerLabels';
import type { ViewMode } from '../../types';

const VIEW_MODES = [
  { value: 'markdown' as ViewMode, label: 'MD', title: 'Markdown view' },
  { value: 'text' as ViewMode, label: 'TXT', title: 'Plain text view' },
  { value: 'raw' as ViewMode, label: 'RAW', title: 'Raw model output (no parsing)' },
];

function ViewModeToggle({ viewMode, onChange }: { viewMode: ViewMode; onChange: (m: ViewMode) => void }) {
  return (
    <div className="flex items-center bg-muted rounded-md p-0.5">
      {VIEW_MODES.map(({ value, label, title }) => (
        <button
          key={value}
          onClick={() => onChange(value)}
          className={`px-2.5 py-1 text-xs font-medium rounded transition-colors ${
            viewMode === value ? 'bg-card text-foreground' : 'text-muted-foreground hover:text-foreground'
          }`}
          title={title}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

interface ChatHeaderProps {
  onModelUnload: () => void;
  onForceUnload: () => void;
}

export const ChatHeader = React.memo(function ChatHeader({ onModelUnload, onForceUnload }: ChatHeaderProps) {
  const { status: modelStatus, isLoading: isModelLoading, loadingAction, hasStatusError, activeProvider, activeProviderModel } = useModelContext();
  const { viewMode, setViewMode, isRightSidebarOpen, toggleRightSidebar, isConfigSidebarOpen, toggleConfigSidebar, isEventLogOpen, toggleEventLog, openProviderSelector } = useUIContext();

  const modelLoaded = modelStatus.loaded || activeProvider !== 'local';
  const remoteProviderLabel = getProviderLabel(activeProvider);

  return (
    <div className="flex items-center justify-between px-4 py-2 border-b border-border" data-testid="chat-header">
      {/* Left: model selector + unload */}
      <div className="flex items-center gap-1 min-w-0">
        <ModelSelector
          currentModelPath={
            activeProvider !== 'local'
              ? `${remoteProviderLabel} (${activeProviderModel})`
              : modelStatus.model_path ?? undefined
          }
          isLoading={isModelLoading}
          loadingAction={loadingAction}
          loadingProgress={modelStatus.loading_progress}
          onOpen={openProviderSelector}
        />
        {isModelLoading && loadingAction === 'loading' ? <button
            onClick={onForceUnload}
            className="p-1.5 rounded-md text-white hover:bg-white/10 transition-colors"
            title="Cancel model loading"
            aria-label="Cancel model loading"
          >
            <X className="h-3.5 w-3.5" />
          </button> : null}
        {!isModelLoading && modelLoaded ? <button
            onClick={onModelUnload}
            className="p-1.5 rounded-md text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors"
            title="Unload model"
          >
            <Unplug className="h-3.5 w-3.5" />
          </button> : null}
        {!isModelLoading && !modelLoaded && hasStatusError ? <button
            onClick={onForceUnload}
            className="p-1.5 rounded-md text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors"
            title="Force unload"
          >
            <Unplug className="h-3.5 w-3.5" />
          </button> : null}
      </div>

      {/* Right: context + view toggle + monitor */}
      {modelLoaded ? <div className="flex items-center gap-3">
          <ViewModeToggle viewMode={viewMode} onChange={setViewMode} />

          <button
            onClick={toggleEventLog}
            className={`btn-icon ${isEventLogOpen ? 'active' : ''}`}
            title="Event log"
          >
            <ScrollText className="h-3.5 w-3.5" />
          </button>

          <button
            onClick={toggleConfigSidebar}
            disabled={isModelLoading}
            className={`btn-icon ${isConfigSidebarOpen ? 'active' : ''} ${isModelLoading ? 'opacity-30 cursor-not-allowed' : ''}`}
            title="Conversation settings"
          >
            <SlidersHorizontal className="h-3.5 w-3.5" />
          </button>

          <button
            onClick={toggleRightSidebar}
            className={`btn-icon ${isRightSidebarOpen ? 'active' : ''}`}
            title="Toggle system monitor"
          >
            <Activity className="h-3.5 w-3.5" />
          </button>
        </div> : null}
    </div>
  );
});
