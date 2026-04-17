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
    <div className="flex items-center gap-2 py-1.5 px-1 rounded hover:bg-accent/30 transition-colors relative overflow-hidden">
      {isDownloading ? (
        <div
          className="absolute inset-y-0 left-0 bg-emerald-500/20 rounded transition-all duration-500 ease-out"
          style={{ width: `${pct}%` }}
        />
      ) : null}
      {isPaused && pendingRecord.file_size > 0 ? (
        <div
          className="absolute inset-0 bg-yellow-500/10 rounded"
          style={{
            width: `${Math.round((pendingRecord.bytes_downloaded / pendingRecord.file_size) * 100)}%`,
          }}
        />
      ) : null}
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
          {quant ? (
            <span className="text-[10px] font-mono font-semibold px-1.5 py-0.5 rounded bg-primary/15 text-primary">
              {quant}
            </span>
          ) : null}
          {type !== 'model' && (
            <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-muted text-muted-foreground">
              {type}
            </span>
          )}
          <span className="text-xs text-muted-foreground">{formatSize(file.size)}</span>
          {isDownloading ? (
            <span className="text-xs text-primary font-medium">
              {pct}% &middot; {formatSize((progress.speed_kbps ?? 0) * 1024)}/s
            </span>
          ) : null}
          {isPaused ? (
            <span className="text-xs text-yellow-600 font-medium">
              Paused &middot; {formatSize(pendingRecord.bytes_downloaded)} /{' '}
              {formatSize(pendingRecord.file_size)}
            </span>
          ) : null}
          {isDone ? <span className="text-xs text-green-500 font-medium">Downloaded</span> : null}
          {isError ? (
            <span className="text-xs text-destructive font-medium truncate max-w-[200px]">
              {progress.message}
            </span>
          ) : null}
        </div>
      </div>
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onDownload(modelId, file, pendingRecord?.dest_path);
        }}
        disabled={isDownloading}
        className="text-muted-foreground hover:text-foreground shrink-0 relative z-10 disabled:opacity-50 cursor-pointer disabled:cursor-not-allowed"
        title={downloadTitle}
        aria-label={downloadAriaLabel}
      >
        {isDownloading ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <ArrowDownToLine className="h-4 w-4" />
        )}
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
            <span>{formatFileCount(ggufCount)}</span>
          </div>
        </div>
        <div className="flex items-center gap-2 ml-2 shrink-0">
          <a
            href={`https://huggingface.co/${model.id}`}
            target="_blank"
            rel="noopener noreferrer"
            onClick={(e) => e.stopPropagation()}
            className="text-muted-foreground hover:text-foreground"
            aria-label="Open on HuggingFace"
          >
            <ExternalLink className="h-4 w-4" />
          </a>
          {ggufCount > 0 &&
            (expanded ? (
              <ChevronDown className="h-4 w-4 text-muted-foreground" />
            ) : (
              <ChevronRight className="h-4 w-4 text-muted-foreground" />
            ))}
        </div>
      </button>

      {expanded ? (
        <div className="mt-2 border-t pt-2 space-y-0.5">
          {loadingFiles ? (
            <div className="flex items-center gap-2 py-2 text-xs text-muted-foreground">
              <Loader2 className="h-3 w-3 animate-spin" /> Loading file details...
            </div>
          ) : null}
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
      ) : null}
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
    <div className="flex items-center gap-2 py-2 px-2 rounded hover:bg-accent/30 transition-colors relative overflow-hidden">
      {/* Progress bar background */}
      {isDownloading ? (
        <div
          className="absolute inset-y-0 left-0 bg-emerald-500/20 rounded transition-all duration-500 ease-out"
          style={{ width: `${pct}%` }}
        />
      ) : null}
      {!isCompleted && !isDownloading && record.file_size > 0 ? (
        <div className="absolute inset-0 bg-yellow-500/10 rounded" style={{ width: `${pct}%` }} />
      ) : null}

      <div className="flex-1 min-w-0 relative z-10">
        <div className="text-sm font-medium truncate">{shortName}</div>
        <div className="flex items-center gap-2 mt-0.5">
          {quant ? (
            <span className="text-[10px] font-mono font-semibold px-1.5 py-0.5 rounded bg-primary/15 text-primary">
              {quant}
            </span>
          ) : null}
          <span className="text-xs text-muted-foreground truncate">{record.model_id}</span>
          <span className="text-xs text-muted-foreground">{formatSize(record.file_size)}</span>
          {(() => {
            if (isCompleted) {
              return (
                <span className="text-xs text-green-500 font-medium">
                  {formatRelativeTime(record.downloaded_at)}
                </span>
              );
            }
            if (isDownloading) {
              return (
                <span className="text-xs text-primary font-medium">
                  {pct}% &middot; {formatSize((progress.speed_kbps ?? 0) * 1024)}/s
                </span>
              );
            }
            return (
              <span className="text-xs text-yellow-600 font-medium">
                Paused &middot; {formatSize(record.bytes_downloaded)} /{' '}
                {formatSize(record.file_size)}
              </span>
            );
          })()}
        </div>
        {isCompleted ? (
          <div className="flex items-center gap-1 mt-0.5">
            <FolderOpen className="h-3 w-3 text-muted-foreground shrink-0" />
            <span className="text-[11px] text-muted-foreground truncate">{record.dest_path}</span>
          </div>
        ) : null}
      </div>

      <div className="relative z-10 shrink-0 flex items-center gap-1">
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
                <Play className="h-4 w-4" />
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
                <Pause className="h-4 w-4" />
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
              <ArrowDownToLine className="h-4 w-4" />
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
          <X className="h-4 w-4" />
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
      <div className="text-center py-12 text-muted-foreground text-sm">
        <ArrowDownToLine className="h-8 w-8 mx-auto mb-3 opacity-40" />
        <p>No downloads yet</p>
        <p className="text-xs mt-1">Search and download models from the Explore tab</p>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {pendingList.length > 0 ? (
        <div>
          <div className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-1.5 px-1">
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
      ) : null}

      {completedList.length > 0 ? (
        <div>
          <div className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-1.5 px-1">
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
      ) : null}
    </div>
  );
};

// ─── Main HubExplorer ───────────────────────────────────────────────

// eslint-disable-next-line max-lines-per-function
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
      <DialogContent className="max-w-2xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>Explore GGUF Models</DialogTitle>
        </DialogHeader>

        {/* Tab bar */}
        <div className="flex border-b">
          <button
            type="button"
            onClick={() => setActiveTab('explore')}
            className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
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
            className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors flex items-center gap-1.5 ${
              activeTab === 'downloads'
                ? 'border-primary text-foreground'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            }`}
            role="tab"
            aria-selected={activeTab === 'downloads'}
          >
            Downloads
            {totalDownloads > 0 ? (
              <span
                className={`text-[10px] font-mono px-1.5 py-0.5 rounded-full ${
                  pendingCount > 0
                    ? 'bg-yellow-500/20 text-yellow-600'
                    : 'bg-muted text-muted-foreground'
                }`}
              >
                {totalDownloads}
              </span>
            ) : null}
          </button>
        </div>

        {/* Explore tab */}
        {activeTab === 'explore' ? (
          <>
            {/* Models directory picker */}
            <button
              type="button"
              onClick={handlePickDirectory}
              disabled={isPicking}
              className={`w-full px-3 py-2 text-sm border rounded-md bg-background text-left flex items-center gap-2 border-input ${
                isPicking ? 'opacity-60' : 'cursor-pointer hover:bg-accent/50 transition-colors'
              }`}
            >
              {isPicking ? (
                <Loader2 className="h-4 w-4 animate-spin shrink-0" />
              ) : (
                <FolderOpen className="h-4 w-4 text-foreground shrink-0" />
              )}
              {modelsDirectory ? (
                <span className="font-mono text-xs truncate">{modelsDirectory}</span>
              ) : (
                <span className="text-foreground/60">
                  Click to set models download directory...
                </span>
              )}
            </button>

            {!modelsDirectory ? (
              <div className="text-center py-12 text-muted-foreground text-sm">
                <FolderOpen className="h-8 w-8 mx-auto mb-3 opacity-40" />
                <p>Set a download directory to browse and download models.</p>
              </div>
            ) : (
              <>
                <div className="flex items-center gap-2">
                  <div className="relative flex-1">
                    <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                    <input
                      value={query}
                      onChange={handleChange}
                      onKeyDown={handleKeyDown}
                      placeholder="Search models or paste repo ID (e.g. unsloth/gemma-4-26B-A4B-it-GGUF)..."
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
                        <option key={opt.value} value={opt.value}>
                          {opt.label}
                        </option>
                      ))}
                    </select>
                  </div>
                </div>

                <div
                  className="relative flex-1 min-h-0 overflow-y-auto space-y-2"
                  style={{ maxHeight: '400px' }}
                >
                  {isLoading ? (
                    <div className="absolute inset-0 z-10 flex items-center justify-center bg-background/60 rounded-md">
                      <Loader2 className="h-5 w-5 animate-spin mr-2 text-muted-foreground" />
                      <span className="text-muted-foreground text-sm">Searching...</span>
                    </div>
                  ) : null}

                  {error ? (
                    <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md text-sm text-destructive">
                      {error}
                    </div>
                  ) : null}

                  {!error && models.length === 0 && !isLoading && (
                    <div className="text-center py-8 text-muted-foreground text-sm">
                      {query ? (
                        <>No GGUF models found for &ldquo;{query}&rdquo;</>
                      ) : (
                        'No models found'
                      )}
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
        ) : null}

        {/* Downloads tab */}
        {activeTab === 'downloads' ? (
          <div className="relative flex-1 min-h-0 overflow-y-auto" style={{ maxHeight: '400px' }}>
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
        ) : null}

        <div className="flex justify-end pt-2 border-t">
          <Button variant="outline" onClick={onClose}>
            Close
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
};
