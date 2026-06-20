import {
  createContext,
  useContext,
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';

import { getConversationFromUrl } from '../hooks/useConversationUrl';
import type { Agent } from '../types';
import {
  listAgents,
  createAgent as createAgentCmd,
  updateAgent as updateAgentCmd,
  deleteAgent as deleteAgentCmd,
  getConversationAgent,
  setConversationAgent as setConversationAgentCmd,
  fetchAgentStatuses as fetchAgentStatusesCmd,
  activateAgent as activateAgentCmd,
  stopAgent as stopAgentCmd,
} from '../utils/tauriCommands';

type AgentPayload = Partial<Agent> & { name: string; provider_id: string };

// Caches the last-resolved conversation->agent pairing so a page refresh can show the
// right agent immediately instead of flashing "No agent" while /api/.../agent loads.
const LAST_CONVERSATION_AGENT_KEY = 'lastConversationAgent';

function readCachedConversationAgent(conversationId: string | null): Agent | null {
  if (!conversationId) return null;
  try {
    const raw = localStorage.getItem(LAST_CONVERSATION_AGENT_KEY);
    if (!raw) return null;
    const cached = JSON.parse(raw) as { conversationId: string; agent: Agent | null };
    return cached.conversationId === conversationId ? cached.agent : null;
  } catch {
    return null;
  }
}

function writeCachedConversationAgent(conversationId: string, agent: Agent | null) {
  try {
    localStorage.setItem(LAST_CONVERSATION_AGENT_KEY, JSON.stringify({ conversationId, agent }));
  } catch {
    // localStorage unavailable (private mode, quota) — cache is best-effort
  }
}

export type AgentStatus = {
  status: 'idle' | 'active' | 'generating' | 'loading';
  loading_progress?: number;
  worker_id?: string;
};

interface AgentContextValue {
  agents: Agent[];
  conversationAgent: Agent | null;
  /** Agent staged for the next new conversation (no conversationId yet). */
  stagedAgent: Agent | null;
  setStagedAgent: (agent: Agent | null) => void;
  loadAgents: () => Promise<void>;
  loadConversationAgent: (conversationId: string) => Promise<Agent | null>;
  setConversationAgent: (conversationId: string, agentId: string | null) => Promise<void>;
  createAgent: (agent: AgentPayload) => Promise<Agent>;
  updateAgent: (id: string, agent: AgentPayload) => Promise<void>;
  deleteAgent: (id: string) => Promise<void>;
  /** Live status for each agent (idle / active / generating). */
  agentStatuses: Record<string, AgentStatus>;
  /** ID of the agent currently being activated (worker spinning up), or null. */
  activatingAgentId: string | null;
  fetchAgentStatuses: () => Promise<void>;
  activateAgent: (id: string) => Promise<void>;
  stopAgent: (id: string) => Promise<void>;
}

const AgentContext = createContext<AgentContextValue | null>(null);

export const AgentProvider = ({ children }: { children: ReactNode }) => {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [activeConversationAgent, setActiveConversationAgent] = useState<Agent | null>(() =>
    readCachedConversationAgent(getConversationFromUrl()),
  );
  const [stagedAgent, setStagedAgent] = useState<Agent | null>(null);
  const [agentStatuses, setAgentStatuses] = useState<Record<string, AgentStatus>>({});
  const [activatingAgentId, setActivatingAgentId] = useState<string | null>(null);

  const loadAgentsList = useCallback(async () => {
    const list = await listAgents();
    setAgents(list);
  }, []);

  // Fetch agents on mount so the AgentPicker dropdown is populated immediately
  useEffect(() => {
    loadAgentsList().catch(() => {});
  }, [loadAgentsList]);

  const loadConversationAgentCb = useCallback(async (conversationId: string) => {
    const agent = await getConversationAgent(conversationId);
    setActiveConversationAgent(agent);
    writeCachedConversationAgent(conversationId, agent);
    return agent;
  }, []);

  const setConversationAgentCb = useCallback(
    async (conversationId: string, agentId: string | null) => {
      await setConversationAgentCmd(conversationId, agentId);
      if (agentId) {
        const agent = await getConversationAgent(conversationId);
        setActiveConversationAgent(agent);
        writeCachedConversationAgent(conversationId, agent);
      } else {
        setActiveConversationAgent(null);
        writeCachedConversationAgent(conversationId, null);
      }
    },
    [],
  );

  const createAgentCb = useCallback(async (data: AgentPayload) => {
    const agent = await createAgentCmd(data as unknown as Record<string, unknown>);
    setAgents((prev) => [...prev, agent]);
    return agent;
  }, []);

  const updateAgentCb = useCallback(async (id: string, data: AgentPayload) => {
    await updateAgentCmd(id, data as unknown as Record<string, unknown>);
    setAgents((prev) => prev.map((a) => (a.id === id ? { ...a, ...data } : a)));
    setActiveConversationAgent((prev) => (prev?.id === id ? { ...prev, ...data } : prev));
  }, []);

  const deleteAgentCb = useCallback(async (id: string) => {
    await deleteAgentCmd(id);
    setAgents((prev) => prev.filter((a) => a.id !== id));
    setActiveConversationAgent((prev) => (prev?.id === id ? null : prev));
    setAgentStatuses((prev) => {
      const { [id]: _removed, ...rest } = prev;
      return rest;
    });
  }, []);

  const fetchAgentStatusesCb = useCallback(async () => {
    const statuses = await fetchAgentStatusesCmd();
    setAgentStatuses(statuses);
  }, []);

  const activateAgentCb = useCallback(
    async (id: string) => {
      setActivatingAgentId(id);
      setAgentStatuses((prev) => ({ ...prev, [id]: { status: 'loading' } }));

      // Poll while the model loads so loading_progress is reflected in the UI
      const LOAD_POLL_MS = 500;
      const pollInterval = setInterval(() => {
        fetchAgentStatusesCb().catch(() => {});
      }, LOAD_POLL_MS);

      try {
        const result = await activateAgentCmd(id);
        setAgentStatuses((prev) => ({ ...prev, [id]: result }));
      } catch (err) {
        setAgentStatuses((prev) => ({ ...prev, [id]: { status: 'idle' } }));
        throw err;
      } finally {
        clearInterval(pollInterval);
        setActivatingAgentId((prev) => (prev === id ? null : prev));
      }
    },
    [fetchAgentStatusesCb],
  );

  const stopAgentCb = useCallback(async (id: string) => {
    await stopAgentCmd(id);
    setAgentStatuses((prev) => ({ ...prev, [id]: { status: 'idle' } }));
  }, []);

  const value = useMemo<AgentContextValue>(
    () => ({
      agents,
      conversationAgent: activeConversationAgent,
      stagedAgent,
      setStagedAgent,
      loadAgents: loadAgentsList,
      loadConversationAgent: loadConversationAgentCb,
      setConversationAgent: setConversationAgentCb,
      createAgent: createAgentCb,
      updateAgent: updateAgentCb,
      deleteAgent: deleteAgentCb,
      agentStatuses,
      activatingAgentId,
      fetchAgentStatuses: fetchAgentStatusesCb,
      activateAgent: activateAgentCb,
      stopAgent: stopAgentCb,
    }),
    [
      agents,
      activeConversationAgent,
      stagedAgent,
      setStagedAgent,
      loadAgentsList,
      loadConversationAgentCb,
      setConversationAgentCb,
      createAgentCb,
      updateAgentCb,
      deleteAgentCb,
      agentStatuses,
      activatingAgentId,
      fetchAgentStatusesCb,
      activateAgentCb,
      stopAgentCb,
    ],
  );

  return <AgentContext.Provider value={value}>{children}</AgentContext.Provider>;
};

// eslint-disable-next-line react-refresh/only-export-components
export function useAgentContext() {
  const ctx = useContext(AgentContext);
  if (!ctx) throw new Error('useAgentContext must be used within AgentProvider');
  return ctx;
}
