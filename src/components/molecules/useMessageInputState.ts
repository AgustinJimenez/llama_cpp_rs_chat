import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';

import { useChatContext } from '../../contexts/ChatContext';
import { useModelContext } from '../../contexts/ModelContext';

const CHARS_PER_TOKEN = 4;

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
  const hasVision = status.has_vision ?? false;
  const isModelBusy = isModelLoading && loadingAction !== null;
  const isRemoteProvider = activeProvider !== 'local';

  const isGeneratingElsewhere =
    status.generating === true &&
    status.active_conversation_id != null &&
    currentConversationId != null &&
    status.active_conversation_id !== currentConversationId;

  // Input is always enabled while generating — messages get queued (local) or backend-queued (remote)
  const disabled = isModelBusy || isGeneratingElsewhere || (!status.loaded && !isRemoteProvider);
  const estimatedConvTokens = useMemo(
    () =>
      Math.round(messages.reduce((sum, m) => sum + (m.content?.length || 0), 0) / CHARS_PER_TOKEN),
    [messages],
  );
  const modelContextSize = status.context_size;
  const isModelLoaded = status.loaded || isRemoteProvider;
  return {
    t,
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
) {
  if (isModelBusy) {
    return loadingAction === 'unloading'
      ? t('chat.placeholderUnloading')
      : t('chat.placeholderLoading');
  }
  if (isGeneratingElsewhere) {
    return t('chat.placeholderGeneratingElsewhere');
  }
  if (!isModelLoaded) {
    return t('chat.placeholderNoModel');
  }
  if (disabled && disabledReason) {
    return disabledReason;
  }
  return t('chat.placeholder');
}
