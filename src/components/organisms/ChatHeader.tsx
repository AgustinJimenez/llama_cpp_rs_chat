import { Unplug, Activity } from 'lucide-react';
import type { ViewMode } from '../../types';

interface ChatHeaderProps {
  modelLoaded: boolean;
  modelPath?: string;
  isModelLoading: boolean;
  tokensUsed?: number;
  maxTokens?: number;
  genTokPerSec?: number;
  viewMode: ViewMode;
  isRightSidebarOpen: boolean;
  onModelUnload: () => void;
  onForceUnload: () => void;
  hasStatusError: boolean;
  onViewModeChange: (mode: ViewMode) => void;
  onToggleRightSidebar: () => void;
}

export function ChatHeader({
  modelLoaded,
  modelPath,
  isModelLoading,
  tokensUsed,
  maxTokens,
  genTokPerSec,
  viewMode,
  isRightSidebarOpen,
  onModelUnload,
  onForceUnload,
  hasStatusError,
  onViewModeChange,
  onToggleRightSidebar,
}: ChatHeaderProps) {
  const modelName = (() => {
    const fullPath = modelPath || '';
    const fileName = fullPath.split(/[/\\]/).pop() || '';
    return fileName.replace(/\.gguf$/i, '');
  })();

  return (
    <div className="flex items-center justify-between px-4 py-2 border-b border-border" data-testid="chat-header">
      {/* Left: model name */}
      <div className="flex items-center gap-2 min-w-0">
        {modelLoaded && (
          <>
            <span className="text-sm font-medium truncate">{modelName}</span>
            <button
              onClick={onModelUnload}
              disabled={isModelLoading}
              className="p-1.5 rounded-md border border-border text-muted-foreground hover:text-destructive hover:border-destructive/50 hover:bg-destructive/10 transition-colors disabled:opacity-50"
              title="Unload model"
            >
              <Unplug className="h-4 w-4" />
            </button>
          </>
        )}
        {!modelLoaded && hasStatusError && (
          <>
            <span className="text-sm text-destructive">Model status unknown</span>
            <button
              onClick={onForceUnload}
              disabled={isModelLoading}
              className="p-1.5 rounded-md border border-border text-muted-foreground hover:text-destructive hover:border-destructive/50 hover:bg-destructive/10 transition-colors disabled:opacity-50"
              title="Force unload"
            >
              <Unplug className="h-4 w-4" />
            </button>
          </>
        )}
      </div>

      {/* Right: context + view toggle + monitor */}
      {modelLoaded && (
        <div className="flex items-center gap-3">
          {tokensUsed !== undefined && maxTokens !== undefined && (
            <span className="text-xs text-muted-foreground font-mono">
              {tokensUsed}/{maxTokens}
            </span>
          )}
          {genTokPerSec !== undefined && genTokPerSec > 0 && (
            <span className="text-xs text-muted-foreground font-mono" title="Generation speed">
              {genTokPerSec.toFixed(1)} tok/s
            </span>
          )}

          <div className="flex items-center bg-muted rounded-md p-0.5">
            <button
              onClick={() => onViewModeChange('markdown')}
              className={`px-2.5 py-1 text-xs font-medium rounded transition-colors ${
                viewMode === 'markdown'
                  ? 'bg-card text-foreground'
                  : 'text-muted-foreground hover:text-foreground'
              }`}
              title="Markdown view"
            >
              MD
            </button>
            <button
              onClick={() => onViewModeChange('text')}
              className={`px-2.5 py-1 text-xs font-medium rounded transition-colors ${
                viewMode === 'text'
                  ? 'bg-card text-foreground'
                  : 'text-muted-foreground hover:text-foreground'
              }`}
              title="Plain text view"
            >
              TXT
            </button>
            <button
              onClick={() => onViewModeChange('raw')}
              className={`px-2.5 py-1 text-xs font-medium rounded transition-colors ${
                viewMode === 'raw'
                  ? 'bg-card text-foreground'
                  : 'text-muted-foreground hover:text-foreground'
              }`}
              title="Raw model output (no parsing)"
            >
              RAW
            </button>
          </div>

          <button
            onClick={onToggleRightSidebar}
            className={`p-1.5 rounded-md transition-colors ${
              isRightSidebarOpen
                ? 'bg-muted text-foreground'
                : 'text-muted-foreground hover:text-foreground hover:bg-muted'
            }`}
            title="Toggle system monitor"
          >
            <Activity className="h-3.5 w-3.5" />
          </button>
        </div>
      )}
    </div>
  );
}
