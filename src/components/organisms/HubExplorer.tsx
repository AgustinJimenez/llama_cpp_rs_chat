/* eslint-disable max-lines -- multi-component Hub explorer with FileRow, ModelCard, DownloadRow, DownloadsTab */
import {
  Search,
  Loader2,
  ExternalLink,
  ChevronDown,
  ChevronRight,
  Download,
  Heart,
  ArrowUpDown,
  ArrowDownToLine,
  Play,
  Pause,
  FolderOpen,
  X,
} from 'lucide-react';
import React, { useState, useEffect, useCallback } from 'react';

import { Button } from '../atoms/button';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '../atoms/dialog';

import { useDownloadContext } from '@/contexts/DownloadContext';
import { useHubSearch } from '@/hooks/useHubSearch';
import type { HubModel, HubSortField } from '@/hooks/useHubSearch';
import {
  fetchHubTree,
  loadModel,
  getConfig,
  saveConfig,
  pickDirectory,
} from '@/utils/tauriCommands';
import type { HubFile, DownloadProgress, HubDownloadRecord } from '@/utils/tauriCommands';

interface HubExplorerProps {
  isOpen: boolean;
  onClose: () => void;
}

type TabId = 'explore' | 'downloads';

const BYTES_PER_GB = 1_073_741_824;
const BYTES_PER_MB = 1_048_576;
const LARGE_NUMBER_THRESHOLD = 1_000_000;
const MS_PER_MINUTE = 60_000;
const DAYS_THRESHOLD = 30;

const SORT_OPTIONS: { value: HubSortField; label: string }[] = [
  { value: 'downloads', label: 'Downloads' },
  { value: 'likes', label: 'Likes' },
  { value: 'lastModified', label: 'Recently Updated' },
  { value: 'createdAt', label: 'Newest' },
];

function formatSize(bytes: number): string {
  if (bytes === 0) return '\u2014';
  const gb = bytes / BYTES_PER_GB;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  const mb = bytes / BYTES_PER_MB;
  if (mb >= 1) return `${mb.toFixed(0)} MB`;
  const kb = bytes / 1024;
  return `${kb.toFixed(0)} KB`;
}

function formatNumber(n: number): string {
  if (n >= LARGE_NUMBER_THRESHOLD) return `${(n / LARGE_NUMBER_THRESHOLD).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function formatFileCount(count: number): string {
  if (count === 0) return 'No GGUF files — needs conversion';
  return `${count} file${count !== 1 ? 's' : ''}`;
}

function formatRelativeTime(timestampMs: number): string {
  const now = Date.now();
  const diff = now - timestampMs;
  const minutes = Math.floor(diff / MS_PER_MINUTE);
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < DAYS_THRESHOLD) return `${days}d ago`;
  return new Date(timestampMs).toLocaleDateString();
}

/** Extract quantization type from a GGUF filename */
function extractQuant(filename: string): string | null {
  // Match patterns like Q4_K_M, IQ3_XS, Q8_0, F16, BF16, MXFP4, etc.
  const base =
    filename
      .replace(/\.gguf$/i, '')
      .split('/')
      .pop() ?? '';
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

/** Sort key for a quant string — lower = smaller/faster model */
/* eslint-disable @typescript-eslint/no-magic-numbers */
function quantSortKey(quant: string | null): number {
  if (!quant) return 999;
  const q = quant.toUpperCase();
  if (q.startsWith('BF16')) return 200;
  if (q.startsWith('F16')) return 190;
  if (q.startsWith('F32')) return 210;
  const m = q.match(/^(?:IQ|Q|MXFP)(\d+)/);
  if (m) return parseInt(m[1], 10);
  return 100;
}
/* eslint-enable @typescript-eslint/no-magic-numbers */

/** Classify file type */
function fileType(name: string): 'mmproj' | 'imatrix' | 'model' {
  const lower = name.toLowerCase();
  if (lower.includes('mmproj')) return 'mmproj';
  if (lower.includes('imatrix')) return 'imatrix';
  return 'model';
}

// eslint-disable-next-line complexity
const FileRow = ({
  file,
  modelId,
  onDownload,
  progress,
  persistedDone,
  pendingRecord,
}: {
  file: HubFile;
  modelId: string;
  onDownload: (modelId: string, file: HubFile, resumeDest?: string) => void;
  progress?: DownloadProgress | null;
  persistedDone?: boolean;
  pendingRecord?: HubDownloadRecord | null;
}) => {
  const quant = extractQuant(file.name);
  const type = fileType(file.name);
  const shortName = file.name.split('/').pop() ?? file.name;
  const hfUrl = `https://huggingface.co/${modelId}/blob/main/${file.name}`;

  const isDownloading = progress?.type === 'progress';
  const isDone = progress?.type === 'done' || (!progress && persistedDone);
  const isError = progress?.type === 'error';
  const isPaused = !isDownloading && !isDone && !isError && !!pendingRecord;
  const pct =
    isDownloading && progress.total && progress.total > 0
      ? Math.round(((progress.bytes ?? 0) / progress.total) * 100)
      : 0;

  let downloadTitle = 'Download to local disk';
  let downloadAriaLabel = 'Download file';
  if (isPaused) {
    downloadTitle = 'Resume download';
    downloadAriaLabel = 'Resume download';
  } else if (isDownloading) {
    downloadTitle = 'Downloading...';
    downloadAriaLabel = 'Downloading';
  }

  return (
    <div className="relative flex items-center gap-2 overflow-hidden rounded px-1 py-1.5 transition-colors hover:bg-accent/30">
      {!!isDownloading && (
        <div
          className="absolute inset-y-0 left-0 rounded bg-emerald-500/20 transition-all duration-500 ease-out"
          style={{ width: `${pct}%` }}
        />
      )}
      {!!isPaused && pendingRecord.file_size > 0 && (
        <div
          className="absolute inset-0 rounded bg-yellow-500/10"
          style={{
            width: `${Math.round((pendingRecord.bytes_downloaded / pendingRecord.file_size) * 100)}%`,
          }}
        />
      )}
      <div className="relative z-10 min-w-0 flex-1">
        <a
          href={hfUrl}
          target="_blank"
          rel="noopener noreferrer"
          onClick={(e) => e.stopPropagation()}
          className="block truncate text-sm hover:underline"
        >
          {shortName}
        </a>
        <div className="mt-0.5 flex items-center gap-2">
          {!!quant && (
            <span className="rounded bg-primary/15 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-primary">
              {quant}
            </span>
          )}
          {type !== 'model' && (
            <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
              {type}
            </span>
          )}
          <span className="text-xs text-muted-foreground">{formatSize(file.size)}</span>
          {!!isDownloading && (
            <span className="text-xs font-medium text-primary">
              {pct}% &middot; {formatSize((progress.speed_kbps ?? 0) * 1024)}/s
            </span>
          )}
          {!!isPaused && (
            <span className="text-xs font-medium text-yellow-600">
              Paused &middot; {formatSize(pendingRecord.bytes_downloaded)} /{' '}
              {formatSize(pendingRecord.file_size)}
            </span>
          )}
          {!!isDone && <span className="text-xs font-medium text-green-500">Downloaded</span>}
          {!!isError && (
            <span className="max-w-[200px] truncate text-xs font-medium text-destructive">
              {progress.message}
            </span>
          )}
        </div>
      </div>
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onDownload(modelId, file, pendingRecord?.dest_path);
        }}
        disabled={isDownloading}
        className="relative z-10 shrink-0 cursor-pointer text-muted-foreground hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50"
        title={downloadTitle}
        aria-label={downloadAriaLabel}
      >
        {!!isDownloading && <Loader2 className="size-4 animate-spin" />}
        {!isDownloading && <ArrowDownToLine className="size-4" />}
      </button>
    </div>
  );
};

const ModelCard = ({
  model,
  onDownload,
  downloads,
  downloadedSet,
  pendingDownloads,
}: {
  model: HubModel;
  onDownload: (modelId: string, file: HubFile, resumeDest?: string) => void;
  downloads: Map<string, DownloadProgress>;
  downloadedSet: Set<string>;
  pendingDownloads: Map<string, HubDownloadRecord>;
}) => {
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
    <div className="rounded-lg border p-3 transition-colors hover:bg-accent/50">
      <button
        type="button"
        className="flex w-full cursor-pointer items-start justify-between text-left"
        onClick={handleExpand}
      >
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium">{model.id}</div>
          <div className="mt-1 flex items-center gap-3 text-xs text-muted-foreground">
            <span className="flex items-center gap-1">
              <Download className="size-3" /> {formatNumber(model.downloads)}
            </span>
            <span className="flex items-center gap-1">
              <Heart className="size-3" /> {formatNumber(model.likes)}
            </span>
            <span>{formatFileCount(ggufCount)}</span>
          </div>
        </div>
        <div className="ml-2 flex shrink-0 items-center gap-2">
          <a
            href={`https://huggingface.co/${model.id}`}
            target="_blank"
            rel="noopener noreferrer"
            onClick={(e) => e.stopPropagation()}
            className="text-muted-foreground hover:text-foreground"
            aria-label="Open on HuggingFace"
          >
            <ExternalLink className="size-4" />
          </a>
          {ggufCount > 0 && !!expanded && <ChevronDown className="size-4 text-muted-foreground" />}
          {ggufCount > 0 && !expanded && <ChevronRight className="size-4 text-muted-foreground" />}
        </div>
      </button>

      {!!expanded && (
        <div className="mt-2 space-y-0.5 border-t pt-2">
          {!!loadingFiles && (
            <div className="flex items-center gap-2 py-2 text-xs text-muted-foreground">
              <Loader2 className="size-3 animate-spin" /> Loading file details...
            </div>
          )}
          {!loadingFiles &&
            [...files]
              .sort((a, b) => {
                const ta = fileType(a.name);
                const tb = fileType(b.name);
                // mmproj/imatrix after models
                const typeOrder = (t: string) => {
                  if (t === 'model') return 0;
                  if (t === 'mmproj') return 1;
                  return 2;
                };
                if (ta !== tb) return typeOrder(ta) - typeOrder(tb);
                // Among models: sort by size ascending (0 = unknown size, sort last)
                const sa = a.size || Number.MAX_SAFE_INTEGER;
                const sb = b.size || Number.MAX_SAFE_INTEGER;
                if (sa !== sb) return sa - sb;
                // Fallback: quant bits ascending
                return quantSortKey(extractQuant(a.name)) - quantSortKey(extractQuant(b.name));
              })
              .map((f) => {
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
        </div>
      )}
    </div>
  );
};

// ─── Downloads Tab ──────────────────────────────────────────────────

const DownloadRow = ({
  record,
  progress,
  onResume,
  onLoad,
  onPause,
  onCancel,
}: {
  record: HubDownloadRecord;
  progress?: DownloadProgress | null;
  onResume: (record: HubDownloadRecord) => void;
  onLoad: (record: HubDownloadRecord) => void;
  onPause: (key: string) => void;
  onCancel: (key: string) => void;
}) => {
  const shortName = record.filename.split('/').pop() ?? record.filename;
  const quant = extractQuant(record.filename);
  const isCompleted = record.status === 'completed';
  const isDownloading = progress?.type === 'progress';
  const FULL_PERCENTAGE = 100;
  let pct = 0;
  if (isDownloading && progress.total && progress.total > 0) {
    pct = Math.round(((progress.bytes ?? 0) / progress.total) * FULL_PERCENTAGE);
  } else if (isCompleted) {
    pct = FULL_PERCENTAGE;
  } else if (record.file_size > 0) {
    pct = Math.round((record.bytes_downloaded / record.file_size) * FULL_PERCENTAGE);
  }

  return (
    <div className="relative flex items-center gap-2 overflow-hidden rounded px-2 py-2 transition-colors hover:bg-accent/30">
      {/* Progress bar background */}
      {!!isDownloading && (
        <div
          className="absolute inset-y-0 left-0 rounded bg-emerald-500/20 transition-all duration-500 ease-out"
          style={{ width: `${pct}%` }}
        />
      )}
      {!isCompleted && !isDownloading && record.file_size > 0 && (
        <div className="absolute inset-0 rounded bg-yellow-500/10" style={{ width: `${pct}%` }} />
      )}

      <div className="relative z-10 min-w-0 flex-1">
        <div className="truncate text-sm font-medium">{shortName}</div>
        <div className="mt-0.5 flex items-center gap-2">
          {!!quant && (
            <span className="rounded bg-primary/15 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-primary">
              {quant}
            </span>
          )}
          <span className="truncate text-xs text-muted-foreground">{record.model_id}</span>
          <span className="text-xs text-muted-foreground">{formatSize(record.file_size)}</span>
          {(() => {
            if (isCompleted) {
              return (
                <span className="text-xs font-medium text-green-500">
                  {formatRelativeTime(record.downloaded_at)}
                </span>
              );
            }
            if (isDownloading) {
              return (
                <span className="text-xs font-medium text-primary">
                  {pct}% &middot; {formatSize((progress.speed_kbps ?? 0) * 1024)}/s
                </span>
              );
            }
            return (
              <span className="text-xs font-medium text-yellow-600">
                Paused &middot; {formatSize(record.bytes_downloaded)} /{' '}
                {formatSize(record.file_size)}
              </span>
            );
          })()}
        </div>
        {!!isCompleted && (
          <div className="mt-0.5 flex items-center gap-1">
            <FolderOpen className="size-3 shrink-0 text-muted-foreground" />
            <span className="truncate text-[11px] text-muted-foreground">{record.dest_path}</span>
          </div>
        )}
      </div>

      <div className="relative z-10 flex shrink-0 items-center gap-1">
        {(() => {
          if (isCompleted) {
            return (
              <button
                type="button"
                onClick={() => onLoad(record)}
                className="text-muted-foreground hover:text-foreground"
                title="Load this model"
                aria-label="Load this model"
              >
                <Play className="size-4" />
              </button>
            );
          }
          if (isDownloading) {
            return (
              <button
                type="button"
                onClick={() => onPause(`${record.model_id}/${record.filename}`)}
                className="text-muted-foreground hover:text-yellow-500"
                title="Pause download"
                aria-label="Pause download"
              >
                <Pause className="size-4" />
              </button>
            );
          }
          return (
            <button
              type="button"
              onClick={() => onResume(record)}
              className="text-muted-foreground hover:text-foreground"
              title="Resume download"
              aria-label="Resume download"
            >
              <ArrowDownToLine className="size-4" />
            </button>
          );
        })()}
        <button
          type="button"
          onClick={() => onCancel(`${record.model_id}/${record.filename}`)}
          className="text-muted-foreground hover:text-destructive"
          title="Cancel and delete"
          aria-label="Cancel and delete download"
        >
          <X className="size-4" />
        </button>
      </div>
    </div>
  );
};

const DownloadsTab = ({
  completedDownloads,
  pendingDownloads,
  downloads,
  onResume,
  onLoad,
  onPause,
  onCancel,
}: {
  completedDownloads: Map<string, HubDownloadRecord>;
  pendingDownloads: Map<string, HubDownloadRecord>;
  downloads: Map<string, DownloadProgress>;
  onResume: (record: HubDownloadRecord) => void;
  onLoad: (record: HubDownloadRecord) => void;
  onPause: (key: string) => void;
  onCancel: (key: string) => void;
}) => {
  const pendingList = Array.from(pendingDownloads.values());
  const completedList = Array.from(completedDownloads.values());
  const isEmpty = pendingList.length === 0 && completedList.length === 0;

  if (isEmpty) {
    return (
      <div className="py-12 text-center text-sm text-muted-foreground">
        <ArrowDownToLine className="mx-auto mb-3 size-8 opacity-40" />
        <p>No downloads yet</p>
        <p className="mt-1 text-xs">Search and download models from the Explore tab</p>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {pendingList.length > 0 && (
        <div>
          <div className="mb-1.5 px-1 text-xs font-medium uppercase tracking-wider text-muted-foreground">
            Pending ({pendingList.length})
          </div>
          <div className="space-y-0.5">
            {pendingList.map((r) => {
              const key = `${r.model_id}/${r.filename}`;
              return (
                <DownloadRow
                  key={key}
                  record={r}
                  progress={downloads.get(key)}
                  onResume={onResume}
                  onLoad={onLoad}
                  onPause={onPause}
                  onCancel={onCancel}
                />
              );
            })}
          </div>
        </div>
      )}

      {completedList.length > 0 && (
        <div>
          <div className="mb-1.5 px-1 text-xs font-medium uppercase tracking-wider text-muted-foreground">
            Completed ({completedList.length})
          </div>
          <div className="space-y-0.5">
            {completedList.map((r) => {
              const key = `${r.model_id}/${r.filename}`;
              return (
                <DownloadRow
                  key={key}
                  record={r}
                  progress={downloads.get(key)}
                  onResume={onResume}
                  onLoad={onLoad}
                  onPause={onPause}
                  onCancel={onCancel}
                />
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
};

// ─── Main HubExplorer ───────────────────────────────────────────────

// eslint-disable-next-line max-lines-per-function, complexity
export const HubExplorer: React.FC<HubExplorerProps> = ({ isOpen, onClose }) => {
  const [activeTab, setActiveTab] = useState<TabId>('explore');
  const [query, setQuery] = useState('');
  const [modelsDirectory, setModelsDirectory] = useState<string | null>(null);
  const [isPicking, setIsPicking] = useState(false);
  const { models, isLoading, error, sort, searchModels, debouncedSearch, changeSort } =
    useHubSearch();
  const {
    downloads,
    downloadedSet,
    completedDownloads,
    pendingDownloads,
    startDownload,
    pauseDownload,
    cancelDownload,
    refreshRecords,
  } = useDownloadContext();

  useEffect(() => {
    if (isOpen) {
      searchModels('');
      refreshRecords();
      getConfig()
        .then((cfg) => {
          const dir = cfg.models_directory ?? null;
          setModelsDirectory(dir);
        })
        .catch(() => {});
    }
  }, [isOpen, searchModels, refreshRecords]);

  // Refresh download records when switching to Downloads tab
  useEffect(() => {
    if (isOpen && activeTab === 'downloads') {
      refreshRecords();
    }
  }, [isOpen, activeTab, refreshRecords]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') searchModels(query);
  };

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setQuery(e.target.value);
    debouncedSearch(e.target.value);
  };

  const handleDownloadClick = useCallback(
    async (modelId: string, file: HubFile, resumeDest?: string) => {
      const dirPath = resumeDest ?? modelsDirectory;
      if (!dirPath) return;
      startDownload(modelId, file, dirPath);
    },
    [startDownload, modelsDirectory],
  );

  const handlePickDirectory = useCallback(async () => {
    if (isPicking) return;
    setIsPicking(true);
    try {
      const dir = await pickDirectory();
      if (dir) {
        const cfg = await getConfig();
        await saveConfig({ ...cfg, models_directory: dir });
        setModelsDirectory(dir);
      }
    } catch (err) {
      console.error('Failed to pick directory:', err);
    } finally {
      setIsPicking(false);
    }
  }, [isPicking]);

  const handleResume = useCallback(
    (record: HubDownloadRecord) => {
      startDownload(
        record.model_id,
        { name: record.filename, size: record.file_size },
        record.dest_path,
      );
    },
    [startDownload],
  );

  const handleLoad = useCallback(
    (record: HubDownloadRecord) => {
      // Construct full path: dest_path is the directory, filename is the file
      // dest_path from DB already includes the directory
      const sep = record.dest_path.includes('\\') ? '\\' : '/';
      const fullPath = `${record.dest_path}${sep}${record.filename}`;
      loadModel(fullPath)
        .then(() => {
          onClose();
        })
        .catch((err) => {
          console.error('Failed to load model:', err);
        });
    },
    [onClose],
  );

  const pendingCount = pendingDownloads.size;
  const completedCount = completedDownloads.size;
  const totalDownloads = pendingCount + completedCount;

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="flex max-h-[80vh] max-w-2xl flex-col">
        <DialogHeader>
          <DialogTitle>Explore GGUF Models</DialogTitle>
        </DialogHeader>

        {/* Tab bar */}
        <div className="flex border-b" role="tablist" aria-label="Hub sections">
          <button
            type="button"
            onClick={() => setActiveTab('explore')}
            className={`border-b-2 px-4 py-2 text-sm font-medium transition-colors ${
              activeTab === 'explore'
                ? 'border-primary text-foreground'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            }`}
            role="tab"
            aria-selected={activeTab === 'explore'}
          >
            Explore
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('downloads')}
            className={`flex items-center gap-1.5 border-b-2 px-4 py-2 text-sm font-medium transition-colors ${
              activeTab === 'downloads'
                ? 'border-primary text-foreground'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            }`}
            role="tab"
            aria-selected={activeTab === 'downloads'}
          >
            Downloads
            {totalDownloads > 0 && (
              <span
                className={`rounded-full px-1.5 py-0.5 font-mono text-[10px] ${
                  pendingCount > 0
                    ? 'bg-yellow-500/20 text-yellow-600'
                    : 'bg-muted text-muted-foreground'
                }`}
              >
                {totalDownloads}
              </span>
            )}
          </button>
        </div>

        {/* Explore tab */}
        {activeTab === 'explore' && (
          <>
            {/* Models directory picker */}
            <button
              type="button"
              onClick={handlePickDirectory}
              disabled={isPicking}
              className={`flex w-full items-center gap-2 rounded-md border border-input bg-background px-3 py-2 text-left text-sm ${
                isPicking ? 'opacity-60' : 'cursor-pointer transition-colors hover:bg-accent/50'
              }`}
            >
              {!!isPicking && <Loader2 className="size-4 shrink-0 animate-spin" />}
              {!isPicking && <FolderOpen className="size-4 shrink-0 text-foreground" />}
              {!!modelsDirectory && (
                <span className="truncate font-mono text-xs">{modelsDirectory}</span>
              )}
              {!modelsDirectory && (
                <span className="text-foreground/60">
                  Click to set models download directory...
                </span>
              )}
            </button>

            {!modelsDirectory && (
              <div className="py-12 text-center text-sm text-muted-foreground">
                <FolderOpen className="mx-auto mb-3 size-8 opacity-40" />
                <p>Set a download directory to browse and download models.</p>
              </div>
            )}
            {!!modelsDirectory && (
              <>
                <div className="flex items-center gap-2">
                  <div className="relative flex-1">
                    <Search className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                    <input
                      value={query}
                      onChange={handleChange}
                      onKeyDown={handleKeyDown}
                      placeholder="Search models or paste repo ID (e.g. unsloth/gemma-4-26B-A4B-it-GGUF)..."
                      className="w-full rounded-md border bg-background py-2 pl-9 pr-3 text-sm focus:outline-none focus:ring-2 focus:ring-ring"
                      // eslint-disable-next-line jsx-a11y/no-autofocus
                      autoFocus
                    />
                  </div>
                  <div className="relative shrink-0">
                    <ArrowUpDown className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                    <select
                      value={sort}
                      onChange={(e) => changeSort(e.target.value as HubSortField, query)}
                      className="cursor-pointer appearance-none rounded-md border bg-background py-2 pl-8 pr-2 text-sm focus:outline-none focus:ring-2 focus:ring-ring"
                    >
                      {SORT_OPTIONS.map((opt) => (
                        <option key={opt.value} value={opt.value}>
                          {opt.label}
                        </option>
                      ))}
                    </select>
                  </div>
                </div>

                <div
                  className="relative min-h-0 flex-1 space-y-2 overflow-y-auto"
                  style={{ maxHeight: '400px' }}
                >
                  {!!isLoading && (
                    <div className="absolute inset-0 z-10 flex items-center justify-center rounded-md bg-background/60">
                      <Loader2 className="mr-2 size-5 animate-spin text-muted-foreground" />
                      <span className="text-sm text-muted-foreground">Searching...</span>
                    </div>
                  )}

                  {!!error && (
                    <div className="rounded-md border border-destructive/20 bg-destructive/10 p-3 text-sm text-destructive">
                      {error}
                    </div>
                  )}

                  {!error && models.length === 0 && !isLoading && (
                    <div className="py-8 text-center text-sm text-muted-foreground">
                      {!!query && <>No GGUF models found for &ldquo;{query}&rdquo;</>}
                      {!query && 'No models found'}
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
              </>
            )}
          </>
        )}

        {/* Downloads tab */}
        {activeTab === 'downloads' && (
          <div className="relative min-h-0 flex-1 overflow-y-auto" style={{ maxHeight: '400px' }}>
            <DownloadsTab
              completedDownloads={completedDownloads}
              pendingDownloads={pendingDownloads}
              downloads={downloads}
              onResume={handleResume}
              onLoad={handleLoad}
              onPause={pauseDownload}
              onCancel={cancelDownload}
            />
          </div>
        )}

        <div className="flex justify-end border-t pt-2">
          <Button variant="outline" onClick={onClose}>
            Close
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
};
