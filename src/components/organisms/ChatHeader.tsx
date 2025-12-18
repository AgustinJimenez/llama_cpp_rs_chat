import { Unplug, Activity } from 'lucide-react';
import type { ViewMode } from '../../types';

interface ChatHeaderProps {
  isSidebarOpen: boolean;
  modelLoaded: boolean;
  modelPath?: string;
  isModelLoading: boolean;
  messagesLength: number;
  tokensUsed?: number;
  maxTokens?: number;
  viewMode: ViewMode;
  isRightSidebarOpen: boolean;
  onToggleSidebar: () => void;
  onModelUnload: () => void;
  onForceUnload: () => void;
  hasStatusError: boolean;
  onViewModeChange: (mode: ViewMode) => void;
  onToggleRightSidebar: () => void;
}

// eslint-disable-next-line max-lines-per-function
export function ChatHeader({
  modelLoaded,
  modelPath,
  isModelLoading,
  messagesLength,
  tokensUsed,
  maxTokens,
  viewMode,
  isRightSidebarOpen,
  onToggleSidebar,
  onModelUnload,
  onForceUnload,
  hasStatusError,
  onViewModeChange,
  onToggleRightSidebar,
}: ChatHeaderProps) {
  return (
    <div className="flex items-center justify-between px-6 py-3 flat-header" data-testid="chat-header">
      <div className="flex items-center gap-4">
        <button
          onClick={onToggleSidebar}
          className="md:hidden p-2 hover:bg-muted rounded-lg"
          data-testid="mobile-sidebar-toggle"
        >
          ☰
        </button>
      </div>

      <div className="flex-1 flex justify-center items-center gap-6">
        {modelLoaded ? (
          <div className="flex items-center gap-3">
            <p className="text-lg font-semibold">
              {(() => {
                const fullPath = modelPath || '';
                const fileName = fullPath.split(/[/\\]/).pop() || 'Model loaded';
                // Remove .gguf extension if present
                return fileName.replace(/\.gguf$/i, '');
              })()}
            </p>
            <button
              onClick={onModelUnload}
              disabled={isModelLoading}
              className="flat-button bg-destructive text-white px-4 py-2 disabled:opacity-50"
              title="Unload model"
            >
              <Unplug className="h-4 w-4" />
            </button>
          </div>
        ) : hasStatusError ? (
          <div className="flex items-center gap-3">
            <p className="text-sm font-semibold text-destructive">Model status unknown</p>
            <button
              onClick={onForceUnload}
              disabled={isModelLoading}
              className="flat-button bg-destructive text-white px-4 py-2 disabled:opacity-50"
              title="Force unload backend"
            >
              <Unplug className="h-4 w-4" />
            </button>
          </div>
        ) : null}
      </div>

      {/* Right section - always show if model loaded and we have data */}
      {modelLoaded && (
        <div className="flex items-center gap-4">
          {/* Context/Tokens Display - show when we have token data */}
          {tokensUsed !== undefined && maxTokens !== undefined && (
            <div className="flex items-center gap-2 text-sm font-medium text-muted-foreground">
              <span>Context:</span>
              <span className="font-mono px-2 py-1 bg-muted rounded text-foreground">{tokensUsed}</span>
              <span>/</span>
              <span className="font-mono px-2 py-1 bg-muted rounded text-foreground">{maxTokens}</span>
              <span>tokens</span>
            </div>
          )}

          {/* WebSocket Connection Status */}
          {/* View Mode Toggle - only show when there are messages */}
          {messagesLength > 0 && (
            <div className="flex items-center gap-2 bg-muted rounded-lg p-1">
              <button
                onClick={() => onViewModeChange('markdown')}
                className={`px-4 py-2 font-medium text-sm transition-all rounded-md ${
                  viewMode === 'markdown'
                    ? 'bg-primary text-white'
                    : 'border border-primary hover:bg-background'
                }`}
                title="Markdown view"
              >
                Markdown
              </button>
              <button
                onClick={() => onViewModeChange('text')}
                className={`px-4 py-2 font-medium text-sm transition-all rounded-md ${
                  viewMode === 'text'
                    ? 'bg-primary text-white'
                    : 'border border-primary hover:bg-background'
                }`}
                title="Plain text view"
              >
                Plain Text
              </button>
            </div>
          )}

          {/* System Monitor Toggle Button */}
          <button
            onClick={onToggleRightSidebar}
            className={`p-2 rounded-lg transition-all ${
              isRightSidebarOpen
                ? 'bg-primary text-white'
                : 'bg-muted hover:bg-muted/80 border border-border'
            }`}
            title="Toggle system monitor"
          >
            <Activity className="h-5 w-5" />
          </button>
        </div>
      )}
    </div>
  );
}
