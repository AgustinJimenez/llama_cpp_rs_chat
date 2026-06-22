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
  status: 'idle' | 'active' | 'generating';
  worker_id?: string;
};

async function parseJsonResponse<T>(response: Response): Promise<T> {
  if (!response.ok) {
    const body = await response.json().catch(() => null);
    const message =
      body && typeof body === 'object' && 'error' in body
        ? String((body as { error: unknown }).error)
        : `Request failed with ${response.status}`;
    throw new Error(message);
  }
  return response.json() as Promise<T>;
}

async function listAgentsRequest(): Promise<Agent[]> {
  const response = await fetch('/api/agents');
  return parseJsonResponse<Agent[]>(response);
}

async function createAgentRequest(agent: AgentPayload): Promise<Agent> {
  const response = await fetch('/api/agents', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(agent),
  });
  return parseJsonResponse<Agent>(response);
}

async function updateAgentRequest(id: string, agent: AgentPayload): Promise<void> {
  const response = await fetch(`/api/agents/${encodeURIComponent(id)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(agent),
  });
  await parseJsonResponse(response);
}

async function deleteAgentRequest(id: string): Promise<void> {
  const response = await fetch(`/api/agents/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
  await parseJsonResponse(response);
}

async function fetchAgentStatusesRequest(): Promise<Record<string, AgentStatus>> {
  const response = await fetch('/api/agents/statuses');
  return parseJsonResponse<Record<string, AgentStatus>>(response);
}

async function activateAgentRequest(id: string): Promise<AgentStatus> {
  const response = await fetch(`/api/agents/${encodeURIComponent(id)}/activate`, {
    method: 'POST',
  });
  return parseJsonResponse<AgentStatus>(response);
}

async function stopAgentRequest(id: string): Promise<void> {
  const response = await fetch(`/api/agents/${encodeURIComponent(id)}/stop`, {
    method: 'POST',
  });
  await parseJsonResponse(response);
}

async function getConversationAgentRequest(conversationId: string): Promise<Agent | null> {
  const response = await fetch(`/api/conversations/${encodeURIComponent(conversationId)}/agent`);
  const body = await parseJsonResponse<{ agent: Agent | null }>(response);
  return body.agent;
}

async function setConversationAgentRequest(
  conversationId: string,
  agentId: string | null,
): Promise<void> {
  const response = await fetch(`/api/conversations/${encodeURIComponent(conversationId)}/agent`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ agent_id: agentId }),
  });
  await parseJsonResponse(response);
}

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

// react-doctor-disable-next-line react-doctor/prefer-useReducer -- genuinely distinct agent management states
export const AgentProvider = ({ children }: { children: ReactNode }) => {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [activeConversationAgent, setActiveConversationAgent] = useState<Agent | null>(() =>
    readCachedConversationAgent(getConversationFromUrl()),
  );
  const [stagedAgent, setStagedAgent] = useState<Agent | null>(null);
  const [agentStatuses, setAgentStatuses] = useState<Record<string, AgentStatus>>({});
  const [activatingAgentId, setActivatingAgentId] = useState<string | null>(null);

  const loadAgents = useCallback(async () => {
    const list = await listAgentsRequest();
    setAgents(list);
  }, []);

  // Fetch agents on mount so the AgentPicker dropdown is populated immediately
  useEffect(() => {
    loadAgents().catch(() => {});
  }, [loadAgents]);

  const loadConversationAgent = useCallback(async (conversationId: string) => {
    const agent = await getConversationAgentRequest(conversationId);
    setActiveConversationAgent(agent);
    writeCachedConversationAgent(conversationId, agent);
    return agent;
  }, []);

  const setConversationAgent = useCallback(
    async (conversationId: string, agentId: string | null) => {
      await setConversationAgentRequest(conversationId, agentId);
      if (agentId) {
        // Re-fetch to get full agent data
        const agent = await getConversationAgentRequest(conversationId);
        setActiveConversationAgent(agent);
        writeCachedConversationAgent(conversationId, agent);
      } else {
        setActiveConversationAgent(null);
        writeCachedConversationAgent(conversationId, null);
      }
    },
    [],
  );

  const createAgent = useCallback(async (data: AgentPayload) => {
    const agent = await createAgentRequest(data);
    setAgents((prev) => [...prev, agent]);
    return agent;
  }, []);

  const updateAgent = useCallback(async (id: string, data: AgentPayload) => {
    await updateAgentRequest(id, data);
    setAgents((prev) => prev.map((a) => (a.id === id ? { ...a, ...data } : a)));
    setActiveConversationAgent((prev) => (prev?.id === id ? { ...prev, ...data } : prev));
  }, []);

  const deleteAgent = useCallback(async (id: string) => {
    await deleteAgentRequest(id);
    setAgents((prev) => prev.filter((a) => a.id !== id));
    setActiveConversationAgent((prev) => (prev?.id === id ? null : prev));
    setAgentStatuses((prev) => {
      const { [id]: _removed, ...rest } = prev;
      return rest;
    });
  }, []);

  const fetchAgentStatuses = useCallback(async () => {
    const statuses = await fetchAgentStatusesRequest();
    setAgentStatuses(statuses);
  }, []);

  const activateAgent = useCallback(async (id: string) => {
    setActivatingAgentId(id);
    setAgentStatuses((prev) => ({ ...prev, [id]: { status: 'active' } }));
    try {
      const result = await activateAgentRequest(id);
      setAgentStatuses((prev) => ({ ...prev, [id]: result }));
    } catch (err) {
      setAgentStatuses((prev) => ({ ...prev, [id]: { status: 'idle' } }));
      throw err;
    } finally {
      setActivatingAgentId((prev) => (prev === id ? null : prev));
    }
  }, []);

  const stopAgent = useCallback(async (id: string) => {
    await stopAgentRequest(id);
    setAgentStatuses((prev) => ({ ...prev, [id]: { status: 'idle' } }));
  }, []);

  const value = useMemo<AgentContextValue>(
    () => ({
      agents,
      conversationAgent: activeConversationAgent,
      stagedAgent,
      setStagedAgent,
      loadAgents,
      loadConversationAgent,
      setConversationAgent,
      createAgent,
      updateAgent,
      deleteAgent,
      agentStatuses,
      activatingAgentId,
      fetchAgentStatuses,
      activateAgent,
      stopAgent,
    }),
    [
      agents,
      activeConversationAgent,
      stagedAgent,
      setStagedAgent,
      loadAgents,
      loadConversationAgent,
      setConversationAgent,
      createAgent,
      updateAgent,
      deleteAgent,
      agentStatuses,
      activatingAgentId,
      fetchAgentStatuses,
      activateAgent,
      stopAgent,
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
