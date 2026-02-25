import React, { useState, useEffect, useMemo } from 'react';
import { Plus, RotateCcw, Trash2, Settings, Search } from 'lucide-react';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '../atoms/dialog';
import { Button } from '../atoms/button';
import { HubExplorer } from './HubExplorer';
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
  onOpenAppSettings?: () => void;
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

const DATE_GROUPS = ['Today', 'Yesterday', 'Previous 7 Days', 'Previous 30 Days', 'Older'] as const;

function getDateGroup(timestamp: string): string {
  const parts = timestamp.split('-');
  if (parts.length < 3) return 'Older';
  const date = new Date(
    Number(parts[0]), Number(parts[1]) - 1, Number(parts[2]),
    parts.length >= 4 ? Number(parts[3]) : 0,
    parts.length >= 5 ? Number(parts[4]) : 0,
    parts.length >= 6 ? Number(parts[5]) : 0
  );
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  const weekAgo = new Date(today);
  weekAgo.setDate(weekAgo.getDate() - 7);
  const monthAgo = new Date(today);
  monthAgo.setDate(monthAgo.getDate() - 30);

  if (date >= today) return 'Today';
  if (date >= yesterday) return 'Yesterday';
  if (date >= weekAgo) return 'Previous 7 Days';
  if (date >= monthAgo) return 'Previous 30 Days';
  return 'Older';
}

// eslint-disable-next-line max-lines-per-function
const Sidebar: React.FC<SidebarProps> = ({ onNewChat, onLoadConversation, currentConversationId, onOpenAppSettings }) => {
  const [conversations, setConversations] = useState<ConversationFile[]>([]);
  const [loading, setLoading] = useState(false);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [conversationToDelete, setConversationToDelete] = useState<ConversationFile | null>(null);
  const [isExplorerOpen, setIsExplorerOpen] = useState(false);

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

  // Group conversations by date
  const groupedEntries = useMemo(() => {
    const groups: Record<string, ConversationFile[]> = {};
    for (const conv of conversations) {
      const group = getDateGroup(conv.timestamp);
      if (!groups[group]) groups[group] = [];
      groups[group].push(conv);
    }
    const result: { group: string; items: { conversation: ConversationFile; flatIndex: number }[] }[] = [];
    let idx = 0;
    for (const group of DATE_GROUPS) {
      if (!groups[group] || groups[group].length === 0) continue;
      result.push({ group, items: groups[group].map(c => ({ conversation: c, flatIndex: idx++ })) });
    }
    return result;
  }, [conversations]);

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
        {/* Top actions */}
        <div className="px-3 pt-3 pb-2 space-y-0.5">
          <button
            className="flex items-center gap-2 w-full px-3 py-2 text-sm text-muted-foreground hover:text-foreground hover:bg-muted rounded-lg transition-colors"
            onClick={onNewChat}
            data-testid="new-chat-btn"
          >
            <Plus size={16} />
            New conversation
          </button>
          <button
            className="flex items-center gap-2 w-full px-3 py-2 text-sm text-muted-foreground hover:text-foreground hover:bg-muted rounded-lg transition-colors"
            onClick={() => setIsExplorerOpen(true)}
          >
            <Search size={16} />
            Explore models
          </button>
        </div>

        {/* Conversations header */}
        <div className="flex items-center justify-between px-4 pt-2 pb-1">
          <span className="text-xs font-medium text-muted-foreground">Conversations</span>
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

        {/* Conversation list — grows to fill space between header and footer */}
        <div className="flex-1 overflow-y-auto px-2 pb-2 min-h-0" data-testid="conversations-list">
          {loading ? (
            <div className="text-center text-muted-foreground text-xs py-6">Loading...</div>
          ) : conversations.length === 0 ? (
            <div className="text-center text-muted-foreground text-xs py-6">No conversations yet</div>
          ) : (
            groupedEntries.map(({ group, items }) => (
              <div key={group}>
                <p className="px-3 pt-3 pb-1 text-xs font-medium text-muted-foreground">{group}</p>
                {items.map(({ conversation, flatIndex }) => {
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
                      data-testid={`conversation-${flatIndex}`}
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
                          data-testid={`delete-conversation-${flatIndex}`}
                        >
                          <Trash2 size={12} />
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
            ))
          )}
        </div>

        {/* Bottom settings bar */}
        <div className="px-3 pb-3 pt-2 border-t border-border">
          <button
            className="flex items-center gap-2 w-full px-3 py-2 text-sm text-muted-foreground hover:text-foreground hover:bg-muted rounded-lg transition-colors"
            onClick={onOpenAppSettings}
            aria-label="App Settings"
          >
            <Settings size={16} />
            Settings
          </button>
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

      {/* HuggingFace Model Explorer */}
      <HubExplorer
        isOpen={isExplorerOpen}
        onClose={() => setIsExplorerOpen(false)}
      />
    </>
  );
};

export default Sidebar;
