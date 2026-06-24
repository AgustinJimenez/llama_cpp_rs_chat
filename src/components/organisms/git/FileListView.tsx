import { ChevronDown, ChevronRight, FolderTree, List, Minus, Pencil, Plus } from 'lucide-react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { FILE_STATUS_CLS, STATUS_ORDER } from './constants';
import type { DirTree, FileChange, FileListMode, FlatTreeItem } from './types';
import { flattenDirTree, insertPath } from './utils';

const TREE_INDENT_BASE = 8;
const TREE_INDENT_PER_DEPTH = 10;

type StatusIconCmp = React.ComponentType<{ className?: string }>;
const FILE_STATUS_ICON: Record<string, StatusIconCmp> = { M: Pencil, A: Plus, D: Minus, R: Pencil };

interface FileListViewProps {
  files: FileChange[];
  filesLoading: boolean;
  onSelectFile: (file: string) => void;
}

export const FileListView: React.FC<FileListViewProps> = ({ files, filesLoading, onSelectFile }) => {
  const { t } = useTranslation();
  const [fileListMode, setFileListMode] = useState<FileListMode>(
    () => (localStorage.getItem('gitGraphFileListMode') as FileListMode | null) ?? 'path',
  );
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

  useEffect(() => {
    setCollapsed(new Set());
  }, [files]);

  const toggleFolder = useCallback((folderPath: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(folderPath)) next.delete(folderPath);
      else next.add(folderPath);
      return next;
    });
  }, []);

  const handleFileListMode = useCallback((mode: FileListMode) => {
    setFileListMode(mode);
    localStorage.setItem('gitGraphFileListMode', mode);
  }, []);

  const treeItems = useMemo<FlatTreeItem[]>(() => {
    if (fileListMode !== 'tree') return [];
    const dirTree: DirTree = new Map();
    for (const f of files) insertPath(dirTree, f.path.split('/'), f);
    return flattenDirTree(dirTree, 0, collapsed, '');
  }, [files, fileListMode, collapsed]);

  const fileSummary = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const f of files) {
      const s = f.status[0] ?? '?';
      counts[s] = (counts[s] ?? 0) + 1;
    }
    return counts;
  }, [files]);

  const filesLabel = filesLoading
    ? t('gitGraph.loadingFiles')
    : `${t('gitGraph.filesChanged')} (${files.length})`;

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden border-t border-border/40">
      <div className="flex shrink-0 items-center gap-1 px-3 py-1.5">
        <p className="flex-1 text-xs font-semibold uppercase tracking-widest text-muted-foreground/50">
          {filesLabel}
        </p>
        <div className="flex items-center overflow-hidden rounded border border-border/40">
          <button
            type="button"
            onClick={() => handleFileListMode('path')}
            className={`px-1.5 py-0.5 ${fileListMode === 'path' ? 'bg-muted text-foreground' : 'text-muted-foreground hover:bg-muted/50'}`}
            title={t('gitGraph.fileViewPath')}
            aria-label={t('gitGraph.fileViewPath')}
          >
            <List className="size-3" />
          </button>
          <button
            type="button"
            onClick={() => handleFileListMode('tree')}
            className={`px-1.5 py-0.5 ${fileListMode === 'tree' ? 'bg-muted text-foreground' : 'text-muted-foreground hover:bg-muted/50'}`}
            title={t('gitGraph.fileViewTree')}
            aria-label={t('gitGraph.fileViewTree')}
          >
            <FolderTree className="size-3" />
          </button>
        </div>
      </div>
      {files.length > 0 && (
        <div className="flex shrink-0 flex-wrap gap-x-3 gap-y-0.5 border-b border-border/40 px-3 pb-2 pt-1">
          {STATUS_ORDER.filter((s) => fileSummary[s]).map((s) => {
            const Icon = FILE_STATUS_ICON[s];
            const cls = FILE_STATUS_CLS[s] ?? 'text-muted-foreground';
            return (
              <span key={s} className={`flex items-center gap-1 text-sm ${cls}`}>
                <Icon className="size-3 shrink-0" />
                <span>{fileSummary[s]}</span>
              </span>
            );
          })}
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {fileListMode === 'path' &&
          files.map((f) => {
            const s = f.status[0] ?? '?';
            const Icon = FILE_STATUS_ICON[s];
            const cls = FILE_STATUS_CLS[s] ?? 'text-muted-foreground';
            return (
              <button
                key={f.path}
                type="button"
                onClick={() => onSelectFile(f.path)}
                className="flex w-full items-center gap-2 rounded px-3 py-0.5 text-left hover:bg-muted/50"
              >
                <Icon className={`size-3 shrink-0 ${cls}`} />
                <span className="min-w-0 truncate text-sm text-foreground/70">{f.path}</span>
              </button>
            );
          })}
        {fileListMode === 'tree' &&
          treeItems.map((item) => {
            if (item.isFolder) {
              const FolderChevron = collapsed.has(item.fullPath) ? ChevronRight : ChevronDown;
              const folderCounts = STATUS_ORDER.filter((s) => (item.statusCounts ?? {})[s]).map((s) => {
                const Icon = FILE_STATUS_ICON[s];
                const cls = FILE_STATUS_CLS[s] ?? 'text-muted-foreground';
                return (
                  <span key={s} className={`flex items-center gap-0.5 ${cls}`}>
                    <Icon className="size-2.5" />
                    <span className="text-[10px] leading-none">{(item.statusCounts ?? {})[s]}</span>
                  </span>
                );
              });
              return (
                <button
                  key={`folder-${item.fullPath}`}
                  type="button"
                  onClick={() => toggleFolder(item.fullPath)}
                  style={{ paddingLeft: item.depth * TREE_INDENT_PER_DEPTH + TREE_INDENT_BASE }}
                  className="flex w-full items-center gap-1 py-0.5 pr-2 text-muted-foreground hover:bg-muted/50"
                >
                  <FolderChevron className="size-3 shrink-0" />
                  <span className="min-w-0 flex-1 truncate text-sm">{item.name}</span>
                  {collapsed.has(item.fullPath) && (
                    <span className="ml-1 flex shrink-0 items-center gap-1.5">{folderCounts}</span>
                  )}
                </button>
              );
            }
            const s = item.file?.status[0] ?? '?';
            const Icon = FILE_STATUS_ICON[s];
            const cls = FILE_STATUS_CLS[s] ?? 'text-muted-foreground';
            return (
              <button
                key={`file-${item.fullPath}`}
                type="button"
                onClick={() => onSelectFile(item.fullPath)}
                style={{ paddingLeft: item.depth * TREE_INDENT_PER_DEPTH + TREE_INDENT_BASE }}
                className="flex w-full items-center gap-1.5 py-0.5 pr-2 hover:bg-muted/50"
              >
                <Icon className={`size-3 shrink-0 ${cls}`} />
                <span className="min-w-0 truncate text-sm text-foreground/70">{item.name}</span>
              </button>
            );
          })}
      </div>
    </div>
  );
};
