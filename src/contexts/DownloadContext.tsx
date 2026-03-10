import { createContext, useContext, useState, useRef, useCallback, useEffect, useMemo, type ReactNode } from 'react';
import { startHubDownload, verifyHubDownloads, deleteHubDownload } from '@/utils/tauriCommands';
import type { DownloadProgress, HubDownloadRecord } from '@/utils/tauriCommands';

interface HubFileRef {
  name: string;
  size: number;
}

interface DownloadContextValue {
  // State
  downloads: Map<string, DownloadProgress>;
  downloadedSet: Set<string>;
  completedDownloads: Map<string, HubDownloadRecord>;
  pendingDownloads: Map<string, HubDownloadRecord>;

  // Actions
  startDownload: (modelId: string, file: HubFileRef, destPath: string) => void;
  cancelDownload: (key: string) => Promise<void>;
  refreshRecords: () => Promise<void>;

  // Derived
  activeCount: number;
  pendingCount: number;
}

const DownloadContext = createContext<DownloadContextValue | null>(null);

export function DownloadProvider({ children }: { children: ReactNode }) {
  const [downloads, setDownloads] = useState<Map<string, DownloadProgress>>(new Map());
  const [downloadedSet, setDownloadedSet] = useState<Set<string>>(new Set());
  const [completedDownloads, setCompletedDownloads] = useState<Map<string, HubDownloadRecord>>(new Map());
  const [pendingDownloads, setPendingDownloads] = useState<Map<string, HubDownloadRecord>>(new Map());
  const abortControllers = useRef<Map<string, AbortController>>(new Map());

  const refreshRecords = useCallback(async () => {
    try {
      const records = await verifyHubDownloads();
      const completedSet = new Set<string>();
      const completedMap = new Map<string, HubDownloadRecord>();
      const pending = new Map<string, HubDownloadRecord>();
      for (const r of records) {
        const key = `${r.model_id}/${r.filename}`;
        if (r.status === 'completed') {
          completedSet.add(key);
          completedMap.set(key, r);
        } else {
          pending.set(key, r);
        }
      }
      setDownloadedSet(completedSet);
      setCompletedDownloads(completedMap);
      setPendingDownloads(pending);
    } catch {
      /* ignore — backend may not be ready yet */
    }
  }, []);

  // Load records once on mount
  useEffect(() => {
    refreshRecords();
  }, [refreshRecords]);

  const startDownload = useCallback((modelId: string, file: HubFileRef, destPath: string) => {
    const key = `${modelId}/${file.name}`;

    // Clear pending state — we're actively downloading now
    setPendingDownloads(prev => {
      const next = new Map(prev);
      next.delete(key);
      return next;
    });

    const controller = startHubDownload(
      modelId,
      file.name,
      destPath,
      (event) => {
        setDownloads(prev => {
          const next = new Map(prev);
          if (event.type === 'done' || event.type === 'error') {
            next.delete(key);
          } else {
            next.set(key, event);
          }
          return next;
        });

        if (event.type === 'done') {
          setDownloadedSet(prev => new Set(prev).add(key));
          abortControllers.current.delete(key);
          refreshRecords();
        }

        if (event.type === 'error') {
          abortControllers.current.delete(key);
          refreshRecords();
        }
      },
    );

    abortControllers.current.set(key, controller);
  }, [refreshRecords]);

  const cancelDownload = useCallback(async (key: string) => {
    // Abort active SSE stream if running
    const ctrl = abortControllers.current.get(key);
    if (ctrl) {
      ctrl.abort();
      abortControllers.current.delete(key);
    }

    // Remove from active downloads UI immediately
    setDownloads(prev => {
      const next = new Map(prev);
      next.delete(key);
      return next;
    });

    // Find the DB record ID so we can delete files + record
    const record = pendingDownloads.get(key) ?? completedDownloads.get(key);
    if (record) {
      try {
        await deleteHubDownload(record.id);
      } catch {
        /* ignore — may already be deleted */
      }
    }

    // Remove from local state
    setPendingDownloads(prev => {
      const next = new Map(prev);
      next.delete(key);
      return next;
    });
    setCompletedDownloads(prev => {
      const next = new Map(prev);
      next.delete(key);
      return next;
    });
    setDownloadedSet(prev => {
      const next = new Set(prev);
      next.delete(key);
      return next;
    });
  }, [pendingDownloads, completedDownloads]);

  const activeCount = downloads.size;
  const pendingCount = pendingDownloads.size;

  const value = useMemo<DownloadContextValue>(() => ({
    downloads, downloadedSet, completedDownloads, pendingDownloads,
    startDownload, cancelDownload, refreshRecords,
    activeCount, pendingCount,
  }), [downloads, downloadedSet, completedDownloads, pendingDownloads,
    startDownload, cancelDownload, refreshRecords, activeCount, pendingCount]);

  return (
    <DownloadContext.Provider value={value}>
      {children}
    </DownloadContext.Provider>
  );
}

// eslint-disable-next-line react-refresh/only-export-components
export function useDownloadContext() {
  const ctx = useContext(DownloadContext);
  if (!ctx) throw new Error('useDownloadContext must be used within DownloadProvider');
  return ctx;
}
