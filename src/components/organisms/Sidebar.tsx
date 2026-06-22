import { Plus, RotateCcw, Settings, Search } from 'lucide-react';
import React, { useState, useEffect, useMemo, useCallback, useRef, Suspense } from 'react';
import { useTranslation } from 'react-i18next';

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

/* eslint-disable max-lines-per-function */
// react-doctor-disable-next-line react-doctor/no-giant-component, react-doctor/prefer-useReducer -- genuinely distinct UI states
const Sidebar: React.FC<SidebarProps> = ({ onNewChat }) => {
  const { t } = useTranslation();
  const {
    loadConversation: onLoadConversation,
    currentConversationId,
    messages,
    streamStatus,
  } = useChatContext();
  const { status: modelStatus } = useModelContext();
  const isModelGenerating = modelStatus.generating === true;
  // Show generating indicator for both local models and remote providers.
  // Use streamStatus (only set during actual generation, not conversation loading).
  // eslint-disable-next-line no-nested-ternary
  const activeGeneratingId = isModelGenerating
    ? modelStatus.active_conversation_id
    : streamStatus
      ? currentConversationId
      : undefined;
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

  const deleteSelected = useCallback(() => {
    if (selectedConversations.size === 0) return;
    // Show confirmation dialog — reuse the same dialog with a fake conversation
    setConversationToDelete({
      name: '__bulk_delete__',
      display_name: `${selectedConversations.size} conversations`,
      timestamp: '',
    } as ConversationFile);
    setDeleteDialogOpen(true);
  }, [selectedConversations]);

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

  // react-doctor-disable-next-line react-doctor/no-effect-event-handler
  useEffect(() => {
    if (connected) fetchConversations();
  }, [connected]);

  // react-doctor-disable-next-line react-doctor/no-effect-event-handler
  useEffect(() => {
    if (currentConversationId && connected) fetchConversations();
  }, [currentConversationId, connected]);

  useEffect(() => {
    const handler = () => fetchConversations();
    window.addEventListener('conversation-title-updated', handler);
    return () => window.removeEventListener('conversation-title-updated', handler);
  }, []);

  useEffect(() => {
    const handler = () => fetchConversations();
    window.addEventListener('conversation-started', handler);
    return () => window.removeEventListener('conversation-started', handler);
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
      if (conversationToDelete.name === '__bulk_delete__') {
        // Bulk delete all selected conversations in parallel
        const toDelete = [...selectedConversations];
        await Promise.allSettled(toDelete.map((name) => deleteConversation(name)));
        setConversations((prev) => prev.filter((c) => !selectedConversations.has(c.name)));
        if (currentConversationId && selectedConversations.has(currentConversationId)) {
          onNewChat();
        }
        cancelSelectMode();
      } else {
        // Single delete
        await deleteConversation(conversationToDelete.name);
        const deletingCurrent =
          currentConversationId && conversationToDelete.name === currentConversationId;
        setConversations((prev) => prev.filter((c) => c.name !== conversationToDelete.name));
        if (deletingCurrent) onNewChat();
      }
      setDeleteDialogOpen(false);
      setConversationToDelete(null);
    } catch (error) {
      console.error('Error deleting conversation:', error);
    }
  };

  const handleDeleteCancel = () => {
    setDeleteDialogOpen(false);
    setConversationToDelete(null);
  };

  const { isMobileSidebarOpen, closeMobileSidebar, sidebarWidth, setSidebarWidth } = useUIContext();
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);

  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (!dragRef.current) return;
      const newWidth = dragRef.current.startWidth + (e.clientX - dragRef.current.startX);
      setSidebarWidth(newWidth);
    };
    const onMouseUp = () => {
      dragRef.current = null;
      document.body.style.cssText += 'cursor:default;user-select:auto;';
    };
    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
    return () => {
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
    };
  }, [setSidebarWidth]);

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

  const rotateCcwClass = loading ? 'animate-spin' : '';
  const isBulkDelete = conversationToDelete?.name === '__bulk_delete__';
  const deleteDialogTitle = isBulkDelete
    ? t('sidebar.deleteConversations')
    : t('sidebar.deleteConversation');
  const deleteDialogDescription = isBulkDelete
    ? t('sidebar.deleteBulkConfirm', { count: selectedConversations.size })
    : t('sidebar.deleteConfirm');

  const groupLabelMap: Record<string, string> = {
    Today: t('sidebar.today'),
    Yesterday: t('sidebar.yesterday'),
    'Previous 7 Days': t('sidebar.previous7Days'),
    'Previous 30 Days': t('sidebar.previous30Days'),
    Older: t('sidebar.older'),
  };

  return (
    <>
      {!!isMobileSidebarOpen && (
        <div
          className="fixed inset-0 z-40 bg-black/50 md:hidden"
          role="button"
          tabIndex={0}
          onClick={closeMobileSidebar}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') closeMobileSidebar();
          }}
        />
      )}

      <nav
        aria-label="Conversations"
        className={`fixed left-0 top-0 z-50 flex h-screen flex-col border-r border-border bg-card transition-transform duration-200 md:z-40 md:translate-x-0 ${
          isMobileSidebarOpen ? 'translate-x-0' : '-translate-x-full'
        }`}
        style={{ width: `${sidebarWidth}px` }}
        data-testid="sidebar"
      >
        <div className="space-y-0.5 px-3 pb-2 pt-3">
          <button
            className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-sm text-foreground/70 transition-colors hover:bg-muted hover:text-foreground"
            onClick={handleNewChat}
            data-testid="new-chat-btn"
          >
            <Plus size={16} />
            {t('sidebar.newConversation')}
          </button>
          <button
            className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-sm text-foreground/70 transition-colors hover:bg-muted hover:text-foreground"
            onClick={() => setIsExplorerOpen(true)}
          >
            <Search size={16} />
            {t('sidebar.exploreModels')}
            {downloadActiveCount > 0 && (
              <span className="ml-auto flex items-center gap-1 text-[10px] text-blue-400">
                <span className="size-1.5 animate-pulse rounded-full bg-blue-400" />
                {downloadActiveCount}
              </span>
            )}
          </button>
        </div>

        <div className="flex items-center justify-between px-4 pb-1 pt-2">
          <span className="text-xs font-medium text-muted-foreground">
            {t('sidebar.conversations')}
          </span>
          <button
            className="rounded p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-50"
            onClick={fetchConversations}
            disabled={loading}
            data-testid="refresh-conversations"
            aria-label={t('sidebar.refreshLabel')}
          >
            <RotateCcw size={12} className={rotateCcwClass} />
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
              placeholder={t('sidebar.searchPlaceholder')}
              value={searchTerm}
              onChange={(e) => setSearchTerm(e.target.value)}
              className="w-full rounded-md border border-border bg-muted py-1.5 pl-7 pr-2 text-xs text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary"
            />
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2" data-testid="conversations-list">
          {(() => {
            if (filteredConversations.length === 0) {
              // Only show empty state when not loading — avoids replacing the list with a label
              if (loading) return null;
              const emptyLabel = searchTerm
                ? t('sidebar.noResults', { searchTerm })
                : t('sidebar.noConversations');
              return (
                <div className="py-6 text-center text-xs text-muted-foreground">{emptyLabel}</div>
              );
            }
            return groupedEntries.map(({ group, items }) => (
              <div key={group}>
                <p className="px-3 pb-1 pt-3 text-xs font-medium text-muted-foreground">
                  {groupLabelMap[group] || group}
                </p>
                <ul className="m-0 list-none p-0">
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
                </ul>
              </div>
            ));
          })()}
        </div>

        {messages.length === 0 && <BackgroundProcesses />}

        {!!selectMode && (
          <SelectBar
            selectedCount={selectedConversations.size}
            onSelectAll={selectAll}
            onDeleteSelected={deleteSelected}
            onCancel={cancelSelectMode}
          />
        )}

        <div className="border-t border-border px-3 pb-3 pt-2">
          <button
            className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-sm text-foreground/70 transition-colors hover:bg-muted hover:text-foreground"
            onClick={onOpenAppSettings}
            aria-label={t('sidebar.appSettingsLabel')}
          >
            <Settings size={16} />
            {t('sidebar.settings')}
          </button>
        </div>

        {/* Resize handle */}
        {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
        <div
          className="absolute right-0 top-0 hidden h-full w-1 cursor-col-resize hover:bg-primary/30 active:bg-primary/50 md:block"
          onMouseDown={(e) => {
            e.preventDefault();
            dragRef.current = { startX: e.clientX, startWidth: sidebarWidth };
            document.body.style.cssText += 'cursor:col-resize;user-select:none;';
          }}
        />
      </nav>

      <Dialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{deleteDialogTitle}</DialogTitle>
            <DialogDescription>{deleteDialogDescription}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={handleDeleteCancel}>
              {t('common.cancel')}
            </Button>
            <Button variant="destructive" onClick={handleDeleteConfirm}>
              {t('common.delete')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {!!isExplorerOpen && (
        <Suspense fallback={null}>
          <HubExplorer isOpen={isExplorerOpen} onClose={() => setIsExplorerOpen(false)} />
        </Suspense>
      )}
    </>
  );
};

export const SidebarComponent = React.memo(Sidebar);
