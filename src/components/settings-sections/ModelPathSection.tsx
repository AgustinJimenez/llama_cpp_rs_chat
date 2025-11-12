import React from 'react';
import { FolderOpen } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';

interface ModelPathSectionProps {
  modelPath: string;
  onModelPathChange: (path: string) => void;
  onBrowseClick: () => void;
}

export const ModelPathSection: React.FC<ModelPathSectionProps> = ({
  modelPath,
  onModelPathChange,
  onBrowseClick
}) => {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm">Model Path</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex gap-2">
          <input
            type="text"
            value={modelPath || '/app/models/'}
            onChange={(e) => onModelPathChange(e.target.value)}
            placeholder="/app/models/model.gguf"
            className="flex-1 px-3 py-2 text-sm border border-input rounded-md bg-background"
            readOnly
          />
          <button
            type="button"
            onClick={onBrowseClick}
            className="flat-button bg-flat-red text-white flex items-center gap-2 px-3"
          >
            <FolderOpen className="h-4 w-4" />
            Browse
          </button>
        </div>
        <p className="text-xs text-muted-foreground">
          Select a GGUF model file for LLaMA inference
        </p>
      </CardContent>
    </Card>
  );
};
