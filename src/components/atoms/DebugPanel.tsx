import { useAgentContext } from '../../contexts/AgentContext';
import { useChatContext } from '../../contexts/ChatContext';
import { useModelContext } from '../../contexts/ModelContext';
import { useSystemResources } from '../../contexts/SystemResourcesContext';
import { useConnection } from '../../hooks/useConnection';
import { useUIContext } from '../../hooks/useUIContext';

const LABEL_TRIM = 14;
const WORKER_ID_TRIM = 8;
const SHORT_DEFAULT = 28;
const SHORT_SM = 20;
const SHORT_MD = 22;
const SHORT_LG = 24;
const SHORT_XL = 26;

const short = (s: string | null | undefined, n = SHORT_DEFAULT): string => {
  if (!s) return '—';
  if (s.length > n) return `…${s.slice(-n)}`;
  return s;
};

const mb = (v: number | undefined): string => (v == null ? '—' : `${v} MB`);

const gb = (v: number | undefined | null): string => (v == null ? '—' : `${v.toFixed(2)} GB`);

const pct = (used: number | undefined, total: number): string => {
  if (used == null || !total) return '—';
  return `${gb(used)} / ${gb(total)} (${Math.round((used / total) * 100)}%)`;
};

interface Row {
  label: string;
  value: string | number | boolean | null | undefined;
  highlight?: boolean;
}

const fmtValue = (value: Row['value']): string => {
  if (value === true) return 'true';
  if (value === false) return 'false';
  if (value == null) return '—';
  return String(value);
};

const valueColor = (value: Row['value'], highlight: boolean | undefined): string => {
  if (highlight) return 'text-red-400';
  if (value === true) return 'text-green-400';
  if (value === false) return 'text-red-400';
  return 'text-foreground';
};

const Section = ({ title, rows }: { title: string; rows: Row[] }): JSX.Element => (
  <div className="mb-4">
    <div className="mb-1 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
      {title}
    </div>
    {rows.map(({ label, value, highlight }) => {
      const color = valueColor(value, highlight);
      const display = fmtValue(value);
      return (
        <div key={label} className="flex justify-between gap-2 py-[1px] font-mono text-xs">
          <span className="shrink-0 text-muted-foreground">{label}</span>
          <span className={`break-all text-right ${color}`}>{display}</span>
        </div>
      );
    })}
  </div>
);

export const DebugPanelContent = (): JSX.Element => {
  const conn = useConnection();
  const chat = useChatContext();
  const model = useModelContext();
  const agent = useAgentContext();
  const ui = useUIContext();
  const sys = useSystemResources();

  const workerRows: Row[] = Object.entries(agent.agentStatuses).map(([id, s]) => {
    const progress = s.loading_progress != null ? ` ${s.loading_progress}%` : '';
    const worker = s.worker_id ? ` [${s.worker_id.slice(0, WORKER_ID_TRIM)}]` : '';
    return {
      label: id.slice(0, LABEL_TRIM),
      value: `${s.status}${progress}${worker}`,
      highlight: s.status === 'generating',
    };
  });

  const tokenDisplay =
    chat.tokensUsed != null ? `${chat.tokensUsed} / ${chat.maxTokens ?? '?'}` : '—';

  return (
    <div className="space-y-1">
      <Section
        title="Connection"
        rows={[
          { label: 'connected', value: conn.connected },
          { label: 'reconnecting', value: conn.reconnecting, highlight: conn.reconnecting },
          { label: 'attempt', value: conn.attempt },
        ]}
      />
      <Section
        title="Chat"
        rows={[
          { label: 'isLoading', value: chat.isLoading, highlight: chat.isLoading },
          { label: 'messages', value: chat.messages.length },
          { label: 'tokens', value: tokenDisplay },
          { label: 'streamStatus', value: short(chat.streamStatus, SHORT_XL) },
          { label: 'conversationId', value: short(chat.currentConversationId, SHORT_MD) },
          { label: 'workerId', value: short(chat.currentConversationWorkerId, SHORT_MD) },
          { label: 'error', value: short(chat.error, SHORT_XL), highlight: !!chat.error },
        ]}
      />
      <Section
        title="Model"
        rows={[
          { label: 'provider', value: model.activeProvider },
          { label: 'loaded', value: model.status.loaded },
          {
            label: 'loading',
            value: model.status.loading ? '✓' : '✗',
            highlight: !!model.status.loading,
          },
          {
            label: 'generating',
            value: model.status.generating ? '✓' : '✗',
            highlight: !!model.status.generating,
          },
          { label: 'model', value: short(model.modelName, SHORT_LG) },
          { label: 'ctx_size', value: model.status.context_size ?? '—' },
          { label: 'gpu_layers', value: model.status.gpu_layers ?? '—' },
          { label: 'memory', value: mb(model.status.memory_usage_mb) },
          { label: 'finish_reason', value: model.status.last_finish_reason ?? '—' },
        ]}
      />
      <Section
        title="Agents"
        rows={[
          { label: 'conversationAgent', value: short(agent.conversationAgent?.name, SHORT_SM) },
          { label: 'stagedAgent', value: short(agent.stagedAgent?.name, SHORT_SM) },
          {
            label: 'activating',
            value: short(agent.activatingAgentId, SHORT_SM),
            highlight: !!agent.activatingAgentId,
          },
          { label: 'workers', value: Object.keys(agent.agentStatuses).length },
          ...workerRows,
        ]}
      />
      <Section
        title="Resources"
        rows={[
          { label: 'VRAM', value: pct(sys.usage.vram_used_gb, sys.totalVramGb) },
          { label: 'RAM', value: pct(sys.usage.app_ram_gb, sys.totalRamGb) },
          { label: 'CPU', value: sys.usage.cpu != null ? `${sys.usage.cpu.toFixed(1)}%` : '—' },
        ]}
      />
      <Section
        title="UI"
        rows={[
          { label: 'rightSidebar', value: ui.isRightSidebarOpen ? '✓' : '✗' },
          { label: 'modelConfig', value: ui.isModelConfigOpen ? '✓' : '✗' },
          { label: 'appSettings', value: ui.isAppSettingsOpen ? '✓' : '✗' },
          { label: 'eventLog', value: ui.isEventLogOpen ? '✓' : '✗' },
          { label: 'browserView', value: ui.isBrowserViewOpen ? '✓' : '✗' },
          { label: 'browserUrl', value: short(ui.browserViewUrl, SHORT_LG) },
        ]}
      />
    </div>
  );
};
