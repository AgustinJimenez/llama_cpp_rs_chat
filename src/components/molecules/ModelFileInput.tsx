import React, { useState } from 'react';
import { Loader2, FolderOpen, Clock, CheckCircle, XCircle, ChevronDown, ChevronRight } from 'lucide-react';

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
  handleBrowseFile
}) => {
  const [historyExpanded, setHistoryExpanded] = useState(false);

  return (
  <div className="space-y-2">
    <div className="relative">
      <button
        type="button"
        data-testid="model-path-input"
        onClick={handleBrowseFile}
        disabled={isCheckingFile}
        className={`w-full px-3 py-2 pr-8 text-sm border rounded-md bg-background text-left flex items-center gap-2 ${
          fileExists === true ? 'border-green-500' :
          fileExists === false ? 'border-red-500' :
          'border-input'
        } ${isCheckingFile ? 'opacity-60' : 'cursor-pointer hover:bg-accent/50 transition-colors'}`}
      >
        {isCheckingFile ? (
          <Loader2 className="h-4 w-4 animate-spin flex-shrink-0" />
        ) : (
          <FolderOpen className="h-4 w-4 text-muted-foreground flex-shrink-0" />
        )}
        {modelPath ? (
          <span className="font-mono text-xs truncate">{modelPath}</span>
        ) : (
          <span className="text-muted-foreground">Click to select a .gguf model file...</span>
        )}
      </button>
      {modelPath.trim() && (
        <div className="absolute right-2 top-1/2 transform -translate-y-1/2 pointer-events-none">
          {isCheckingFile ? (
            <Clock className="h-4 w-4 text-muted-foreground animate-pulse" />
          ) : fileExists === true ? (
            <CheckCircle className="h-4 w-4 text-green-500" />
          ) : fileExists === false ? (
            <XCircle className="h-4 w-4 text-red-500" />
          ) : null}
        </div>
      )}
    </div>

    {/* File existence status */}
    {modelPath.trim() && (
      <div className="text-xs space-y-2">
        {isCheckingFile ? (
          <span className="text-muted-foreground flex items-center gap-1" data-testid="file-checking">
            <Clock className="h-3 w-3" />
            Checking file...
          </span>
        ) : fileExists === true ? (
          <span className="text-green-600 flex items-center gap-1" data-testid="file-found-label" id="file-found-label">
            <CheckCircle className="h-3 w-3" />
            File found and accessible
          </span>
        ) : fileExists === false ? (
          <>
            {directoryError ? (
              <div className="space-y-2">
                <span className="text-amber-600 flex items-center gap-1">
                  <XCircle className="h-3 w-3" />
                  {directoryError}
                </span>
                {directorySuggestions.length > 0 && (
                  <div className="pl-4 space-y-1">
                    {directorySuggestions.map((suggestion, idx) => (
                      <button
                        key={idx}
                        type="button"
                        onClick={() => {
                          const basePath = modelPath.trim();
                          const newPath = basePath.endsWith('\\') || basePath.endsWith('/')
                            ? `${basePath}${suggestion}`
                            : `${basePath}\\${suggestion}`;
                          setModelPath(newPath);
                        }}
                        className="block w-full text-left px-3 py-2 text-xs bg-muted hover:bg-accent rounded border border-border transition-colors"
                      >
                        {suggestion}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            ) : (
              <span className="text-red-600 flex items-center gap-1">
                <XCircle className="h-3 w-3" />
                File not found or inaccessible
              </span>
            )}
          </>
        ) : null}
      </div>
    )}

    {/* Model history — collapsible below input */}
    {modelHistory.length > 0 ? (
      <div className="border border-input rounded-md overflow-hidden">
        <button
          type="button"
          onClick={() => setHistoryExpanded(!historyExpanded)}
          className="w-full px-3 py-1.5 text-xs text-muted-foreground bg-muted/50 flex items-center gap-1.5 hover:bg-muted transition-colors"
        >
          {historyExpanded ? (
            <ChevronDown className="h-3 w-3" />
          ) : (
            <ChevronRight className="h-3 w-3" />
          )}
          <Clock className="h-3 w-3" />
          Recent models ({modelHistory.length})
        </button>
        {historyExpanded ? modelHistory.map((path, idx) => (
          <button
            key={idx}
            type="button"
            onClick={() => setModelPath(path)}
            className="block w-full text-left px-3 py-2 text-sm hover:bg-accent transition-colors border-t border-input"
          >
            <div className="font-mono text-xs truncate">{path}</div>
          </button>
        )) : null}
      </div>
    ) : null}
  </div>
  );
};
