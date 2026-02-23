import React, { useState, useEffect } from 'react';
import { Plus, RotateCcw, Trash2 } from 'lucide-react';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '../atoms/dialog';
import { Button } from '../atoms/button';
import { getConversations, deleteConversation } from '../../utils/tauriCommands';

interface ConversationFile {
  name: string;
  displayName: string;
  timestamp: string;
}

interface SidebarProps {
  onNewChat: () => void;
  onLoadConversation: (filename: string) => void;
  currentConversationId?: string | null;
}

function relativeTime(timestamp: string): string {
  const parts = timestamp.split('-');
  if (parts.length < 6) return timestamp;
  const [year, month, day, hour, minute, second] = parts;
  const date = new Date(
    Number(year), Number(month) - 1, Number(day),
    Number(hour), Number(minute), Number(second)
  );
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 1) return 'now';
  if (diffMin < 60) return `${diffMin}m`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h`;
  const diffDay = Math.floor(diffHr / 24);
  if (diffDay < 7) return `${diffDay}d`;
  const diffWeek = Math.floor(diffDay / 7);
  if (diffWeek < 5) return `${diffWeek}w`;
  const diffMonth = Math.floor(diffDay / 30);
  if (diffMonth < 12) return `${diffMonth}mo`;
  return `${Math.floor(diffDay / 365)}y`;
}

// eslint-disable-next-line max-lines-per-function
const Sidebar: React.FC<SidebarProps> = ({ onNewChat, onLoadConversation, currentConversationId }) => {
  const [conversations, setConversations] = useState<ConversationFile[]>([]);
  const [loading, setLoading] = useState(false);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [conversationToDelete, setConversationToDelete] = useState<ConversationFile | null>(null);

  const fetchConversations = async () => {
    setLoading(true);
    try {
      const data = await getConversations();
      setConversations((data.conversations || []) as unknown as ConversationFile[]);
    } catch (error) {
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
    fetchConversations();
  }, []);

  useEffect(() => {
    if (currentConversationId) {
      fetchConversations();
    }
  }, [currentConversationId]);

  const handleDeleteClick = (e: React.MouseEvent, conversation: ConversationFile) => {
    e.stopPropagation();
    setConversationToDelete(conversation);
    setDeleteDialogOpen(true);
  };

  const handleDeleteConfirm = async () => {
    if (!conversationToDelete) return;
    try {
      await deleteConversation(conversationToDelete.name);
      const deletingCurrentConversation = currentConversationId && conversationToDelete.name === currentConversationId;
      setConversations(prev => prev.filter(c => c.name !== conversationToDelete.name));
      setDeleteDialogOpen(false);
      setConversationToDelete(null);
      if (deletingCurrentConversation) {
        onNewChat();
      }
    } catch (error) {
      console.error('Error deleting conversation:', error);
    }
  };

  const handleDeleteCancel = () => {
    setDeleteDialogOpen(false);
    setConversationToDelete(null);
  };

  return (
    <>
      {/* Sidebar — always visible */}
      <div
        className="fixed top-0 left-0 h-screen w-[240px] bg-card border-r border-border z-40 flex flex-col"
        data-testid="sidebar"
      >
        {/* New thread button */}
        <div className="px-3 pt-3 pb-2">
          <button
            className="flex items-center gap-2 w-full px-3 py-2 text-sm text-muted-foreground hover:text-foreground hover:bg-muted rounded-lg transition-colors"
            onClick={onNewChat}
            data-testid="new-chat-btn"
          >
            <Plus size={16} />
            New thread
          </button>
        </div>

        {/* Threads header */}
        <div className="flex items-center justify-between px-4 pt-2 pb-1">
          <span className="text-xs font-medium text-muted-foreground">Threads</span>
          <button
            className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted transition-colors disabled:opacity-50"
            onClick={fetchConversations}
            disabled={loading}
            data-testid="refresh-conversations"
            aria-label="Refresh conversations"
          >
            <RotateCcw size={12} className={loading ? 'animate-spin' : ''} />
          </button>
        </div>

        {/* Conversation list */}
        <div className="flex-1 overflow-y-auto px-2 pb-2" data-testid="conversations-list">
          {loading ? (
            <div className="text-center text-muted-foreground text-xs py-6">Loading...</div>
          ) : conversations.length === 0 ? (
            <div className="text-center text-muted-foreground text-xs py-6">No conversations yet</div>
          ) : (
            conversations.map((conversation, index) => {
              const isActive = currentConversationId === conversation.name;
              return (
                <div
                  key={conversation.name}
                  role="button"
                  tabIndex={0}
                  className={`group flex items-center justify-between px-3 py-2 rounded-lg cursor-pointer transition-colors text-sm ${
                    isActive
                      ? 'bg-muted text-foreground'
                      : 'text-muted-foreground hover:bg-muted/50 hover:text-foreground'
                  }`}
                  onClick={() => onLoadConversation(conversation.name)}
                  onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') onLoadConversation(conversation.name); }}
                  data-testid={`conversation-${index}`}
                >
                  <span className="truncate flex-1 min-w-0">
                    {conversation.displayName || conversation.name}
                  </span>
                  <div className="flex items-center gap-1 flex-shrink-0 ml-2">
                    <span className="text-xs text-muted-foreground">
                      {relativeTime(conversation.timestamp)}
                    </span>
                    <button
                      className="opacity-0 group-hover:opacity-100 p-0.5 rounded text-muted-foreground hover:text-destructive transition-all"
                      onClick={(e) => handleDeleteClick(e, conversation)}
                      aria-label="Delete conversation"
                      data-testid={`delete-conversation-${index}`}
                    >
                      <Trash2 size={12} />
                    </button>
                  </div>
                </div>
              );
            })
          )}
        </div>

      </div>

      {/* Delete Confirmation Dialog */}
      <Dialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Conversation</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete this conversation? This action cannot be undone.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={handleDeleteCancel}>Cancel</Button>
            <Button variant="destructive" onClick={handleDeleteConfirm}>Delete</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
};

export default Sidebar;
