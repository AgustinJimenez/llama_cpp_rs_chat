import { Folder, File, ArrowLeft, HardDrive } from 'lucide-react';
import React, { useState, useEffect } from 'react';

import type { FileItem } from '../../types';
import { browseFiles } from '../../utils/tauriCommands';
import { Button } from '../atoms/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../atoms/dialog';
// Using regular div with overflow instead of ScrollArea

interface FileBrowserProps {
  isOpen: boolean;
  onClose: () => void;
  onSelectFile: (filePath: string) => void;
  filter?: string; // File extension filter (e.g., '.gguf')
  title?: string;
  mode?: 'file' | 'directory'; // 'directory' shows "Select This Folder" button
  startPath?: string; // Override default starting path
}

// eslint-disable-next-line max-lines-per-function
export const FileBrowser: React.FC<FileBrowserProps> = ({
  isOpen,
  onClose,
  onSelectFile,
  filter = '.gguf',
  title = 'Select Model File',
  mode = 'file',
  startPath,
}) => {
  const [files, setFiles] = useState<FileItem[]>([]);
  const [currentPath, setCurrentPath] = useState<string>(startPath ?? '/app/models');
  const [parentPath, setParentPath] = useState<string | undefined>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);

  const fetchFiles = async (path: string) => {
    setLoading(true);
    setError(null);

    try {
      const data = await browseFiles(path);
      setFiles(data.files);
      setCurrentPath(data.current_path);
      setParentPath(data.parent_path);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to load files';
      setError(errorMessage);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (isOpen) {
      fetchFiles(currentPath);
      setSelectedFile(null);
    }
  }, [isOpen, currentPath]);

  const handleFileClick = (file: FileItem) => {
    if (file.is_directory) {
      setCurrentPath(file.path);
    } else if (file.name.toLowerCase().endsWith(filter.toLowerCase())) {
      setSelectedFile(file.path);
    }
  };

  const handleSelectFile = () => {
    if (selectedFile) {
      onSelectFile(selectedFile);
      onClose();
    }
  };

  const goToParent = () => {
    if (parentPath) {
      setCurrentPath(parentPath);
    }
  };

  const formatFileSize = (bytes?: number) => {
    if (!bytes) return '';
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${sizes[i]}`;
  };

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="max-h-[80vh] max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <HardDrive className="size-5" />
            {title}
          </DialogTitle>
          <DialogDescription>
            {mode === 'directory' && 'Navigate to a folder and click "Select This Folder"'}
            {mode !== 'directory' && `Browse and select a model file (${filter} files)`}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {/* Current Path */}
          <div className="flex items-center gap-2 rounded-md bg-muted p-2">
            <span className="font-mono text-sm">{currentPath}</span>
          </div>

          {/* Navigation */}
          <div className="flex gap-2">
            {!!parentPath && (
              <Button
                variant="outline"
                size="sm"
                onClick={goToParent}
                className="flex items-center gap-2"
              >
                <ArrowLeft className="size-4" />
                Back
              </Button>
            )}
          </div>

          {/* File List */}
          <div className="h-[400px] overflow-y-auto rounded-md border">
            {(() => {
              if (loading) {
                return (
                  <div className="flex items-center justify-center py-8">
                    <div className="text-sm text-muted-foreground">Loading files...</div>
                  </div>
                );
              }
              if (error) {
                return <div className="p-4 text-sm text-destructive">Error: {error}</div>;
              }
              if (files.length === 0) {
                return (
                  <div className="flex items-center justify-center py-8">
                    <div className="text-sm text-muted-foreground">No files found</div>
                  </div>
                );
              }
              return (
                <div className="p-2">
                  {files.map((file) => {
                    const isSelectable =
                      !file.is_directory && file.name.toLowerCase().endsWith(filter.toLowerCase());
                    const isSelected = selectedFile === file.path;

                    return (
                      <div
                        key={file.path}
                        role="button"
                        tabIndex={0}
                        className={`flex cursor-pointer items-center gap-3 rounded-md p-2 transition-colors hover:bg-muted/50 ${
                          isSelected ? 'border border-primary/20 bg-primary/10' : ''
                        } ${
                          !file.is_directory && !isSelectable ? 'cursor-not-allowed opacity-50' : ''
                        }`}
                        onClick={() => handleFileClick(file)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter' || e.key === ' ') handleFileClick(file);
                        }}
                      >
                        {!!file.is_directory && <Folder className="size-4 text-blue-500" />}
                        {!file.is_directory && (
                          <File
                            className={`size-4 ${isSelectable ? 'text-green-500' : 'text-gray-400'}`}
                          />
                        )}
                        <div className="min-w-0 flex-1">
                          <div className="truncate text-sm font-medium">{file.name}</div>
                          {!file.is_directory && !!file.size && (
                            <div className="text-xs text-muted-foreground">
                              {formatFileSize(file.size)}
                            </div>
                          )}
                        </div>
                        {!!isSelected && (
                          <div className="text-xs font-medium text-primary">Selected</div>
                        )}
                      </div>
                    );
                  })}
                </div>
              );
            })()}
          </div>

          {/* Selected File Display */}
          {!!selectedFile && (
            <div className="rounded-md border border-primary/20 bg-primary/5 p-3">
              <div className="text-sm font-medium">Selected file:</div>
              <div className="mt-1 font-mono text-sm text-muted-foreground">{selectedFile}</div>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          {mode === 'directory' && (
            <Button
              onClick={() => {
                onSelectFile(currentPath);
                onClose();
              }}
            >
              Select This Folder
            </Button>
          )}
          {mode !== 'directory' && (
            <Button onClick={handleSelectFile} disabled={!selectedFile}>
              Select File
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
