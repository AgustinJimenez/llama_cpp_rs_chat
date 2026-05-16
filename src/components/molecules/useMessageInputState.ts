import { useMemo, useState, useEffect } from 'react';
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

  // Input is always enabled while generating — messages get queued (local) or backend-queued (remote)
  const disabled =
    isModelBusy || isGeneratingElsewhere || isCompacting || (!status.loaded && !isRemoteProvider);
  const estimatedConvTokens = useMemo(() => {
    // Prefer actual prompt token count from the last assistant message's timings —
    // that's what the model really processed, no estimation needed.
    for (let i = messages.length - 1; i >= 0; i--) {
      const pt = messages[i].timings?.promptTokens;
      if (pt) return pt;
    }
    // Fall back to char-length estimate (no timing data yet)
    return Math.round(
      messages.reduce((sum, m) => sum + (m.compacted ? 0 : m.content?.length || 0), 0) /
        CHARS_PER_TOKEN,
    );
  }, [messages]);
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
    isCompacting,
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
  if (!isModelLoaded) {
    return t('chat.placeholderNoModel');
  }
  if (disabled && disabledReason) {
    return disabledReason;
  }
  return t('chat.placeholder');
}
