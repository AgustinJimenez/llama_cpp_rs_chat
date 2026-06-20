import { useMemo, useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';

import { useAgentContext } from '../../contexts/AgentContext';
import { useChatContext } from '../../contexts/ChatContext';
import { useModelContext } from '../../contexts/ModelContext';
import type { Agent } from '../../types';

const CHARS_PER_TOKEN = 4;

function resolveActiveAgent(
  currentConversationId: string | null,
  conversationAgent: Agent | null,
  stagedAgent: Agent | null,
): Agent | null {
  return currentConversationId ? conversationAgent : (conversationAgent ?? stagedAgent);
}

function useAgentProviderInfo(
  activeAgent: Agent | null,
  activeProvider: string,
  agentStatuses: Record<string, { status: string }>,
) {
  const isRemoteProvider =
    activeProvider !== 'local' || (activeAgent !== null && activeAgent.provider_id !== 'local');
  const localAgentStatus =
    activeAgent !== null && activeAgent.provider_id === 'local'
      ? agentStatuses[activeAgent.id]?.status
      : undefined;
  const isLocalAgentReady = localAgentStatus === 'active' || localAgentStatus === 'generating';
  return { isRemoteProvider, isLocalAgentReady };
}

export function useInputState() {
  const { t } = useTranslation();
  const {
    sendMessage: onSendMessage,
    isLoading,
    stopGeneration,
    messages,
    currentConversationId,
    queuedMessage,
    cancelQueuedMessage,
  } = useChatContext();
  const { status, isLoading: isModelLoading, loadingAction, activeProvider } = useModelContext();
  const { stagedAgent, conversationAgent, agentStatuses } = useAgentContext();
  const hasVision = status.has_vision ?? false;
  const isModelBusy = isModelLoading && loadingAction !== null;
  const activeAgent = resolveActiveAgent(currentConversationId, conversationAgent, stagedAgent);
  const { isRemoteProvider, isLocalAgentReady } = useAgentProviderInfo(
    activeAgent,
    activeProvider,
    agentStatuses,
  );

  const [isCompacting, setIsCompacting] = useState(false);
  useEffect(() => {
    const onStart = () => setIsCompacting(true);
    const onDone = () => setIsCompacting(false);
    window.addEventListener('conversation-compacting', onStart);
    window.addEventListener('conversation-compacted', onDone);
    return () => {
      window.removeEventListener('conversation-compacting', onStart);
      window.removeEventListener('conversation-compacted', onDone);
    };
  }, []);

  const isGeneratingElsewhere =
    status.generating === true &&
    status.active_conversation_id != null &&
    currentConversationId != null &&
    status.active_conversation_id !== currentConversationId;

  // For local provider, an agent must be selected before chatting.
  const noAgentSelected = !isRemoteProvider && activeAgent === null;

  // Input is always enabled while generating — messages get queued (local) or backend-queued (remote)
  const disabled =
    isModelBusy ||
    isGeneratingElsewhere ||
    isCompacting ||
    noAgentSelected ||
    (!status.loaded && !isRemoteProvider && !isLocalAgentReady);
  const estimatedConvTokens = useMemo(() => {
    // Use promptTokens + genTokens from the last assistant message with timing data.
    // Skip this estimate if compacted messages exist — timing data is from before
    // compaction and doesn't reflect the actual (reduced) context window.
    const hasCompacted = messages.some((m) => m.compacted);
    if (!hasCompacted) {
      for (let i = messages.length - 1; i >= 0; i--) {
        const tm = messages[i].timings;
        if (tm?.promptTokens && tm?.genTokens) return tm.promptTokens + tm.genTokens;
      }
    }
    // Fall back to char-length estimate (no timing data yet, or post-compaction)
    return Math.round(
      messages.reduce((sum, m) => sum + (m.compacted ? 0 : m.content?.length || 0), 0) /
        CHARS_PER_TOKEN,
    );
  }, [messages]);
  const modelContextSize =
    status.context_size ??
    (activeAgent?.provider_id === 'local' ? activeAgent.context_size : undefined);
  const isModelLoaded = status.loaded || isRemoteProvider || isLocalAgentReady;
  return {
    t,
    onSendMessage,
    isLoading,
    stopGeneration,
    hasVision,
    isModelBusy,
    isModelLoaded,
    isGeneratingElsewhere,
    isCompacting,
    noAgentSelected,
    disabled,
    estimatedConvTokens,
    modelContextSize,
    loadingAction,
    queuedMessage,
    cancelQueuedMessage,
  };
}

export function getPlaceholder(
  t: (key: string) => string,
  isModelBusy: boolean,
  loadingAction: string | null,
  disabled: boolean,
  isGeneratingElsewhere: boolean,
  isModelLoaded: boolean,
  disabledReason?: string,
  isCompacting?: boolean,
  noAgentSelected?: boolean,
) {
  if (isCompacting) {
    return 'Compacting conversation…';
  }
  if (isModelBusy) {
    return loadingAction === 'unloading'
      ? t('chat.placeholderUnloading')
      : t('chat.placeholderLoading');
  }
  if (isGeneratingElsewhere) {
    return t('chat.placeholderGeneratingElsewhere');
  }
  if (noAgentSelected) {
    return 'Select an agent to start chatting…';
  }
  if (!isModelLoaded) {
    return t('chat.placeholderNoModel');
  }
  if (disabled && disabledReason) {
    return disabledReason;
  }
  return t('chat.placeholder');
}
