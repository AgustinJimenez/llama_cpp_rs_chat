import { createContext, useContext, useCallback, useMemo, useState, type ReactNode } from 'react';

import type { Agent } from '../types';

type AgentPayload = Partial<Agent> & { name: string; provider_id: string };

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
}

const AgentContext = createContext<AgentContextValue | null>(null);

export const AgentProvider = ({ children }: { children: ReactNode }) => {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [activeConversationAgent, setActiveConversationAgent] = useState<Agent | null>(null);
  const [stagedAgent, setStagedAgent] = useState<Agent | null>(null);

  const loadAgents = useCallback(async () => {
    const list = await listAgentsRequest();
    setAgents(list);
  }, []);

  const loadConversationAgent = useCallback(async (conversationId: string) => {
    const agent = await getConversationAgentRequest(conversationId);
    setActiveConversationAgent(agent);
    return agent;
  }, []);

  const setConversationAgent = useCallback(
    async (conversationId: string, agentId: string | null) => {
      await setConversationAgentRequest(conversationId, agentId);
      if (agentId) {
        // Re-fetch to get full agent data
        const agent = await getConversationAgentRequest(conversationId);
        setActiveConversationAgent(agent);
      } else {
        setActiveConversationAgent(null);
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
