import { useState, useCallback } from 'react';
import { ArrowDownToLine, X, Play, ChevronDown } from 'lucide-react';
import { useDownloadContext } from '@/contexts/DownloadContext';
import { pickDirectory } from '@/utils/tauriCommands';
import type { HubDownloadRecord } from '@/utils/tauriCommands';

function formatSize(bytes: number): string {
  if (bytes === 0) return '\u2014';
  const gb = bytes / 1_073_741_824;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  const mb = bytes / 1_048_576;
  if (mb >= 1) return `${mb.toFixed(0)} MB`;
  return `${(bytes / 1024).toFixed(0)} KB`;
}

function extractQuant(filename: string): string | null {
  const base = filename.replace(/\.gguf$/i, '').split('/').pop() ?? '';
  const stripped = base.replace(/-\d{5}-of-\d{5}$/, '').replace(/-imat$/i, '');
  const m = stripped.match(/[-_]((?:IQ|Q|F|BF|MXFP)\d[A-Z0-9_]*?)$/i);
  if (m) return m[1].toUpperCase();
  const m2 = stripped.match(/((?:IQ|Q|F|BF|MXFP)\d[A-Z0-9_]*)$/i);
  if (m2) return m2[1].toUpperCase();
  return null;
}

export function DownloadFloat() {
  const {
    downloads,
    pendingDownloads,
    startDownload,
    cancelDownload,
    activeCount,
    pendingCount,
  } = useDownloadContext();
  const [expanded, setExpanded] = useState(false);

  const totalVisible = activeCount + pendingCount;

  const handleResume = useCallback(async (record: HubDownloadRecord) => {
    const dirPath = record.dest_path || await pickDirectory();
    if (!dirPath) return;
    startDownload(record.model_id, { name: record.filename, size: record.file_size }, dirPath);
  }, [startDownload]);

  // Don't render if nothing to show
  if (totalVisible === 0) return null;

  // Merge active downloads + pending into a display list
  const items: {
    key: string;
    filename: string;
    modelId: string;
    quant: string | null;
    isActive: boolean;
    bytes: number;
    total: number;
    speed: number;
    record?: HubDownloadRecord;
  }[] = [];

  // Active downloads
  for (const [key, progress] of downloads) {
    const [modelId, ...rest] = key.split('/');
    const filename = rest.join('/');
    items.push({
      key,
      filename,
      modelId,
      quant: extractQuant(filename),
      isActive: true,
      bytes: progress.bytes ?? 0,
      total: progress.total ?? 0,
      speed: progress.speed_kbps ?? 0,
      record: undefined,
    });
  }

  // Pending (paused) — skip keys that are already active
  for (const [key, record] of pendingDownloads) {
    if (downloads.has(key)) continue;
    items.push({
      key,
      filename: record.filename,
      modelId: record.model_id,
      quant: extractQuant(record.filename),
      isActive: false,
      bytes: record.bytes_downloaded,
      total: record.file_size,
      speed: 0,
      record,
    });
  }

  // Aggregate progress for the pill
  const totalBytes = items.reduce((a, b) => a + b.bytes, 0);
  const totalSize = items.reduce((a, b) => a + b.total, 0);
  const overallPct = totalSize > 0 ? Math.round((totalBytes / totalSize) * 100) : 0;

  return (
    <div className="fixed bottom-6 right-6 z-50 flex flex-col items-end gap-2">
      {/* Expanded panel */}
      {expanded ? (
        <div className="bg-card border border-border rounded-lg shadow-2xl w-80 max-h-64 overflow-y-auto">
          <div className="flex items-center justify-between px-3 py-2 border-b border-border">
            <span className="text-xs font-medium text-foreground/70">Downloads</span>
            <button
              onClick={() => setExpanded(false)}
              className="p-0.5 rounded hover:bg-muted text-muted-foreground"
            >
              <ChevronDown size={14} />
            </button>
          </div>
          <div className="divide-y divide-border">
            {items.map((item) => (
              <div key={item.key} className="px-3 py-2 space-y-1">
                <div className="flex items-center gap-1.5">
                  <span className="text-xs font-medium text-foreground truncate flex-1">
                    {item.filename}
                  </span>
                  {item.quant ? (
                    <span className="text-[9px] font-mono px-1 py-0.5 rounded bg-muted text-muted-foreground shrink-0">
                      {item.quant}
                    </span>
                  ) : null}
                </div>
                <div className="flex items-center gap-2">
                  {/* Progress bar */}
                  <div className="flex-1 h-1.5 bg-muted rounded-full overflow-hidden">
                    <div
                      className={`h-full rounded-full transition-all ${
                        item.isActive ? 'bg-blue-500' : 'bg-yellow-500/60'
                      }`}
                      style={{ width: `${item.total > 0 ? (item.bytes / item.total) * 100 : 0}%` }}
                    />
                  </div>
                  {/* Info */}
                  <span className="text-[10px] text-muted-foreground whitespace-nowrap">
                    {item.isActive && item.speed > 0
                      ? `${(item.speed / 1024).toFixed(1)} MB/s`
                      : `${formatSize(item.bytes)} / ${formatSize(item.total)}`
                    }
                  </span>
                  {/* Action buttons */}
                  {!item.isActive && item.record ? (
                    <button
                      onClick={() => handleResume(item.record!)}
                      className="p-0.5 rounded hover:bg-muted text-blue-400"
                      title="Resume"
                    >
                      <Play size={12} />
                    </button>
                  ) : null}
                  <button
                    onClick={() => cancelDownload(item.key)}
                    className="p-0.5 rounded hover:bg-destructive/20 text-muted-foreground hover:text-destructive"
                    title="Cancel and delete"
                  >
                    <X size={12} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      {/* Collapsed pill */}
      <button
        onClick={() => setExpanded(prev => !prev)}
        className="flex items-center gap-2 px-3 py-2 bg-card border border-border rounded-full shadow-lg hover:bg-muted transition-colors"
      >
        <div className="relative">
          <ArrowDownToLine size={16} className={activeCount > 0 ? 'text-blue-400' : 'text-yellow-500'} />
          {activeCount > 0 ? (
            <span className="absolute -top-0.5 -right-0.5 h-2 w-2 rounded-full bg-blue-400 animate-pulse" />
          ) : null}
        </div>
        <span className="text-xs font-medium text-foreground">
          {activeCount > 0 ? `${overallPct}%` : `${pendingCount} paused`}
        </span>
      </button>
    </div>
  );
}
