import { useCallback, useEffect, useMemo, useState } from 'react';

import {
  fetchGitStatus,
  processCommits,
} from '../../../utils/gitGraph';
import type { AssignedCommit, GraphEdge } from '../../../utils/gitGraph';

import { fetchGitLog } from './utils';

export interface GitGraphState {
  path: string;
  setPath: (p: string) => void;
  recentPaths: string[];
  loading: boolean;
  loadError: string | null;
  commits: AssignedCommit[];
  edges: GraphEdge[];
  maxLane: number;
  wipChangesCount: number;
  selectedHash: string | null;
  setSelectedHash: (h: string | null) => void;
  filteredCommits: AssignedCommit[];
  displayRows: Array<{ commit: AssignedCommit; refIndex: number }>;
  adjCommits: AssignedCommit[];
  adjEdges: GraphEdge[];
  loadGraph: (p: string) => Promise<void>;
}

export function useGitGraph(searchQuery: string): GitGraphState {
  const [path, setPath] = useState(() => localStorage.getItem('gitGraphPath') ?? '');
  const [recentPaths, setRecentPaths] = useState<string[]>(() => {
    try { return JSON.parse(localStorage.getItem('gitGraphPaths') ?? '[]') as string[]; } catch { return []; }
  });
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [commits, setCommits] = useState<AssignedCommit[]>([]);
  const [edges, setEdges] = useState<GraphEdge[]>([]);
  const [maxLane, setMaxLane] = useState(0);
  const [wipChangesCount, setWipChangesCount] = useState(0);
  const [selectedHash, setSelectedHash] = useState<string | null>(null);

  const loadGraph = useCallback(async (repoPath: string) => {
    const trimmed = repoPath.trim();
    if (!trimmed) return;
    setLoading(true);
    setLoadError(null);
    try {
      const data = await fetchGitLog(trimmed);
      if (data.error) {
        setLoadError(data.error);
        setCommits([]);
        setEdges([]);
        setMaxLane(0);
      } else {
        const result = processCommits(data.commits);
        setCommits(result.commits);
        setEdges(result.edges);
        setMaxLane(result.maxLane);
        setSelectedHash(null);
        localStorage.setItem('gitGraphPath', trimmed);
        setRecentPaths((prev) => {
          const MAX_RECENT = 20;
          const next = [trimmed, ...prev.filter((p) => p !== trimmed)].slice(0, MAX_RECENT);
          localStorage.setItem('gitGraphPaths', JSON.stringify(next));
          return next;
        });
        fetchGitStatus(trimmed)
          .then((s) => { setWipChangesCount(s.staged.length + s.unstaged.length); })
          .catch(() => {});
      }
    } catch (error) {
      setLoadError(error instanceof Error ? error.message : 'Unknown error');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    const saved = localStorage.getItem('gitGraphPath');
    if (saved) void loadGraph(saved);
  }, [loadGraph]);

  const filteredCommits = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return commits;
    return commits.filter(
      (c) => c.subject.toLowerCase().includes(q) || c.hash.startsWith(q) || c.author.toLowerCase().includes(q),
    );
  }, [commits, searchQuery]);

  const displayRows = useMemo(() => {
    const rows: Array<{ commit: AssignedCommit; refIndex: number }> = [];
    for (const c of commits) {
      const count = Math.max(1, c.refs.length);
      for (let i = 0; i < count; i++) rows.push({ commit: c, refIndex: i });
    }
    return rows;
  }, [commits]);

  const { adjCommits, adjEdges } = useMemo(() => {
    const rowMap = new Map<number, number>();
    let dRow = 0;
    for (const c of commits) {
      rowMap.set(c.row, dRow);
      dRow += Math.max(1, c.refs.length);
    }
    return {
      adjCommits: commits.map((c) => ({ ...c, row: rowMap.get(c.row) ?? c.row })),
      adjEdges: edges.map((e) => ({
        ...e,
        fromRow: rowMap.get(e.fromRow) ?? e.fromRow,
        toRow: rowMap.get(e.toRow) ?? e.toRow,
      })),
    };
  }, [commits, edges]);

  return {
    path,
    setPath,
    recentPaths,
    loading,
    loadError,
    commits,
    edges,
    maxLane,
    wipChangesCount,
    selectedHash,
    setSelectedHash,
    filteredCommits,
    displayRows,
    adjCommits,
    adjEdges,
    loadGraph,
  };
}
