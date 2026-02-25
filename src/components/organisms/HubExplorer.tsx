import React, { useState, useEffect, useRef, useCallback } from 'react';
import { Search, Loader2, ExternalLink, ChevronDown, ChevronRight, Download, Heart, ArrowUpDown, ArrowDownToLine } from 'lucide-react';
import {
  Dialog, DialogContent, DialogHeader, DialogTitle,
} from '../atoms/dialog';
import { Button } from '../atoms/button';
import { fetchHubTree, startHubDownload, verifyHubDownloads, pickDirectory } from '@/utils/tauriCommands';
import { useHubSearch } from '@/hooks/useHubSearch';
import type { HubModel, HubSortField } from '@/hooks/useHubSearch';
import type { HubFile, DownloadProgress, HubDownloadRecord } from '@/utils/tauriCommands';

interface HubExplorerProps {
  isOpen: boolean;
  onClose: () => void;
}

const SORT_OPTIONS: { value: HubSortField; label: string }[] = [
  { value: 'downloads', label: 'Downloads' },
  { value: 'likes', label: 'Likes' },
  { value: 'lastModified', label: 'Recently Updated' },
  { value: 'createdAt', label: 'Newest' },
];

function formatSize(bytes: number): string {
  if (bytes === 0) return '\u2014';
  const gb = bytes / 1_073_741_824;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  const mb = bytes / 1_048_576;
  if (mb >= 1) return `${mb.toFixed(0)} MB`;
  const kb = bytes / 1024;
  return `${kb.toFixed(0)} KB`;
}

function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

/** Extract quantization type from a GGUF filename */
function extractQuant(filename: string): string | null {
  // Match patterns like Q4_K_M, IQ3_XS, Q8_0, F16, BF16, MXFP4, etc.
  const base = filename.replace(/\.gguf$/i, '').split('/').pop() ?? '';
  // Multi-part files: strip -00001-of-00002, and strip -imat suffix
  const stripped = base.replace(/-\d{5}-of-\d{5}$/, '').replace(/-imat$/i, '');
  // Match last quant-like segment after a dash or underscore
  const m = stripped.match(/[-_]((?:IQ|Q|F|BF|MXFP)\d[A-Z0-9_]*?)$/i);
  if (m) return m[1].toUpperCase();
  // Also match standalone patterns at end
  const m2 = stripped.match(/((?:IQ|Q|F|BF|MXFP)\d[A-Z0-9_]*)$/i);
  if (m2) return m2[1].toUpperCase();
  return null;
}

/** Classify file type */
function fileType(name: string): 'mmproj' | 'imatrix' | 'model' {
  const lower = name.toLowerCase();
  if (lower.includes('mmproj')) return 'mmproj';
  if (lower.includes('imatrix')) return 'imatrix';
  return 'model';
}

// eslint-disable-next-line complexity
function FileRow({ file, modelId, onDownload, progress, persistedDone, pendingRecord }: {
  file: HubFile;
  modelId: string;
  onDownload: (modelId: string, file: HubFile, resumeDest?: string) => void;
  progress?: DownloadProgress | null;
  persistedDone?: boolean;
  pendingRecord?: HubDownloadRecord | null;
}) {
  const quant = extractQuant(file.name);
  const type = fileType(file.name);
  const shortName = file.name.split('/').pop() ?? file.name;
  const hfUrl = `https://huggingface.co/${modelId}/blob/main/${file.name}`;

  const isDownloading = progress?.type === 'progress';
  const isDone = progress?.type === 'done' || (!progress && persistedDone);
  const isError = progress?.type === 'error';
  const isPaused = !isDownloading && !isDone && !isError && !!pendingRecord;
  const pct = isDownloading && progress.total && progress.total > 0
    ? Math.round((progress.bytes! / progress.total) * 100)
    : 0;

  return (
    <div className="flex items-center gap-2 py-1.5 px-1 rounded hover:bg-accent/30 transition-colors relative overflow-hidden">
      {isDownloading ? <div
          className="absolute inset-0 bg-primary/10 rounded transition-all duration-300"
          style={{ width: `${pct}%` }}
        /> : null}
      {isPaused && pendingRecord.file_size > 0 ? <div
          className="absolute inset-0 bg-yellow-500/10 rounded"
          style={{ width: `${Math.round((pendingRecord.bytes_downloaded / pendingRecord.file_size) * 100)}%` }}
        /> : null}
      <div className="flex-1 min-w-0 relative z-10">
        <a
          href={hfUrl}
          target="_blank"
          rel="noopener noreferrer"
          onClick={(e) => e.stopPropagation()}
          className="text-sm hover:underline truncate block"
        >
          {shortName}
        </a>
        <div className="flex items-center gap-2 mt-0.5">
          {quant ? <span className="text-[10px] font-mono font-semibold px-1.5 py-0.5 rounded bg-primary/15 text-primary">
              {quant}
            </span> : null}
          {type !== 'model' && (
            <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-muted text-muted-foreground">
              {type}
            </span>
          )}
          <span className="text-xs text-muted-foreground">{formatSize(file.size)}</span>
          {isDownloading ? <span className="text-xs text-primary font-medium">
              {pct}% &middot; {formatSize((progress.speed_kbps ?? 0) * 1024)}/s
            </span> : null}
          {isPaused ? <span className="text-xs text-yellow-600 font-medium">
              Paused &middot; {formatSize(pendingRecord.bytes_downloaded)} / {formatSize(pendingRecord.file_size)}
            </span> : null}
          {isDone ? <span className="text-xs text-green-500 font-medium">Downloaded</span> : null}
          {isError ? <span className="text-xs text-destructive font-medium truncate max-w-[200px]">
              {progress.message}
            </span> : null}
        </div>
      </div>
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onDownload(modelId, file, pendingRecord?.dest_path);
        }}
        disabled={isDownloading}
        className="text-muted-foreground hover:text-foreground shrink-0 relative z-10 disabled:opacity-50"
        title={isPaused ? 'Resume download' : isDownloading ? 'Downloading...' : 'Download to local disk'}
      >
        {isDownloading
          ? <Loader2 className="h-4 w-4 animate-spin" />
          : <ArrowDownToLine className="h-4 w-4" />
        }
      </button>
    </div>
  );
}

function ModelCard({ model, onDownload, downloads, downloadedSet, pendingDownloads }: {
  model: HubModel;
  onDownload: (modelId: string, file: HubFile, resumeDest?: string) => void;
  downloads: Map<string, DownloadProgress>;
  downloadedSet: Set<string>;
  pendingDownloads: Map<string, HubDownloadRecord>;
}) {
  const [expanded, setExpanded] = useState(false);
  const [detailedFiles, setDetailedFiles] = useState<HubFile[] | null>(null);
  const [loadingFiles, setLoadingFiles] = useState(false);
  const ggufCount = model.files.length;

  const handleExpand = async () => {
    const willExpand = !expanded;
    setExpanded(willExpand);
    if (willExpand && !detailedFiles) {
      setLoadingFiles(true);
      try {
        const files = await fetchHubTree(model.id);
        setDetailedFiles(files);
      } catch {
        // Fall back to search-level data (no sizes)
        setDetailedFiles(model.files);
      } finally {
        setLoadingFiles(false);
      }
    }
  };

  const files = detailedFiles ?? model.files;

  return (
    <div className="border rounded-lg p-3 hover:bg-accent/50 transition-colors">
      <button
        type="button"
        className="flex items-start justify-between cursor-pointer w-full text-left"
        onClick={handleExpand}
      >
        <div className="flex-1 min-w-0">
          <div className="font-medium text-sm truncate">{model.id}</div>
          <div className="flex items-center gap-3 text-xs text-muted-foreground mt-1">
            <span className="flex items-center gap-1">
              <Download className="h-3 w-3" /> {formatNumber(model.downloads)}
            </span>
            <span className="flex items-center gap-1">
              <Heart className="h-3 w-3" /> {formatNumber(model.likes)}
            </span>
            <span>{ggufCount} file{ggufCount !== 1 ? 's' : ''}</span>
          </div>
        </div>
        <div className="flex items-center gap-2 ml-2 shrink-0">
          <a
            href={`https://huggingface.co/${model.id}`}
            target="_blank"
            rel="noopener noreferrer"
            onClick={(e) => e.stopPropagation()}
            className="text-muted-foreground hover:text-foreground"
          >
            <ExternalLink className="h-4 w-4" />
          </a>
          {ggufCount > 0 && (expanded
            ? <ChevronDown className="h-4 w-4 text-muted-foreground" />
            : <ChevronRight className="h-4 w-4 text-muted-foreground" />
          )}
        </div>
      </button>

      {expanded ? <div className="mt-2 border-t pt-2 space-y-0.5">
          {loadingFiles ? <div className="flex items-center gap-2 py-2 text-xs text-muted-foreground">
              <Loader2 className="h-3 w-3 animate-spin" /> Loading file details...
            </div> : null}
          {!loadingFiles && files.map((f) => {
            const key = `${model.id}/${f.name}`;
            return (
              <FileRow
                key={f.name}
                file={f}
                modelId={model.id}
                onDownload={onDownload}
                progress={downloads.get(key)}
                persistedDone={downloadedSet.has(key)}
                pendingRecord={pendingDownloads.get(key)}
              />
            );
          })}
        </div> : null}
    </div>
  );
}

// eslint-disable-next-line max-lines-per-function
export const HubExplorer: React.FC<HubExplorerProps> = ({ isOpen, onClose }) => {
  const [query, setQuery] = useState('');
  const { models, isLoading, error, sort, searchModels, debouncedSearch, changeSort } = useHubSearch();

  // Download state
  const [downloads, setDownloads] = useState<Map<string, DownloadProgress>>(new Map());
  const [downloadedSet, setDownloadedSet] = useState<Set<string>>(new Set());
  const [pendingDownloads, setPendingDownloads] = useState<Map<string, HubDownloadRecord>>(new Map());
  const abortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    if (isOpen) {
      searchModels('');
      // Clear stale in-memory progress so only DB-backed labels survive across opens
      setDownloads(new Map());
      // Load persisted download records and verify files still exist on disk
      verifyHubDownloads()
        .then((records) => {
          const completed = new Set<string>();
          const pending = new Map<string, HubDownloadRecord>();
          for (const r of records) {
            const key = `${r.model_id}/${r.filename}`;
            if (r.status === 'completed') {
              completed.add(key);
            } else {
              pending.set(key, r);
            }
          }
          setDownloadedSet(completed);
          setPendingDownloads(pending);
        })
        .catch(() => { /* ignore — just won't show persisted labels */ });
    }
  }, [isOpen, searchModels]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') searchModels(query);
  };

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setQuery(e.target.value);
    debouncedSearch(e.target.value);
  };

  const handleDownloadClick = useCallback(async (modelId: string, file: HubFile, resumeDest?: string) => {
    // If resuming, use stored destination; otherwise ask user to pick
    const dirPath = resumeDest ?? await pickDirectory();
    if (!dirPath) return; // user cancelled

    const key = `${modelId}/${file.name}`;
    // Clear pending state for this file since we're actively downloading now
    setPendingDownloads(prev => {
      const next = new Map(prev);
      next.delete(key);
      return next;
    });

    const controller = startHubDownload(
      modelId,
      file.name,
      dirPath,
      (event) => {
        setDownloads(prev => {
          const next = new Map(prev);
          next.set(key, event);
          return next;
        });
        // When done, add to persistent set so label survives across sessions
        if (event.type === 'done') {
          setDownloadedSet(prev => new Set(prev).add(key));
        }
      },
    );
    abortRef.current = controller;
  }, []);

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="max-w-2xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>Explore GGUF Models</DialogTitle>
        </DialogHeader>

        <div className="flex items-center gap-2">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <input
              value={query}
              onChange={handleChange}
              onKeyDown={handleKeyDown}
              placeholder="Search HuggingFace models..."
              className="w-full pl-9 pr-3 py-2 border rounded-md bg-background text-sm focus:outline-none focus:ring-2 focus:ring-ring"
              // eslint-disable-next-line jsx-a11y/no-autofocus
              autoFocus
            />
          </div>
          <div className="relative shrink-0">
            <ArrowUpDown className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground pointer-events-none" />
            <select
              value={sort}
              onChange={(e) => changeSort(e.target.value as HubSortField, query)}
              className="pl-8 pr-2 py-2 border rounded-md bg-background text-sm appearance-none cursor-pointer focus:outline-none focus:ring-2 focus:ring-ring"
            >
              {SORT_OPTIONS.map((opt) => (
                <option key={opt.value} value={opt.value}>{opt.label}</option>
              ))}
            </select>
          </div>
        </div>

        <div className="relative flex-1 min-h-0 overflow-y-auto space-y-2" style={{ maxHeight: '400px' }}>
          {isLoading ? <div className="absolute inset-0 z-10 flex items-center justify-center bg-background/60 rounded-md">
              <Loader2 className="h-5 w-5 animate-spin mr-2 text-muted-foreground" />
              <span className="text-muted-foreground text-sm">Searching...</span>
            </div> : null}

          {error ? <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md text-sm text-destructive">
              {error}
            </div> : null}

          {!error && models.length === 0 && !isLoading && (
            <div className="text-center py-8 text-muted-foreground text-sm">
              {query ? <>No GGUF models found for &ldquo;{query}&rdquo;</> : 'No models found'}
            </div>
          )}

          {models.map((m) => (
            <ModelCard
              key={m.id}
              model={m}
              onDownload={handleDownloadClick}
              downloads={downloads}
              downloadedSet={downloadedSet}
              pendingDownloads={pendingDownloads}
            />
          ))}
        </div>

        <div className="flex justify-end pt-2 border-t">
          <Button variant="outline" onClick={onClose}>Close</Button>
        </div>
      </DialogContent>
    </Dialog>
  );
};
