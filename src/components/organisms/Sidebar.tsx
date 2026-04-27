import { Plus, RotateCcw, Settings, Search } from 'lucide-react';
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

import { ConversationItem, SelectBar, DATE_GROUPS, getDateGroup } from './sidebar-components';
import type { ConversationFile } from './sidebar-components';

const HubExplorer = React.lazy(() =>
  import('./HubExplorer').then((m) => ({ default: m.HubExplorer })),
);

interface SidebarProps {
  onNewChat: () => void;
}

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
  const [selectMode, setSelectMode] = useState(false);
  const [selectedConversations, setSelectedConversations] = useState<Set<string>>(new Set());

  const toggleSelect = useCallback((name: string) => {
    setSelectedConversations((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const selectAll = useCallback(() => {
    setSelectedConversations(new Set(conversations.map((c) => c.name)));
  }, [conversations]);

  const cancelSelectMode = useCallback(() => {
    setSelectMode(false);
    setSelectedConversations(new Set());
  }, []);

  const deleteSelected = useCallback(async () => {
    const toDelete = Array.from(selectedConversations);
    for (const name of toDelete) {
      try {
        await deleteConversation(name);
      } catch {
        /* ignore individual failures */
      }
    }
    setConversations((prev) => prev.filter((c) => !selectedConversations.has(c.name)));
    if (currentConversationId && selectedConversations.has(currentConversationId)) {
      onNewChat();
    }
    cancelSelectMode();
  }, [selectedConversations, currentConversationId, onNewChat, cancelSelectMode]);

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
    if (connected) fetchConversations();
  }, [connected]);

  useEffect(() => {
    if (currentConversationId && connected) fetchConversations();
  }, [currentConversationId, connected]);

  useEffect(() => {
    const handler = () => fetchConversations();
    window.addEventListener('conversation-title-updated', handler);
    return () => window.removeEventListener('conversation-title-updated', handler);
  }, []);

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
      const deletingCurrent =
        currentConversationId && conversationToDelete.name === currentConversationId;
      setConversations((prev) => prev.filter((c) => c.name !== conversationToDelete.name));
      setDeleteDialogOpen(false);
      setConversationToDelete(null);
      if (deletingCurrent) onNewChat();
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

      <div
        className={`fixed top-0 left-0 h-screen w-[240px] bg-card border-r border-border z-50 flex flex-col transition-transform duration-200 md:translate-x-0 md:z-40 ${
          isMobileSidebarOpen ? 'translate-x-0' : '-translate-x-full'
        }`}
        data-testid="sidebar"
      >
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
                    selectMode={selectMode}
                    isSelected={selectedConversations.has(conversation.name)}
                    onLoad={handleLoadConversation}
                    onDelete={handleDeleteClick}
                    onToggleSelect={toggleSelect}
                    onEnterSelectMode={() => setSelectMode(true)}
                  />
                ))}
              </div>
            ));
          })()}
        </div>

        {messages.length === 0 ? <BackgroundProcesses /> : null}

        {selectMode ? (
          <SelectBar
            selectedCount={selectedConversations.size}
            onSelectAll={selectAll}
            onDeleteSelected={deleteSelected}
            onCancel={cancelSelectMode}
          />
        ) : null}

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

      {isExplorerOpen ? (
        <Suspense fallback={null}>
          <HubExplorer isOpen={isExplorerOpen} onClose={() => setIsExplorerOpen(false)} />
        </Suspense>
      ) : null}
    </>
  );
};

export default React.memo(Sidebar);
