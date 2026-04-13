import { Plus, Trash2, RefreshCw, Server, ToggleLeft, ToggleRight } from 'lucide-react';
import React, { useState } from 'react';
import { toast } from 'react-hot-toast';

import { useMcpServers } from '../../hooks/useMcpServers';
import type { McpServerConfig, McpTransport } from '../../types';

const RADIX_36 = 36;
const ID_SLICE_END = 8;

function generateId(): string {
  return `mcp_${Date.now()}_${Math.random().toString(RADIX_36).slice(2, ID_SLICE_END)}`;
}

// eslint-disable-next-line max-lines-per-function -- single cohesive form section
export const McpSettingsSection: React.FC = () => {
  const {
    servers,
    statuses,
    isRefreshing,
    saveServer,
    deleteServer,
    toggleServer,
    refreshConnections,
  } = useMcpServers();

  const [showAddForm, setShowAddForm] = useState(false);
  const [formName, setFormName] = useState('');
  const [formTransport, setFormTransport] = useState<'Stdio' | 'Http'>('Stdio');
  const [formCommand, setFormCommand] = useState('');
  const [formArgs, setFormArgs] = useState('');
  const [formUrl, setFormUrl] = useState('');

  const handleAdd = async () => {
    if (!formName.trim()) {
      toast.error('Server name is required');
      return;
    }

    let transport: McpTransport;
    if (formTransport === 'Stdio') {
      if (!formCommand.trim()) {
        toast.error('Command is required for stdio transport');
        return;
      }
      transport = {
        type: 'Stdio',
        command: formCommand.trim(),
        args: formArgs.trim() ? formArgs.trim().split(/\s+/) : [],
        env_vars: {},
      };
    } else {
      if (!formUrl.trim()) {
        toast.error('URL is required for HTTP transport');
        return;
      }
      transport = { type: 'Http', url: formUrl.trim() };
    }

    const config: McpServerConfig = {
      id: generateId(),
      name: formName.trim(),
      transport,
      enabled: true,
    };

    try {
      await saveServer(config);
      toast.success(`MCP server "${config.name}" added`);
      setShowAddForm(false);
      setFormName('');
      setFormCommand('');
      setFormArgs('');
      setFormUrl('');
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to save server');
    }
  };

  const handleDelete = async (server: McpServerConfig) => {
    try {
      await deleteServer(server.id);
      toast.success(`Removed "${server.name}"`);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to delete server');
    }
  };

  const handleToggle = async (server: McpServerConfig) => {
    try {
      await toggleServer(server.id, !server.enabled);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to toggle server');
    }
  };

  const handleRefresh = async () => {
    try {
      await refreshConnections();
      toast.success('MCP connections refreshed');
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to refresh');
    }
  };

  const getStatus = (id: string) => statuses.find((s) => s.id === id);

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Server className="h-4 w-4 text-muted-foreground" />
          <span className="text-sm font-medium text-foreground">MCP Servers</span>
        </div>
        <div className="flex gap-2">
          <button
            className="flat-button bg-muted px-3 py-1 text-xs flex items-center gap-1"
            onClick={handleRefresh}
            disabled={isRefreshing}
          >
            <RefreshCw className={`h-3 w-3 ${isRefreshing ? 'animate-spin' : ''}`} />
            {isRefreshing ? 'Refreshing...' : 'Refresh'}
          </button>
          <button
            className="flat-button bg-muted px-3 py-1 text-xs flex items-center gap-1"
            onClick={() => setShowAddForm(!showAddForm)}
          >
            <Plus className="h-3 w-3" />
            Add Server
          </button>
        </div>
      </div>

      <p className="text-xs text-muted-foreground">
        Connect external tool servers via the Model Context Protocol. After adding servers, click
        Refresh to connect and discover tools.
      </p>

      {/* Add Server Form */}
      {showAddForm ? (
        <div className="space-y-2 p-3 rounded-lg border border-border bg-muted/50">
          <div className="space-y-1">
            <label htmlFor="mcp-server-name" className="text-xs font-medium text-foreground">
              Name
            </label>
            <input
              id="mcp-server-name"
              className="w-full px-2 py-1.5 rounded bg-muted border border-border text-sm text-foreground"
              placeholder="e.g., filesystem"
              value={formName}
              onChange={(e) => setFormName(e.target.value)}
            />
          </div>

          <div className="space-y-1">
            <label htmlFor="mcp-transport" className="text-xs font-medium text-foreground">
              Transport
            </label>
            <select
              id="mcp-transport"
              className="w-full px-2 py-1.5 rounded bg-muted border border-border text-sm text-foreground"
              value={formTransport}
              onChange={(e) => setFormTransport(e.target.value as 'Stdio' | 'Http')}
            >
              <option value="Stdio">Stdio (child process)</option>
              <option value="Http">HTTP/SSE</option>
            </select>
          </div>

          {formTransport === 'Stdio' ? (
            <>
              <div className="space-y-1">
                <label htmlFor="mcp-command" className="text-xs font-medium text-foreground">
                  Command
                </label>
                <input
                  id="mcp-command"
                  className="w-full px-2 py-1.5 rounded bg-muted border border-border text-sm text-foreground"
                  placeholder="e.g., npx"
                  value={formCommand}
                  onChange={(e) => setFormCommand(e.target.value)}
                />
              </div>
              <div className="space-y-1">
                <label htmlFor="mcp-args" className="text-xs font-medium text-foreground">
                  Arguments (space-separated)
                </label>
                <input
                  id="mcp-args"
                  className="w-full px-2 py-1.5 rounded bg-muted border border-border text-sm text-foreground"
                  placeholder="e.g., -y @modelcontextprotocol/server-filesystem /tmp"
                  value={formArgs}
                  onChange={(e) => setFormArgs(e.target.value)}
                />
              </div>
            </>
          ) : (
            <div className="space-y-1">
              <label htmlFor="mcp-url" className="text-xs font-medium text-foreground">
                URL
              </label>
              <input
                id="mcp-url"
                className="w-full px-2 py-1.5 rounded bg-muted border border-border text-sm text-foreground"
                placeholder="http://localhost:3000/sse"
                value={formUrl}
                onChange={(e) => setFormUrl(e.target.value)}
              />
            </div>
          )}

          <div className="flex gap-2 pt-1">
            <button
              className="flat-button bg-primary text-white px-4 py-1.5 text-xs"
              onClick={handleAdd}
            >
              Add
            </button>
            <button
              className="flat-button bg-muted px-4 py-1.5 text-xs"
              onClick={() => setShowAddForm(false)}
            >
              Cancel
            </button>
          </div>
        </div>
      ) : null}

      {/* Server List */}
      {servers.length === 0 && !showAddForm && (
        <p className="text-xs text-muted-foreground italic py-2">
          No MCP servers configured. Click &quot;Add Server&quot; to get started.
        </p>
      )}

      {servers.map((server) => {
        const status = getStatus(server.id);
        const transportLabel =
          server.transport.type === 'Stdio'
            ? `${server.transport.command} ${server.transport.args.join(' ')}`
            : server.transport.url;

        return (
          <div
            key={server.id}
            className="flex items-start gap-2 p-2 rounded border border-border bg-muted/30"
          >
            <button
              className="mt-0.5 text-muted-foreground hover:text-foreground"
              onClick={() => handleToggle(server)}
              title={server.enabled ? 'Disable' : 'Enable'}
            >
              {server.enabled ? (
                <ToggleRight className="h-4 w-4 text-green-500" />
              ) : (
                <ToggleLeft className="h-4 w-4" />
              )}
            </button>

            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-foreground">{server.name}</span>
                {status ? (
                  <span className="text-xs text-green-500">
                    {status.tool_count} tool{status.tool_count !== 1 ? 's' : ''}
                  </span>
                ) : null}
              </div>
              <div className="text-xs text-muted-foreground truncate" title={transportLabel}>
                {server.transport.type}: {transportLabel}
              </div>
              {status && status.tools.length > 0 ? (
                <div className="text-xs text-muted-foreground mt-1">
                  Tools: {status.tools.map((t) => t.replace(/^mcp__[^_]+__/, '')).join(', ')}
                </div>
              ) : null}
            </div>

            <button
              className="text-muted-foreground hover:text-red-500"
              onClick={() => handleDelete(server)}
              title="Remove"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          </div>
        );
      })}
    </div>
  );
};
