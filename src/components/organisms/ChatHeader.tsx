import { Unplug, Activity, SlidersHorizontal } from 'lucide-react';
import { ModelSelector } from './ModelSelector';
import { useModelContext } from '../../contexts/ModelContext';
import { useChatContext } from '../../contexts/ChatContext';
import { useUIContext } from '../../contexts/UIContext';
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

export function ChatHeader({ onModelUnload, onForceUnload }: ChatHeaderProps) {
  const { status: modelStatus, isLoading: isModelLoading, loadingAction, hasStatusError } = useModelContext();
  const { currentConversationId } = useChatContext();
  const { viewMode, setViewMode, isRightSidebarOpen, toggleRightSidebar, isConfigSidebarOpen, toggleConfigSidebar, openModelConfig } = useUIContext();

  const modelLoaded = modelStatus.loaded;

  return (
    <div className="flex items-center justify-between px-4 py-2 border-b border-border" data-testid="chat-header">
      {/* Left: model selector + unload */}
      <div className="flex items-center gap-1 min-w-0">
        <ModelSelector
          currentModelPath={modelStatus.model_path ?? undefined}
          isLoading={isModelLoading && currentConversationId !== null}
          loadingAction={loadingAction}
          onOpen={openModelConfig}
        />
        {modelLoaded ? <button
            onClick={onModelUnload}
            disabled={isModelLoading}
            className="p-1.5 rounded-md text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors disabled:opacity-50"
            title="Unload model"
          >
            <Unplug className="h-3.5 w-3.5" />
          </button> : null}
        {!modelLoaded && hasStatusError ? <button
            onClick={onForceUnload}
            disabled={isModelLoading}
            className="p-1.5 rounded-md text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors disabled:opacity-50"
            title="Force unload"
          >
            <Unplug className="h-3.5 w-3.5" />
          </button> : null}
      </div>

      {/* Right: context + view toggle + monitor */}
      {modelLoaded ? <div className="flex items-center gap-3">
          <ViewModeToggle viewMode={viewMode} onChange={setViewMode} />

          <button
            onClick={toggleConfigSidebar}
            className={`p-1.5 rounded-md transition-colors ${
              isConfigSidebarOpen
                ? 'bg-muted text-foreground'
                : 'text-muted-foreground hover:text-foreground hover:bg-muted'
            }`}
            title="Conversation settings"
          >
            <SlidersHorizontal className="h-3.5 w-3.5" />
          </button>

          <button
            onClick={toggleRightSidebar}
            className={`p-1.5 rounded-md transition-colors ${
              isRightSidebarOpen
                ? 'bg-muted text-foreground'
                : 'text-muted-foreground hover:text-foreground hover:bg-muted'
            }`}
            title="Toggle system monitor"
          >
            <Activity className="h-3.5 w-3.5" />
          </button>
        </div> : null}
    </div>
  );
}
