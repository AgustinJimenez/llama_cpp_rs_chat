import { useEffect, useRef } from 'react';

/**
 * Get conversation ID from URL query parameter
 */
export function getConversationFromUrl(): string | null {
  const params = new URLSearchParams(window.location.search);
  return params.get('conversation');
}

/**
 * Update URL with conversation ID (without page reload)
 */
export function updateUrlWithConversation(conversationId: string | null) {
  const url = new URL(window.location.href);
  if (conversationId) {
    // Strip .txt for cleaner URLs
    const cleanId = conversationId.replace(/\.txt$/, '');
    url.searchParams.set('conversation', cleanId);
  } else {
    url.searchParams.delete('conversation');
  }
  window.history.replaceState({}, '', url.toString());
}

/**
 * Normalize conversation ID to ensure .txt extension
 */
export function normalizeConversationId(id: string): string {
  return id.endsWith('.txt') ? id : `${id}.txt`;
}

interface UseConversationUrlOptions {
  currentConversationId: string | null;
  loadConversation: (filename: string) => Promise<void>;
}

/**
 * Hook to sync conversation ID with URL parameters.
 * - Loads conversation from URL on initial mount
 * - Updates URL when conversation changes
 */
export function useConversationUrl({
  currentConversationId,
  loadConversation,
}: UseConversationUrlOptions) {
  const initialLoadDone = useRef(false);

  // Sync URL with current conversation ID
  useEffect(() => {
    // Skip URL update during initial load to prevent overwriting URL param
    if (!initialLoadDone.current) return;
    updateUrlWithConversation(currentConversationId);
  }, [currentConversationId]);

  // Load conversation from URL on initial mount
  useEffect(() => {
    if (initialLoadDone.current) return;
    initialLoadDone.current = true;

    const conversationFromUrl = getConversationFromUrl();
    if (conversationFromUrl) {
      console.log('[useConversationUrl] Loading conversation from URL:', conversationFromUrl);
      const normalizedId = normalizeConversationId(conversationFromUrl);
      loadConversation(normalizedId);
    }
  }, [loadConversation]);
}
