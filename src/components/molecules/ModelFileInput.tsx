import React from 'react';
import { Loader2, FolderOpen, Clock, CheckCircle, XCircle } from 'lucide-react';
import { Button } from '../atoms/button';

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

export const ModelFileInput: React.FC<ModelFileInputProps> = ({
  modelPath,
  setModelPath,
  fileExists,
  isCheckingFile,
  directoryError,
  directorySuggestions,
  modelHistory,
  showHistory,
  setShowHistory,
  isTauri,
  handleBrowseFile
}) => (
  <div className="space-y-2">
    <div className="flex gap-2">
      <div className="flex-1 relative">
        <input
          type="text"
          data-testid="model-path-input"
          value={modelPath}
          onChange={(e) => setModelPath(e.target.value.replace(/"/g, ''))}
          onFocus={() => setShowHistory(true)}
          onBlur={() => setTimeout(() => setShowHistory(false), 200)}
          placeholder={isTauri ? "Select a .gguf file or enter full path" : "Enter full path to .gguf file (e.g., C:\\path\\to\\model.gguf)"}
          className={`w-full px-3 py-2 pr-8 text-sm border rounded-md bg-background ${
            fileExists === true ? 'border-green-500' :
            fileExists === false ? 'border-red-500' :
            'border-input'
          }`}
        />
        {modelPath.trim() && (
          <div className="absolute right-2 top-1/2 transform -translate-y-1/2">
            {isCheckingFile ? (
              <Clock className="h-4 w-4 text-muted-foreground animate-pulse" />
            ) : fileExists === true ? (
              <CheckCircle className="h-4 w-4 text-green-500" />
            ) : fileExists === false ? (
              <XCircle className="h-4 w-4 text-red-500" />
            ) : null}
          </div>
        )}
        {/* Model History Suggestions */}
        {showHistory && modelHistory.length > 0 && !modelPath.trim() && (
          <div className="absolute z-10 w-full mt-1 bg-background border border-input rounded-md shadow-lg max-h-60 overflow-y-auto">
            <div className="p-2 text-xs text-muted-foreground border-b">
              Previously used paths:
            </div>
            {modelHistory.map((path, idx) => (
              <button
                key={idx}
                type="button"
                onClick={() => {
                  setModelPath(path);
                  setShowHistory(false);
                }}
                className="block w-full text-left px-3 py-2 text-sm hover:bg-accent transition-colors border-b last:border-b-0"
              >
                <div className="font-mono text-xs truncate">{path}</div>
              </button>
            ))}
          </div>
        )}
      </div>
      {isTauri && (
        <Button
          type="button"
          onClick={handleBrowseFile}
          disabled={isCheckingFile}
          variant="outline"
          className="flex items-center gap-2 px-3"
        >
          {isCheckingFile ? (
            <>
              <Loader2 className="h-4 w-4 animate-spin" />
              Reading...
            </>
          ) : (
            <>
              <FolderOpen className="h-4 w-4" />
              Browse
            </>
          )}
        </Button>
      )}
    </div>

    {!isTauri && (
      <p className="text-xs text-muted-foreground">
        📝 Web mode: Please enter the full file path manually (e.g., C:\Users\Name\Documents\model.gguf)
      </p>
    )}

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
                        📄 {suggestion}
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
  </div>
);
