import React, { useState, useEffect } from 'react';
import { Folder, File, ArrowLeft, HardDrive } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../atoms/dialog';
import { Button } from '../atoms/button';
// Using regular div with overflow instead of ScrollArea
import type { FileItem, BrowseFilesResponse } from '../../types';

interface FileBrowserProps {
  isOpen: boolean;
  onClose: () => void;
  onSelectFile: (filePath: string) => void;
  filter?: string; // File extension filter (e.g., '.gguf')
  title?: string;
}

export const FileBrowser: React.FC<FileBrowserProps> = ({ 
  isOpen, 
  onClose, 
  onSelectFile, 
  filter = '.gguf',
  title = 'Select Model File'
}) => {
  const [files, setFiles] = useState<FileItem[]>([]);
  const [currentPath, setCurrentPath] = useState<string>('/app/models');
  const [parentPath, setParentPath] = useState<string | undefined>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);

  const fetchFiles = async (path: string) => {
    setLoading(true);
    setError(null);
    
    try {
      const response = await fetch(`/api/browse?path=${encodeURIComponent(path)}`);
      if (!response.ok) {
        throw new Error('Failed to browse files');
      }
      
      const data: BrowseFilesResponse = await response.json();
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
      <DialogContent className="max-w-2xl max-h-[80vh]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <HardDrive className="h-5 w-5" />
            {title}
          </DialogTitle>
          <DialogDescription>
            Browse and select a model file ({filter} files)
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {/* Current Path */}
          <div className="flex items-center gap-2 p-2 bg-muted rounded-md">
            <span className="text-sm font-mono">{currentPath}</span>
          </div>

          {/* Navigation */}
          <div className="flex gap-2">
            {parentPath && (
              <Button
                variant="outline"
                size="sm"
                onClick={goToParent}
                className="flex items-center gap-2"
              >
                <ArrowLeft className="h-4 w-4" />
                Back
              </Button>
            )}
          </div>

          {/* File List */}
          <div className="h-[400px] border rounded-md overflow-y-auto">
            {loading ? (
              <div className="flex items-center justify-center py-8">
                <div className="text-sm text-muted-foreground">Loading files...</div>
              </div>
            ) : error ? (
              <div className="p-4 text-sm text-destructive">
                Error: {error}
              </div>
            ) : files.length === 0 ? (
              <div className="flex items-center justify-center py-8">
                <div className="text-sm text-muted-foreground">No files found</div>
              </div>
            ) : (
              <div className="p-2">
                {files.map((file, index) => {
                  const isSelectable = !file.is_directory && file.name.toLowerCase().endsWith(filter.toLowerCase());
                  const isSelected = selectedFile === file.path;
                  
                  return (
                    <div
                      key={index}
                      className={`flex items-center gap-3 p-2 rounded-md cursor-pointer hover:bg-muted/50 transition-colors ${
                        isSelected ? 'bg-primary/10 border border-primary/20' : ''
                      } ${
                        !file.is_directory && !isSelectable ? 'opacity-50 cursor-not-allowed' : ''
                      }`}
                      onClick={() => handleFileClick(file)}
                    >
                      {file.is_directory ? (
                        <Folder className="h-4 w-4 text-blue-500" />
                      ) : (
                        <File className={`h-4 w-4 ${isSelectable ? 'text-green-500' : 'text-gray-400'}`} />
                      )}
                      <div className="flex-1 min-w-0">
                        <div className="text-sm font-medium truncate">
                          {file.name}
                        </div>
                        {!file.is_directory && file.size && (
                          <div className="text-xs text-muted-foreground">
                            {formatFileSize(file.size)}
                          </div>
                        )}
                      </div>
                      {isSelected && (
                        <div className="text-xs text-primary font-medium">
                          Selected
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>

          {/* Selected File Display */}
          {selectedFile && (
            <div className="p-3 bg-primary/5 border border-primary/20 rounded-md">
              <div className="text-sm font-medium">Selected file:</div>
              <div className="text-sm font-mono text-muted-foreground mt-1">
                {selectedFile}
              </div>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button 
            onClick={handleSelectFile} 
            disabled={!selectedFile}
          >
            Select File
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};