import { Plus, Trash2, RefreshCw, Server, ToggleLeft, ToggleRight } from 'lucide-react';
import React, { useState } from 'react';
import { toast } from 'react-hot-toast';
import { useTranslation } from 'react-i18next';

import { useMcpServers } from '../../hooks/useMcpServers';
import type { McpServerConfig, McpTransport } from '../../types';

const RADIX_36 = 36;
const ID_SLICE_END = 8;

function generateId(): string {
  return `mcp_${Date.now()}_${Math.random().toString(RADIX_36).slice(2, ID_SLICE_END)}`;
}

/* eslint-disable max-lines-per-function */
// react-doctor-disable-next-line react-doctor/prefer-useReducer -- genuinely distinct states
export const McpSettingsSection: React.FC = () => {
  const { t } = useTranslation();
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
  const refreshLabel = isRefreshing ? t('mcp.refreshing') : t('common.refresh');

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Server className="size-4 text-muted-foreground" />
          <span className="text-sm font-medium text-foreground">{t('mcp.title')}</span>
        </div>
        <div className="flex gap-2">
          <button
            className="flat-button flex items-center gap-1 bg-muted px-3 py-1 text-xs"
            onClick={handleRefresh}
            disabled={isRefreshing}
          >
            <RefreshCw className={`size-3 ${isRefreshing ? 'animate-spin' : ''}`} />
            {refreshLabel}
          </button>
          <button
            className="flat-button flex items-center gap-1 bg-muted px-3 py-1 text-xs"
            onClick={() => setShowAddForm(!showAddForm)}
          >
            <Plus className="size-3" />
            {t('mcp.addServer')}
          </button>
        </div>
      </div>

      <p className="text-xs text-muted-foreground">{t('mcp.description')}</p>

      {/* Add Server Form */}
      {!!showAddForm && (
        <div className="space-y-2 rounded-lg border border-border bg-muted/50 p-3">
          <div className="space-y-1">
            <label htmlFor="mcp-server-name" className="text-xs font-medium text-foreground">
              {t('mcp.nameLabel')}
            </label>
            <input
              id="mcp-server-name"
              className="w-full rounded border border-border bg-muted px-2 py-1.5 text-sm text-foreground"
              placeholder={t('mcp.namePlaceholder')}
              value={formName}
              onChange={(e) => setFormName(e.target.value)}
            />
          </div>

          <div className="space-y-1">
            <label htmlFor="mcp-transport" className="text-xs font-medium text-foreground">
              {t('mcp.transportLabel')}
            </label>
            <select
              id="mcp-transport"
              className="w-full rounded border border-border bg-muted px-2 py-1.5 text-sm text-foreground"
              value={formTransport}
              onChange={(e) => setFormTransport(e.target.value as 'Stdio' | 'Http')}
            >
              <option value="Stdio">{t('mcp.transportStdio')}</option>
              <option value="Http">{t('mcp.transportHttp')}</option>
            </select>
          </div>

          {formTransport === 'Stdio' && (
            <>
              <div className="space-y-1">
                <label htmlFor="mcp-command" className="text-xs font-medium text-foreground">
                  {t('mcp.commandLabel')}
                </label>
                <input
                  id="mcp-command"
                  className="w-full rounded border border-border bg-muted px-2 py-1.5 text-sm text-foreground"
                  placeholder={t('mcp.commandPlaceholder')}
                  value={formCommand}
                  onChange={(e) => setFormCommand(e.target.value)}
                />
              </div>
              <div className="space-y-1">
                <label htmlFor="mcp-args" className="text-xs font-medium text-foreground">
                  {t('mcp.argsLabel')}
                </label>
                <input
                  id="mcp-args"
                  className="w-full rounded border border-border bg-muted px-2 py-1.5 text-sm text-foreground"
                  placeholder={t('mcp.argsPlaceholder')}
                  value={formArgs}
                  onChange={(e) => setFormArgs(e.target.value)}
                />
              </div>
            </>
          )}
          {formTransport !== 'Stdio' && (
            <div className="space-y-1">
              <label htmlFor="mcp-url" className="text-xs font-medium text-foreground">
                {t('mcp.urlLabel')}
              </label>
              <input
                id="mcp-url"
                className="w-full rounded border border-border bg-muted px-2 py-1.5 text-sm text-foreground"
                placeholder={t('mcp.urlPlaceholder')}
                value={formUrl}
                onChange={(e) => setFormUrl(e.target.value)}
              />
            </div>
          )}

          <div className="flex gap-2 pt-1">
            <button
              className="flat-button bg-primary px-4 py-1.5 text-xs text-white"
              onClick={handleAdd}
            >
              {t('common.add')}
            </button>
            <button
              className="flat-button bg-muted px-4 py-1.5 text-xs"
              onClick={() => setShowAddForm(false)}
            >
              {t('common.cancel')}
            </button>
          </div>
        </div>
      )}

      {/* Server List */}
      {servers.length === 0 && !showAddForm && (
        <p className="py-2 text-xs italic text-muted-foreground">{t('mcp.noServers')}</p>
      )}

      {servers.map((server) => {
        const status = getStatus(server.id);
        const transportLabel =
          server.transport.type === 'Stdio'
            ? `${server.transport.command} ${server.transport.args.join(' ')}`
            : server.transport.url;

        const toggleIcon = server.enabled ? (
          <ToggleRight className="size-4 text-green-500" />
        ) : (
          <ToggleLeft className="size-4" />
        );

        const toolsElement =
          status && status.tools.length > 0 ? (
            <div className="mt-1 text-xs text-muted-foreground">
              {t('mcp.toolsList', {
                names: status.tools
                  .map((toolName) => toolName.replace(/^mcp__[^_]+__/, ''))
                  .join(', '),
              })}
            </div>
          ) : null;

        const toggleTitle = server.enabled ? t('common.disable') : t('common.enable');

        return (
          <div
            key={server.id}
            className="flex items-start gap-2 rounded border border-border bg-muted/30 p-2"
          >
            <button
              className="mt-0.5 text-muted-foreground hover:text-foreground"
              onClick={() => handleToggle(server)}
              title={toggleTitle}
              type="button"
            >
              {toggleIcon}
            </button>

            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-foreground">{server.name}</span>
                {!!status && (
                  <span className="text-xs text-green-500">
                    {t('mcp.toolsLabel', { count: status.tool_count })}
                  </span>
                )}
              </div>
              <div className="truncate text-xs text-muted-foreground" title={transportLabel}>
                {server.transport.type}: {transportLabel}
              </div>
              {toolsElement}
            </div>

            <button
              className="text-muted-foreground hover:text-red-500"
              onClick={() => handleDelete(server)}
              title={t('common.remove')}
            >
              <Trash2 className="size-3.5" />
            </button>
          </div>
        );
      })}
    </div>
  );
};
