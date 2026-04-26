import { Plus, RotateCcw, Trash2, Settings, Search, Loader2 } from 'lucide-react';
import React, { useState, useEffect, useMemo, useCallback, Suspense } from 'react';

import { useChatContext } from '../../contexts/ChatContext';
import { useDownloadContext } from '../../contexts/DownloadContext';
import { useModelContext } from '../../contexts/ModelContext';
import { useConnection } from '../../hooks/useConnection';
import { useUIContext } from '../../hooks/useUIContext';
import { getConversations, deleteConversation } from '../../utils/tauriCommands';
import { Button } from '../atoms/button';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '../atoms/dialog';
import { BackgroundProcesses } from '../molecules/BackgroundProcesses';

const HubExplorer = React.lazy(() =>
  import('./HubExplorer').then((m) => ({ default: m.HubExplorer })),
);

const TIMESTAMP_PARTS_COUNT = 6;
const DAYS_PER_WEEK = 7;
const DAYS_PER_MONTH = 30;
const MONTHS_PER_YEAR = 12;
const DAYS_PER_YEAR = 365;
const POLL_INTERVAL_MS = 60000;

interface ConversationFile {
  name: string;
  display_name: string;
  timestamp: string;
  title?: string;
}

interface SidebarProps {
  onNewChat: () => void;
}

function relativeTime(timestamp: string): string {
  const parts = timestamp.split('-');
  if (parts.length < TIMESTAMP_PARTS_COUNT) return timestamp;
  const [year, month, day, hour, minute, second] = parts;
  const date = new Date(
    Date.UTC(
      Number(year),
      Number(month) - 1,
      Number(day),
      Number(hour),
      Number(minute),
      Number(second),
    ),
  );
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMin = Math.floor(diffMs / POLL_INTERVAL_MS);
  if (diffMin < 1) return 'now';
  if (diffMin < 60) return `${diffMin}m`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h`;
  const diffDay = Math.floor(diffHr / 24);
  if (diffDay < DAYS_PER_WEEK) return `${diffDay}d`;
  const diffWeek = Math.floor(diffDay / DAYS_PER_WEEK);
  if (diffWeek < 5) return `${diffWeek}w`;
  const diffMonth = Math.floor(diffDay / DAYS_PER_MONTH);
  if (diffMonth < MONTHS_PER_YEAR) return `${diffMonth}mo`;
  return `${Math.floor(diffDay / DAYS_PER_YEAR)}y`;
}

const DATE_GROUPS = ['Today', 'Yesterday', 'Previous 7 Days', 'Previous 30 Days', 'Older'] as const;

function getDateGroup(timestamp: string): string {
  const parts = timestamp.split('-');
  if (parts.length < 3) return 'Older';
  const date = new Date(
    Date.UTC(
      Number(parts[0]),
      Number(parts[1]) - 1,
      Number(parts[2]),
      parts.length >= 4 ? Number(parts[3]) : 0,
      parts.length >= 5 ? Number(parts[4]) : 0,
      parts.length >= TIMESTAMP_PARTS_COUNT ? Number(parts[5]) : 0,
    ),
  );
  const now = new Date();
  const today = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate()));
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  const weekAgo = new Date(today);
  weekAgo.setDate(weekAgo.getDate() - DAYS_PER_WEEK);
  const monthAgo = new Date(today);
  monthAgo.setDate(monthAgo.getDate() - DAYS_PER_MONTH);

  if (date >= today) return 'Today';
  if (date >= yesterday) return 'Yesterday';
  if (date >= weekAgo) return 'Previous 7 Days';
  if (date >= monthAgo) return 'Previous 30 Days';
  return 'Older';
}

const ConversationItem = React.memo(
  ({
    conversation,
    isActive,
    isGenerating,
    flatIndex,
    onLoad,
    onDelete,
  }: {
    conversation: ConversationFile;
    isActive: boolean;
    isGenerating: boolean;
    flatIndex: number;
    onLoad: (name: string) => void;
    onDelete: (e: React.MouseEvent, conversation: ConversationFile) => void;
  }) => (
    <div
      key={conversation.name}
      role="button"
      tabIndex={0}
      className={`group flex items-center justify-between px-3 py-2 rounded-lg cursor-pointer transition-colors text-sm ${
        isActive ? 'bg-card text-foreground font-semibold' : 'text-foreground hover:bg-muted/30'
      }`}
      onClick={() => onLoad(conversation.name)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') onLoad(conversation.name);
      }}
      data-testid={`conversation-${flatIndex}`}
    >
      <div className="flex items-center gap-1 truncate flex-1 min-w-0">
        {isGenerating ? (
          <Loader2 size={12} className="animate-spin text-cyan-400 flex-shrink-0" />
        ) : null}
        <span
          className="truncate"
          title={conversation.title || conversation.display_name || conversation.name}
        >
          {conversation.title || conversation.display_name || conversation.name}
        </span>
      </div>
      <div className="flex items-center gap-1 flex-shrink-0 ml-2">
        <span className="text-xs text-foreground/50">{relativeTime(conversation.timestamp)}</span>
        <button
          className={`opacity-0 group-hover:opacity-100 p-0.5 rounded transition-all ${isActive ? 'text-foreground/40 hover:text-destructive' : 'text-muted-foreground hover:text-destructive'}`}
          onClick={(e) => onDelete(e, conversation)}
          aria-label="Delete conversation"
          data-testid={`delete-conversation-${flatIndex}`}
        >
          <Trash2 size={12} />
        </button>
      </div>
    </div>
  ),
);
ConversationItem.displayName = 'ConversationItem';

// eslint-disable-next-line max-lines-per-function
const Sidebar: React.FC<SidebarProps> = ({ onNewChat }) => {
  const {
    loadConversation: onLoadConversation,
    currentConversationId,
    messages,
  } = useChatContext();
  const { status: modelStatus } = useModelContext();
  const activeGeneratingId = modelStatus.active_conversation_id;
  const { openAppSettings: onOpenAppSettings } = useUIContext();
  const { activeCount: downloadActiveCount } = useDownloadContext();
  const { connected } = useConnection();
  const [conversations, setConversations] = useState<ConversationFile[]>([]);
  const [loading, setLoading] = useState(false);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [conversationToDelete, setConversationToDelete] = useState<ConversationFile | null>(null);
  const [isExplorerOpen, setIsExplorerOpen] = useState(false);
  const [searchTerm, setSearchTerm] = useState('');

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

  // Fetch conversations on mount and when server reconnects
  useEffect(() => {
    if (connected) fetchConversations();
  }, [connected]);

  useEffect(() => {
    if (currentConversationId && connected) {
      fetchConversations();
    }
  }, [currentConversationId, connected]);

  // Listen for background title generation completing
  useEffect(() => {
    const handler = () => {
      fetchConversations();
    };
    window.addEventListener('conversation-title-updated', handler);
    return () => window.removeEventListener('conversation-title-updated', handler);
  }, []);

  // Filter conversations by search term, then group by date
  const filteredConversations = useMemo(() => {
    if (!searchTerm.trim()) return conversations;
    const term = searchTerm.toLowerCase();
    return conversations.filter((c) => {
      const title = (c.title || c.display_name || c.name).toLowerCase();
      return title.includes(term);
    });
  }, [conversations, searchTerm]);

  const groupedEntries = useMemo(() => {
    const groups: Record<string, ConversationFile[]> = {};
    for (const conv of filteredConversations) {
      const group = getDateGroup(conv.timestamp);
      if (!groups[group]) groups[group] = [];
      groups[group].push(conv);
    }
    const result: {
      group: string;
      items: { conversation: ConversationFile; flatIndex: number }[];
    }[] = [];
    let idx = 0;
    for (const group of DATE_GROUPS) {
      if (!groups[group] || groups[group].length === 0) continue;
      result.push({
        group,
        items: groups[group].map((c) => ({ conversation: c, flatIndex: idx++ })),
      });
    }
    return result;
  }, [filteredConversations]);

  const handleDeleteClick = useCallback((e: React.MouseEvent, conversation: ConversationFile) => {
    e.stopPropagation();
    setConversationToDelete(conversation);
    setDeleteDialogOpen(true);
  }, []);

  const handleDeleteConfirm = async () => {
    if (!conversationToDelete) return;
    try {
      await deleteConversation(conversationToDelete.name);
      const deletingCurrentConversation =
        currentConversationId && conversationToDelete.name === currentConversationId;
      setConversations((prev) => prev.filter((c) => c.name !== conversationToDelete.name));
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

  const { isMobileSidebarOpen, closeMobileSidebar } = useUIContext();

  const handleNewChat = useCallback(() => {
    onNewChat();
    closeMobileSidebar();
  }, [onNewChat, closeMobileSidebar]);

  const handleLoadConversation = useCallback(
    (name: string) => {
      onLoadConversation(name);
      closeMobileSidebar();
    },
    [onLoadConversation, closeMobileSidebar],
  );

  return (
    <>
      {/* Mobile backdrop */}
      {isMobileSidebarOpen ? (
        <div
          className="fixed inset-0 z-40 bg-black/50 md:hidden"
          role="button"
          tabIndex={0}
          onClick={closeMobileSidebar}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') closeMobileSidebar();
          }}
        />
      ) : null}

      {/* Sidebar — hidden on mobile by default, overlay when toggled */}
      <div
        className={`fixed top-0 left-0 h-screen w-[240px] bg-card border-r border-border z-50 flex flex-col transition-transform duration-200 md:translate-x-0 md:z-40 ${
          isMobileSidebarOpen ? 'translate-x-0' : '-translate-x-full'
        }`}
        data-testid="sidebar"
      >
        {/* Top actions */}
        <div className="px-3 pt-3 pb-2 space-y-0.5">
          <button
            className="flex items-center gap-2 w-full px-3 py-2 text-sm text-foreground/70 hover:text-foreground hover:bg-muted rounded-lg transition-colors"
            onClick={handleNewChat}
            data-testid="new-chat-btn"
          >
            <Plus size={16} />
            New conversation
          </button>
          <button
            className="flex items-center gap-2 w-full px-3 py-2 text-sm text-foreground/70 hover:text-foreground hover:bg-muted rounded-lg transition-colors"
            onClick={() => setIsExplorerOpen(true)}
          >
            <Search size={16} />
            Explore models
            {downloadActiveCount > 0 ? (
              <span className="ml-auto flex items-center gap-1 text-[10px] text-blue-400">
                <span className="h-1.5 w-1.5 rounded-full bg-blue-400 animate-pulse" />
                {downloadActiveCount}
              </span>
            ) : null}
          </button>
        </div>

        {/* Conversations header */}
        <div className="flex items-center justify-between px-4 pt-2 pb-1">
          <span className="text-xs font-medium text-foreground/50">Conversations</span>
          <button
            className="p-1 rounded text-foreground/50 hover:text-foreground hover:bg-muted transition-colors disabled:opacity-50"
            onClick={fetchConversations}
            disabled={loading}
            data-testid="refresh-conversations"
            aria-label="Refresh conversations"
          >
            <RotateCcw size={12} className={loading ? 'animate-spin' : ''} />
          </button>
        </div>

        {/* Search */}
        <div className="px-3 pb-2">
          <div className="relative">
            <Search
              size={12}
              className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground"
            />
            <input
              type="text"
              placeholder="Search conversations..."
              value={searchTerm}
              onChange={(e) => setSearchTerm(e.target.value)}
              className="w-full pl-7 pr-2 py-1.5 text-xs bg-muted border border-border rounded-md text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary"
            />
          </div>
        </div>

        {/* Conversation list — grows to fill space between header and footer */}
        <div className="flex-1 overflow-y-auto px-2 pb-2 min-h-0" data-testid="conversations-list">
          {(() => {
            if (loading) {
              return <div className="text-center text-foreground/50 text-xs py-6">Loading...</div>;
            }
            if (filteredConversations.length === 0) {
              return (
                <div className="text-center text-foreground/50 text-xs py-6">
                  {searchTerm ? `No results for "${searchTerm}"` : 'No conversations yet'}
                </div>
              );
            }
            return groupedEntries.map(({ group, items }) => (
              <div key={group}>
                <p className="px-3 pt-3 pb-1 text-xs font-medium text-foreground/50">{group}</p>
                {items.map(({ conversation, flatIndex }) => (
                  <ConversationItem
                    key={conversation.name}
                    conversation={conversation}
                    isActive={currentConversationId === conversation.name}
                    isGenerating={activeGeneratingId === conversation.name}
                    flatIndex={flatIndex}
                    onLoad={handleLoadConversation}
                    onDelete={handleDeleteClick}
                  />
                ))}
              </div>
            ));
          })()}
        </div>

        {/* Background processes indicator — only in sidebar when no conversation is active
            (when a conversation is open, the stats bar shows it instead) */}
        {messages.length === 0 ? <BackgroundProcesses /> : null}

        {/* Bottom settings bar */}
        <div className="px-3 pb-3 pt-2 border-t border-border">
          <button
            className="flex items-center gap-2 w-full px-3 py-2 text-sm text-foreground/70 hover:text-foreground hover:bg-muted rounded-lg transition-colors"
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
            <Button variant="outline" onClick={handleDeleteCancel}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={handleDeleteConfirm}>
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* HuggingFace Model Explorer */}
      {isExplorerOpen ? (
        <Suspense fallback={null}>
          <HubExplorer isOpen={isExplorerOpen} onClose={() => setIsExplorerOpen(false)} />
        </Suspense>
      ) : null}
    </>
  );
};

export default React.memo(Sidebar);
