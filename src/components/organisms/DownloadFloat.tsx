import { ArrowDownToLine, X, Play, ChevronDown } from 'lucide-react';
import { useState, useCallback } from 'react';

import { useDownloadContext } from '@/contexts/DownloadContext';
import { pickDirectory } from '@/utils/tauriCommands';
import type { HubDownloadRecord } from '@/utils/tauriCommands';

const BYTES_PER_GB = 1_073_741_824;
const BYTES_PER_MB = 1_048_576;

function formatSize(bytes: number): string {
  if (bytes === 0) return '\u2014';
  const gb = bytes / BYTES_PER_GB;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  const mb = bytes / BYTES_PER_MB;
  if (mb >= 1) return `${mb.toFixed(0)} MB`;
  return `${(bytes / 1024).toFixed(0)} KB`;
}

function extractQuant(filename: string): string | null {
  const base =
    filename
      .replace(/\.gguf$/i, '')
      .split('/')
      .pop() ?? '';
  const stripped = base.replace(/-\d{5}-of-\d{5}$/, '').replace(/-imat$/i, '');
  const m = stripped.match(/[-_]((?:IQ|Q|F|BF|MXFP)\d[A-Z0-9_]*?)$/i);
  if (m) return m[1].toUpperCase();
  const m2 = stripped.match(/((?:IQ|Q|F|BF|MXFP)\d[A-Z0-9_]*)$/i);
  if (m2) return m2[1].toUpperCase();
  return null;
}

export const DownloadFloat = () => {
  const { downloads, pendingDownloads, startDownload, cancelDownload, activeCount, pendingCount } =
    useDownloadContext();
  const [expanded, setExpanded] = useState(false);

  const totalVisible = activeCount + pendingCount;

  const handleResume = useCallback(
    async (record: HubDownloadRecord) => {
      const dirPath = record.dest_path || (await pickDirectory());
      if (!dirPath) return;
      startDownload(record.model_id, { name: record.filename, size: record.file_size }, dirPath);
    },
    [startDownload],
  );

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
  const arrowIconClass = activeCount > 0 ? 'text-blue-400' : 'text-yellow-500';
  const pillLabel = activeCount > 0 ? `${overallPct}%` : `${pendingCount} paused`;

  return (
    <div className="fixed bottom-6 right-6 z-50 flex flex-col items-end gap-2">
      {/* Expanded panel */}
      {!!expanded && (
        <div className="max-h-64 w-80 overflow-y-auto rounded-lg border border-border bg-card shadow-2xl">
          <div className="flex items-center justify-between border-b border-border px-3 py-2">
            <span className="text-xs font-medium text-foreground/70">Downloads</span>
            <button
              onClick={() => setExpanded(false)}
              className="rounded p-0.5 text-muted-foreground hover:bg-muted"
            >
              <ChevronDown size={14} />
            </button>
          </div>
          <div className="divide-y divide-border">
            {items.map((item) => {
              const speedLabel =
                item.isActive && item.speed > 0
                  ? `${(item.speed / 1024).toFixed(1)} MB/s`
                  : `${formatSize(item.bytes)} / ${formatSize(item.total)}`;
              return (
                <div key={item.key} className="space-y-1 px-3 py-2">
                  <div className="flex items-center gap-1.5">
                    <span className="flex-1 truncate text-xs font-medium text-foreground">
                      {item.filename}
                    </span>
                    {!!item.quant && (
                      <span className="shrink-0 rounded bg-muted px-1 py-0.5 font-mono text-[9px] text-muted-foreground">
                        {item.quant}
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-2">
                    {/* Progress bar */}
                    <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                      <div
                        className={`h-full rounded-full transition-all ${
                          item.isActive ? 'bg-blue-500' : 'bg-yellow-500/60'
                        }`}
                        style={{
                          width: `${item.total > 0 ? (item.bytes / item.total) * 100 : 0}%`,
                        }}
                      />
                    </div>
                    {/* Info */}
                    <span className="whitespace-nowrap text-[10px] text-muted-foreground">
                      {speedLabel}
                    </span>
                    {/* Action buttons */}
                    {!item.isActive && !!item.record && (
                      <button
                        onClick={() => item.record && handleResume(item.record)}
                        className="rounded p-0.5 text-blue-400 hover:bg-muted"
                        title="Resume"
                      >
                        <Play size={12} />
                      </button>
                    )}
                    <button
                      onClick={() => cancelDownload(item.key)}
                      className="rounded p-0.5 text-muted-foreground hover:bg-destructive/20 hover:text-destructive"
                      title="Cancel and delete"
                    >
                      <X size={12} />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Collapsed pill */}
      <button
        onClick={() => setExpanded((prev) => !prev)}
        className="flex items-center gap-2 rounded-full border border-border bg-card px-3 py-2 shadow-lg transition-colors hover:bg-muted"
      >
        <div className="relative">
          <ArrowDownToLine size={16} className={arrowIconClass} />
          {activeCount > 0 && (
            <span className="absolute -right-0.5 -top-0.5 size-2 animate-pulse rounded-full bg-blue-400" />
          )}
        </div>
        <span className="text-xs font-medium text-foreground">{pillLabel}</span>
      </button>
    </div>
  );
};
