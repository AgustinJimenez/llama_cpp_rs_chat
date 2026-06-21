import {
  Loader2,
  FolderOpen,
  Clock,
  CheckCircle,
  XCircle,
  ChevronDown,
  ChevronRight,
} from 'lucide-react';
import React, { useState } from 'react';

export interface ModelFileInputProps {
  modelPath: string;
  setModelPath: (path: string) => void;
  fileExists: boolean | null;
  isCheckingFile: boolean;
  directoryError: string | null;
  directorySuggestions: string[];
  modelHistory: string[];
  showHistory: boolean;
  setShowHistory: (show: boolean) => void;
  isTauri: boolean;
  handleBrowseFile: () => void;
}

// eslint-disable-next-line max-lines-per-function, complexity
export const ModelFileInput: React.FC<ModelFileInputProps> = ({
  modelPath,
  setModelPath,
  fileExists,
  isCheckingFile,
  directoryError,
  directorySuggestions,
  modelHistory,
  showHistory: _showHistory,
  setShowHistory: _setShowHistory,
  isTauri: _isTauri,
  handleBrowseFile,
}) => {
  const [historyExpanded, setHistoryExpanded] = useState(false);
  const historyChevron = historyExpanded ? (
    <ChevronDown className="size-3" />
  ) : (
    <ChevronRight className="size-3" />
  );

  let borderClass = 'border-input';
  if (fileExists === true) borderClass = 'border-green-500';
  else if (fileExists === false) borderClass = 'border-red-500';

  const fileIcon = isCheckingFile ? (
    <Loader2 className="size-4 flex-shrink-0 animate-spin" />
  ) : (
    <FolderOpen className="size-4 flex-shrink-0 text-foreground" />
  );
  const pathLabel = modelPath ? (
    <span className="truncate font-mono text-xs">{modelPath}</span>
  ) : (
    <span className="text-foreground/60">Click to select a .gguf model file...</span>
  );
  const buttonStateClass = isCheckingFile
    ? 'opacity-60'
    : 'cursor-pointer hover:bg-accent/50 transition-colors';

  return (
    <div className="space-y-2">
      <div className="relative">
        <button
          type="button"
          data-testid="model-path-input"
          onClick={handleBrowseFile}
          disabled={isCheckingFile}
          className={`flex w-full items-center gap-2 rounded-md border bg-background px-3 py-2 pr-8 text-left text-sm ${borderClass} ${buttonStateClass}`}
        >
          {fileIcon}
          {pathLabel}
        </button>
        {modelPath.trim() && (
          <div className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 transform">
            {!!isCheckingFile && <Clock className="size-4 animate-pulse text-muted-foreground" />}
            {!isCheckingFile && fileExists === true && (
              <CheckCircle className="size-4 text-green-500" />
            )}
            {!isCheckingFile && fileExists === false && <XCircle className="size-4 text-red-500" />}
          </div>
        )}
      </div>

      {/* File existence status */}
      {modelPath.trim() && (
        <div className="space-y-2 text-xs">
          {!!isCheckingFile && (
            <span
              className="flex items-center gap-1 text-muted-foreground"
              data-testid="file-checking"
            >
              <Clock className="size-3" />
              Checking file...
            </span>
          )}
          {!isCheckingFile && fileExists === true && (
            <span
              className="flex items-center gap-1 text-green-600"
              data-testid="file-found-label"
              id="file-found-label"
            >
              <CheckCircle className="size-3" />
              File found and accessible
            </span>
          )}
          {!isCheckingFile && fileExists === false && !!directoryError && (
            <div className="space-y-2">
              <span className="flex items-center gap-1 text-amber-600">
                <XCircle className="size-3" />
                {directoryError}
              </span>
              {directorySuggestions.length > 0 && (
                <div className="space-y-1 pl-4">
                  {directorySuggestions.map((suggestion) => (
                    <button
                      key={suggestion}
                      type="button"
                      onClick={() => {
                        const basePath = modelPath.trim();
                        const newPath =
                          basePath.endsWith('\\') || basePath.endsWith('/')
                            ? `${basePath}${suggestion}`
                            : `${basePath}\\${suggestion}`;
                        setModelPath(newPath);
                      }}
                      className="block w-full rounded border border-border bg-muted px-3 py-2 text-left text-xs transition-colors hover:bg-accent"
                    >
                      {suggestion}
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}
          {!isCheckingFile && fileExists === false && !directoryError && (
            <span className="flex items-center gap-1 text-red-600">
              <XCircle className="size-3" />
              File not found or inaccessible
            </span>
          )}
        </div>
      )}

      {/* Model history — collapsible below input */}
      {modelHistory.length > 0 && (
        <div className="overflow-hidden rounded-md border border-input">
          <button
            type="button"
            onClick={() => setHistoryExpanded(!historyExpanded)}
            className="flex w-full items-center gap-1.5 bg-muted/50 px-3 py-1.5 text-xs text-foreground transition-colors hover:bg-muted"
          >
            {historyChevron}
            <Clock className="size-3" />
            Recent models ({modelHistory.length})
          </button>
          {!!historyExpanded &&
            modelHistory.map((path) => (
              <button
                key={path}
                type="button"
                onClick={() => setModelPath(path)}
                className="block w-full border-t border-input px-3 py-2 text-left text-sm transition-colors hover:bg-accent"
              >
                <div className="truncate font-mono text-xs">{path}</div>
              </button>
            ))}
        </div>
      )}
    </div>
  );
};
