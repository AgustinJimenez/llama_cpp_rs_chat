import React, { useState, useEffect } from 'react';
import { Menu, ChevronLeft, Plus, Settings, RotateCcw, X } from 'lucide-react';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '../atoms/dialog';
import { Button } from '../atoms/button';

interface ConversationFile {
  name: string;
  displayName: string;
  timestamp: string;
}

interface SidebarProps {
  isOpen: boolean;
  onToggle: () => void;
  onNewChat: () => void;
  onOpenSettings: () => void;
  onLoadConversation: (filename: string) => void;
  currentConversationId?: string | null;
}

// eslint-disable-next-line max-lines-per-function
const Sidebar: React.FC<SidebarProps> = ({ isOpen, onToggle, onNewChat, onOpenSettings, onLoadConversation, currentConversationId }) => {
  const [conversations, setConversations] = useState<ConversationFile[]>([]);
  const [loading, setLoading] = useState(false);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [conversationToDelete, setConversationToDelete] = useState<ConversationFile | null>(null);

  const fetchConversations = async () => {
    setLoading(true);

    // Add timeout to prevent infinite loading
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 5000); // 5 second timeout

    try {
      const response = await fetch('/api/conversations', {
        signal: controller.signal
      });
      clearTimeout(timeoutId);

      if (response.ok) {
        const data = await response.json();
        setConversations(data.conversations || []);
      } else {
        console.error('Failed to fetch conversations');
      }
    } catch (error) {
      clearTimeout(timeoutId);
      if ((error as Error).name === 'AbortError') {
        console.error('Fetch timeout: conversations request took too long');
      } else {
        console.error('Error fetching conversations:', error);
      }
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (isOpen) {
      fetchConversations();
    }
  }, [isOpen]);

  // Auto-refresh when currentConversationId changes (new conversation created)
  useEffect(() => {
    if (currentConversationId && isOpen) {
      fetchConversations();
    }
  }, [currentConversationId, isOpen]);

  const formatTimestamp = (timestamp: string) => {
    // Parse timestamp format: YYYY-MM-DD-HH-mm-ss-SSS
    const parts = timestamp.split('-');
    if (parts.length >= 6) {
      const [year, month, day, hour, minute] = parts;
      return `${month}/${day}/${year} ${hour}:${minute}`;
    }
    return timestamp;
  };

  const handleDeleteClick = (e: React.MouseEvent, conversation: ConversationFile) => {
    e.stopPropagation(); // Prevent conversation from being loaded
    setConversationToDelete(conversation);
    setDeleteDialogOpen(true);
  };

  const handleDeleteConfirm = async () => {
    if (!conversationToDelete) return;

    try {
      const response = await fetch(`/api/conversations/${conversationToDelete.name}`, {
        method: 'DELETE',
      });

      if (response.ok) {
        // Check if we're deleting the currently loaded conversation
        const deletingCurrentConversation = currentConversationId && conversationToDelete.name === currentConversationId;

        // Remove from list
        setConversations(prev => prev.filter(c => c.name !== conversationToDelete.name));
        setDeleteDialogOpen(false);
        setConversationToDelete(null);

        // If we deleted the current conversation, clear the chat
        if (deletingCurrentConversation) {
          onNewChat();
        }
      } else {
        console.error('Failed to delete conversation');
        alert('Failed to delete conversation');
      }
    } catch (error) {
      console.error('Error deleting conversation:', error);
      alert('Error deleting conversation');
    }
  };

  const handleDeleteCancel = () => {
    setDeleteDialogOpen(false);
    setConversationToDelete(null);
  };

  return (
    <>
      {/* Overlay for mobile */}
      {isOpen && (
        <div
          className="fixed inset-0 bg-black/50 z-[999] md:hidden"
          onClick={onToggle}
          data-testid="sidebar-overlay"
        />
      )}

      {/* Sidebar */}
      <div
        className={`
          fixed top-0 left-0 h-screen bg-card border-r border-border z-[1000] flex flex-col
          transition-all duration-300 ease-in-out
          ${isOpen ? 'w-[280px] translate-x-0' : 'w-[70px] translate-x-0 max-md:-translate-x-full max-md:w-[280px]'}
        `}
        data-testid="sidebar"
      >
        {/* Header */}
        <div className="flex items-center justify-center p-4 border-b border-border min-h-[60px]">
          <button
            className={`
              bg-primary border-none text-white cursor-pointer rounded-lg transition-all duration-200
              flex items-center justify-center font-medium
              hover:opacity-90 hover:-translate-y-px active:translate-y-0
              ${isOpen ? 'w-full h-10 text-xl px-2' : 'w-12 h-12 flex-shrink-0 text-2xl'}
            `}
            onClick={onToggle}
            data-testid="sidebar-toggle"
            aria-label={isOpen ? 'Close sidebar' : 'Open sidebar'}
          >
            {isOpen ? <ChevronLeft size={20} /> : <Menu size={24} />}
          </button>
          {isOpen && <h2 className="ml-3 text-lg font-semibold m-0">LLaMA Chat</h2>}
        </div>

        {/* Action Buttons */}
        {isOpen && (
          <div className="p-4 flex flex-col gap-2 border-b border-border">
            <button
              className="
                flex items-center gap-3 px-4 py-3 bg-primary text-white border-none rounded-lg
                text-sm font-medium cursor-pointer transition-all duration-200 w-full text-left
                hover:opacity-90 hover:-translate-y-px active:translate-y-0
              "
              onClick={onNewChat}
              data-testid="new-chat-btn"
            >
              <Plus size={16} className="flex-shrink-0" />
              New Chat
            </button>
            <button
              className="
                flex items-center gap-3 px-4 py-3 bg-[hsl(217_33%_30%)] text-foreground border-none rounded-lg
                text-sm font-medium cursor-pointer transition-all duration-200 w-full text-left
                hover:bg-muted hover:-translate-y-px active:translate-y-0
              "
              onClick={onOpenSettings}
              data-testid="settings-btn"
            >
              <Settings size={16} className="flex-shrink-0" />
              Settings
            </button>
          </div>
        )}

        {/* Conversations List */}
        {isOpen && (
          <div className="flex-1 overflow-hidden flex flex-col">
            <div className="flex items-center justify-between px-4 pt-4 pb-2">
              <h3 className="m-0 text-sm font-semibold uppercase tracking-wider text-muted-foreground">
                Recent Conversations
              </h3>
              <button
                className="
                  bg-transparent border-none cursor-pointer p-1 rounded-md transition-all duration-200
                  w-8 h-8 flex items-center justify-center text-muted-foreground
                  hover:bg-muted active:scale-95 disabled:opacity-50 disabled:cursor-not-allowed
                "
                onClick={fetchConversations}
                disabled={loading}
                data-testid="refresh-conversations"
                aria-label="Refresh conversations"
              >
                <RotateCcw size={16} className={`flex-shrink-0 ${loading ? 'animate-spin' : ''}`} />
              </button>
            </div>

            <div
              className="flex-1 overflow-y-auto px-4 pb-4 pt-2"
              data-testid="conversations-list"
            >
              {loading ? (
                <div className="text-center text-muted-foreground text-sm py-8 font-medium">
                  Loading...
                </div>
              ) : conversations.length === 0 ? (
                <div className="text-center text-muted-foreground text-sm py-8 font-medium">
                  No conversations yet
                </div>
              ) : (
                conversations.map((conversation, index) => {
                  const isActive = currentConversationId === conversation.name;
                  // Debug: log comparison on first render
                  if (index === 0 && currentConversationId) {
                    console.log('[SIDEBAR] Comparing:', { currentConversationId, conversationName: conversation.name, isActive });
                  }
                  return (
                    <div
                      key={conversation.name}
                      className={`
                        p-3 mb-2 rounded-lg cursor-pointer transition-all duration-200
                        flex items-start justify-between gap-2 relative
                        hover:bg-muted hover:-translate-y-px
                        ${isActive
                          ? 'bg-primary/30 border-2 border-primary'
                          : 'bg-transparent border border-transparent'}
                      `}
                      onClick={() => onLoadConversation(conversation.name)}
                      data-testid={`conversation-${index}`}
                    >
                      <div className="flex-1 min-w-0">
                        <div className="text-sm font-semibold mb-1 text-foreground">
                          Chat {formatTimestamp(conversation.timestamp)}
                        </div>
                        <div className="text-xs font-mono text-muted-foreground font-normal">
                          {conversation.name}
                        </div>
                      </div>
                      <button
                        className="
                          bg-destructive border-none text-white cursor-pointer p-1 rounded-md
                          transition-all duration-200 flex items-center justify-center flex-shrink-0 font-medium
                          hover:opacity-90 hover:scale-105 active:scale-95
                        "
                        onClick={(e) => handleDeleteClick(e, conversation)}
                        aria-label="Delete conversation"
                        data-testid={`delete-conversation-${index}`}
                      >
                        <X size={16} />
                      </button>
                    </div>
                  );
                })
              )}
            </div>
          </div>
        )}

        {/* Collapsed state indicator */}
        {!isOpen && (
          <div className="flex flex-col items-center gap-2 p-2">
            <button
              className="
                w-12 h-12 flex-shrink-0 bg-primary border-none rounded-lg text-white text-xl
                cursor-pointer flex items-center justify-center transition-all duration-200 font-medium
                hover:opacity-90 hover:-translate-y-px active:translate-y-0
              "
              onClick={onNewChat}
              data-testid="collapsed-new-chat"
              aria-label="New chat"
            >
              <Plus size={20} className="flex-shrink-0" />
            </button>
            <button
              className="
                w-12 h-12 flex-shrink-0 bg-primary border-none rounded-lg text-white text-xl
                cursor-pointer flex items-center justify-center transition-all duration-200 font-medium
                hover:opacity-90 hover:-translate-y-px active:translate-y-0
              "
              onClick={onOpenSettings}
              data-testid="collapsed-settings"
              aria-label="Settings"
            >
              <Settings size={20} className="flex-shrink-0" />
            </button>
          </div>
        )}
      </div>

      {/* Delete Confirmation Dialog */}
      <Dialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Conversation</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete this conversation? This action cannot be undone.
            </DialogDescription>
            {conversationToDelete && (
              <div className="mt-2 text-sm font-medium">
                {conversationToDelete.name}
              </div>
            )}
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={handleDeleteCancel}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={handleDeleteConfirm}>
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
};

export default Sidebar;
