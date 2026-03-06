import { useState, useCallback, useEffect } from 'react';
import type { McpServerConfig, McpServerStatus } from '../types';

interface McpState {
  servers: McpServerConfig[];
  statuses: McpServerStatus[];
  isLoading: boolean;
  isRefreshing: boolean;
  error: string | null;
}

export function useMcpServers() {
  const [state, setState] = useState<McpState>({
    servers: [],
    statuses: [],
    isLoading: false,
    isRefreshing: false,
    error: null,
  });

  const loadServers = useCallback(async () => {
    setState(s => ({ ...s, isLoading: true, error: null }));
    try {
      const resp = await fetch('/api/mcp/servers');
      if (!resp.ok) throw new Error(await resp.text());
      const servers: McpServerConfig[] = await resp.json();
      setState(s => ({ ...s, servers, isLoading: false }));
    } catch (err) {
      setState(s => ({
        ...s,
        isLoading: false,
        error: err instanceof Error ? err.message : 'Failed to load MCP servers',
      }));
    }
  }, []);

  const saveServer = useCallback(async (config: McpServerConfig) => {
    const resp = await fetch('/api/mcp/servers', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(config),
    });
    if (!resp.ok) throw new Error(await resp.text());
    await loadServers();
  }, [loadServers]);

  const deleteServer = useCallback(async (id: string) => {
    const resp = await fetch(`/api/mcp/servers/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    });
    if (!resp.ok) throw new Error(await resp.text());
    await loadServers();
  }, [loadServers]);

  const toggleServer = useCallback(async (id: string, enabled: boolean) => {
    const resp = await fetch(`/api/mcp/servers/${encodeURIComponent(id)}/toggle`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ enabled }),
    });
    if (!resp.ok) throw new Error(await resp.text());
    await loadServers();
  }, [loadServers]);

  const refreshConnections = useCallback(async () => {
    setState(s => ({ ...s, isRefreshing: true, error: null }));
    try {
      const resp = await fetch('/api/mcp/refresh', { method: 'POST' });
      if (!resp.ok) throw new Error(await resp.text());
      // After refresh, load status
      const statusResp = await fetch('/api/mcp/tools');
      if (statusResp.ok) {
        const data = await statusResp.json();
        setState(s => ({ ...s, statuses: data.servers || [], isRefreshing: false }));
      } else {
        setState(s => ({ ...s, isRefreshing: false }));
      }
    } catch (err) {
      setState(s => ({
        ...s,
        isRefreshing: false,
        error: err instanceof Error ? err.message : 'Failed to refresh MCP connections',
      }));
    }
  }, []);

  const loadStatus = useCallback(async () => {
    try {
      const resp = await fetch('/api/mcp/tools');
      if (resp.ok) {
        const data = await resp.json();
        setState(s => ({ ...s, statuses: data.servers || [] }));
      }
    } catch {
      // Ignore status load errors
    }
  }, []);

  useEffect(() => {
    loadServers();
    loadStatus();
  }, [loadServers, loadStatus]);

  return {
    ...state,
    saveServer,
    deleteServer,
    toggleServer,
    refreshConnections,
    reloadServers: loadServers,
    reloadStatus: loadStatus,
  };
}
