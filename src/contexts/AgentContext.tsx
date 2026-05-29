import { createContext, useContext, useCallback, useMemo, useState, type ReactNode } from 'react';

import type { Agent } from '../types';
import {
  listAgents,
  createAgent as apiCreateAgent,
  updateAgent as apiUpdateAgent,
  deleteAgent as apiDeleteAgent,
  getConversationAgent,
  setConversationAgent as apiSetConversationAgent,
} from '../utils/apiClient';

interface AgentContextValue {
  agents: Agent[];
  conversationAgent: Agent | null;
  /** Agent staged for the next new conversation (no conversationId yet). */
  stagedAgent: Agent | null;
  setStagedAgent: (agent: Agent | null) => void;
  loadAgents: () => Promise<void>;
  loadConversationAgent: (conversationId: string) => Promise<Agent | null>;
  setConversationAgent: (conversationId: string, agentId: string | null) => Promise<void>;
  createAgent: (agent: Partial<Agent> & { name: string; provider_id: string }) => Promise<Agent>;
  updateAgent: (
    id: string,
    agent: Partial<Agent> & { name: string; provider_id: string },
  ) => Promise<void>;
  deleteAgent: (id: string) => Promise<void>;
}

const AgentContext = createContext<AgentContextValue | null>(null);

export const AgentProvider = ({ children }: { children: ReactNode }) => {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [activeConversationAgent, setActiveConversationAgent] = useState<Agent | null>(null);
  const [stagedAgent, setStagedAgent] = useState<Agent | null>(null);

  const loadAgents = useCallback(async () => {
    const list = await listAgents();
    setAgents(list);
  }, []);

  const loadConversationAgent = useCallback(async (conversationId: string) => {
    const agent = await getConversationAgent(conversationId);
    setActiveConversationAgent(agent);
    return agent;
  }, []);

  const setConversationAgent = useCallback(
    async (conversationId: string, agentId: string | null) => {
      await apiSetConversationAgent(conversationId, agentId ?? '');
      if (agentId) {
        // Re-fetch to get full agent data
        const agent = await getConversationAgent(conversationId);
        setActiveConversationAgent(agent);
      } else {
        setActiveConversationAgent(null);
      }
    },
    [],
  );

  const createAgent = useCallback(
    async (data: Partial<Agent> & { name: string; provider_id: string }) => {
      const agent = await apiCreateAgent(data);
      setAgents((prev) => [...prev, agent]);
      return agent;
    },
    [],
  );

  const updateAgent = useCallback(
    async (id: string, data: Partial<Agent> & { name: string; provider_id: string }) => {
      await apiUpdateAgent(id, data);
      setAgents((prev) => prev.map((a) => (a.id === id ? { ...a, ...data } : a)));
      setActiveConversationAgent((prev) => (prev?.id === id ? { ...prev, ...data } : prev));
    },
    [],
  );

  const deleteAgent = useCallback(async (id: string) => {
    await apiDeleteAgent(id);
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
