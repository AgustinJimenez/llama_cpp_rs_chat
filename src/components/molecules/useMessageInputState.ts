import { useMemo } from 'react';

import { useChatContext } from '../../contexts/ChatContext';
import { useModelContext } from '../../contexts/ModelContext';

const CHARS_PER_TOKEN = 4;

export function useInputState() {
  const {
    sendMessage: onSendMessage,
    isLoading,
    stopGeneration,
    messages,
    currentConversationId,
  } = useChatContext();
  const { status, isLoading: isModelLoading, loadingAction, activeProvider } = useModelContext();
  const hasVision = status.has_vision ?? false;
  const isModelBusy = isModelLoading && loadingAction !== null;
  const isRemoteProvider = activeProvider !== 'local';

  const isGeneratingElsewhere =
    status.generating === true &&
    status.active_conversation_id != null &&
    currentConversationId != null &&
    status.active_conversation_id !== currentConversationId;

  const disabled = isLoading || isModelBusy || isGeneratingElsewhere || (!status.loaded && !isRemoteProvider);
  const estimatedConvTokens = useMemo(
    () =>
      Math.round(messages.reduce((sum, m) => sum + (m.content?.length || 0), 0) / CHARS_PER_TOKEN),
    [messages],
  );
  const modelContextSize = status.context_size;
  const isModelLoaded = status.loaded || isRemoteProvider;
  return {
    onSendMessage,
    isLoading,
    stopGeneration,
    hasVision,
    isModelBusy,
    isModelLoaded,
    isGeneratingElsewhere,
    disabled,
    estimatedConvTokens,
    modelContextSize,
    loadingAction,
  };
}

export function getPlaceholder(
  isModelBusy: boolean,
  loadingAction: string | null,
  disabled: boolean,
  isGeneratingElsewhere: boolean,
  isModelLoaded: boolean,
  disabledReason?: string,
) {
  if (isModelBusy) return loadingAction === 'unloading' ? 'Unloading model...' : 'Loading model...';
  if (isGeneratingElsewhere) return 'Generation active on another conversation';
  if (!isModelLoaded) return 'Load a model to start chatting';
  if (disabled && disabledReason) return disabledReason;
  return 'Ask anything';
}
