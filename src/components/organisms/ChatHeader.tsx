import { Unplug, Activity, Loader2 } from 'lucide-react';
import type { ViewMode } from '../../types';
import type { LoadingAction } from '../../hooks/useModel';

interface ChatHeaderProps {
  isSidebarOpen: boolean;
  modelLoaded: boolean;
  modelPath?: string;
  isModelLoading: boolean;
  loadingAction: LoadingAction;
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
  loadingAction,
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
              className="flat-button bg-destructive text-white px-4 py-2 disabled:opacity-50 flex items-center gap-2"
              title={loadingAction === 'unloading' ? 'Unloading model...' : 'Unload model'}
            >
              {loadingAction === 'unloading' ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin" />
                  <span>Unloading...</span>
                </>
              ) : (
                <Unplug className="h-4 w-4" />
              )}
            </button>
          </div>
        ) : hasStatusError ? (
          <div className="flex items-center gap-3">
            <p className="text-sm font-semibold text-destructive">Model status unknown</p>
            <button
              onClick={onForceUnload}
              disabled={isModelLoading}
              className="flat-button bg-destructive text-white px-4 py-2 disabled:opacity-50 flex items-center gap-2"
              title={loadingAction === 'unloading' ? 'Force unloading...' : 'Force unload backend'}
            >
              {loadingAction === 'unloading' ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin" />
                  <span>Unloading...</span>
                </>
              ) : (
                <Unplug className="h-4 w-4" />
              )}
            </button>
          </div>
        ) : null}
      </div>

      {/* Right section */}
      <div className="flex items-center gap-4">
        {/* Context/Tokens Display - show when model loaded and we have token data */}
        {modelLoaded && tokensUsed !== undefined && maxTokens !== undefined && (
          <div className="flex items-center gap-2 text-sm font-medium text-muted-foreground">
            <span>Context:</span>
            <span className="font-mono px-2 py-1 bg-muted rounded text-foreground">{tokensUsed}</span>
            <span>/</span>
            <span className="font-mono px-2 py-1 bg-muted rounded text-foreground">{maxTokens}</span>
            <span>tokens</span>
          </div>
        )}

        {/* View Mode Toggle - show whenever there are messages (even if no model loaded) */}
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

        {/* System Monitor Toggle Button - only show when model is loaded */}
        {modelLoaded && (
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
        )}
      </div>
    </div>
  );
}
