import { useState, useCallback } from 'react';
import { toast } from 'react-hot-toast';
import { TauriAPI } from '../utils/tauri';
import type { Message, ChatRequest } from '../types';

export function useChat() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [currentConversationId, setCurrentConversationId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const addMessage = useCallback((message: Message) => {
    setMessages(prev => [...prev, message]);
  }, []);

  const updateMessage = useCallback((messageId: string, content: string) => {
    setMessages(prev => prev.map(msg => 
      msg.id === messageId ? { ...msg, content } : msg
    ));
  }, []);

  const sendMessage = useCallback(async (content: string) => {
    if (isLoading || !content.trim()) return;

    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      content: content.trim(),
      timestamp: Date.now(),
    };

    addMessage(userMessage);
    setIsLoading(true);
    setError(null);

    // Create assistant message placeholder
    const assistantMessageId = crypto.randomUUID();
    const assistantMessage: Message = {
      id: assistantMessageId,
      role: 'assistant',
      content: '',
      timestamp: Date.now(),
    };
    addMessage(assistantMessage);

    try {
      const request: ChatRequest = {
        message: content.trim(),
        conversation_id: currentConversationId || undefined,
      };

      let streamingContent = '';

      await TauriAPI.sendMessageStream(
        request,
        // onToken - append each token to the message
        (token: string) => {
          streamingContent += token;
          updateMessage(assistantMessageId, streamingContent);
        },
        // onComplete - set conversation ID and stop loading
        (_messageId: string, conversationId: string) => {
          if (!currentConversationId) {
            setCurrentConversationId(conversationId);
          }
          setIsLoading(false);
        },
        // onError - handle errors
        (errorMessage: string) => {
          setError(errorMessage);
          toast.error(`Chat error: ${errorMessage}`, { duration: 5000 });
          updateMessage(assistantMessageId, `Error: ${errorMessage}`);
          setIsLoading(false);
        }
      );

    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'An unknown error occurred';
      setError(errorMessage);
      toast.error(`Chat error: ${errorMessage}`, { duration: 5000 });
      updateMessage(assistantMessageId, `Error: ${errorMessage}`);
      setIsLoading(false);
    }
  }, [isLoading, currentConversationId, addMessage, updateMessage]);

  const clearMessages = useCallback(() => {
    setMessages([]);
    setCurrentConversationId(null);
    setError(null);
  }, []);

  const loadConversation = useCallback(async (filename: string) => {
    setIsLoading(true);
    setError(null);
    
    try {
      const response = await fetch(`/api/conversation/${filename}`);
      if (!response.ok) {
        throw new Error('Failed to load conversation');
      }
      
      const data = await response.json();
      if (data.messages) {
        setMessages(data.messages);
        // Extract conversation ID from filename if needed
        setCurrentConversationId(filename);
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to load conversation';
      setError(errorMessage);
      toast.error(`Failed to load conversation: ${errorMessage}`, { duration: 5000 });
    } finally {
      setIsLoading(false);
    }
  }, []);

  return {
    messages,
    isLoading,
    error,
    sendMessage,
    clearMessages,
    loadConversation,
    currentConversationId,
  };
}