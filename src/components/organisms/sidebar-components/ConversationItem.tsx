import { Loader2, MoreVertical, Trash2, CheckSquare, Square } from 'lucide-react';
import React, { useState } from 'react';

import type { ConversationFile } from './types';
import { relativeTime } from './utils';

function getItemStyle(isSelected: boolean, isActive: boolean): string {
  if (isSelected) return 'bg-primary/10 text-foreground';
  if (isActive) return 'bg-card text-foreground font-semibold';
  return 'text-foreground hover:bg-muted/30';
}

interface ConversationItemProps {
  conversation: ConversationFile;
  isActive: boolean;
  isGenerating: boolean;
  flatIndex: number;
  selectMode: boolean;
  isSelected: boolean;
  onLoad: (name: string) => void;
  onDelete: (e: React.MouseEvent, conversation: ConversationFile) => void;
  onToggleSelect: (name: string) => void;
  onEnterSelectMode: () => void;
}

export const ConversationItem = React.memo(
  ({
    conversation,
    isActive,
    isGenerating,
    flatIndex,
    selectMode,
    isSelected,
    onLoad,
    onDelete,
    onToggleSelect,
    onEnterSelectMode,
  }: ConversationItemProps) => {
    const [menuOpen, setMenuOpen] = useState(false);
    const displayName = conversation.title || conversation.display_name || conversation.name;

    return (
      <div
        role="button"
        tabIndex={0}
        className={`group flex items-center justify-between px-3 py-2 rounded-lg cursor-pointer transition-colors text-sm ${getItemStyle(
          isSelected,
          isActive,
        )}`}
        onClick={() => {
          if (selectMode) onToggleSelect(conversation.name);
          else onLoad(conversation.name);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            if (selectMode) onToggleSelect(conversation.name);
            else onLoad(conversation.name);
          }
        }}
        data-testid={`conversation-${flatIndex}`}
      >
        {selectMode ? (
          <button
            className="p-0.5 mr-1 flex-shrink-0"
            onClick={(e) => {
              e.stopPropagation();
              onToggleSelect(conversation.name);
            }}
          >
            {isSelected ? (
              <CheckSquare size={14} className="text-primary" />
            ) : (
              <Square size={14} className="text-muted-foreground" />
            )}
          </button>
        ) : null}

        <div className="flex items-center gap-1 truncate flex-1 min-w-0">
          {isGenerating ? (
            <Loader2 size={12} className="animate-spin text-cyan-400 flex-shrink-0" />
          ) : null}
          <span className="truncate" title={displayName}>
            {displayName}
          </span>
        </div>

        <div className="flex items-center gap-1 flex-shrink-0 ml-2">
          <span className="text-xs text-foreground/50">{relativeTime(conversation.timestamp)}</span>
          {!selectMode ? (
            <div className="relative">
              <button
                className={`opacity-0 group-hover:opacity-100 p-0.5 rounded transition-all ${
                  isActive
                    ? 'text-foreground/40 hover:text-foreground'
                    : 'text-muted-foreground hover:text-foreground'
                }`}
                onClick={(e) => {
                  e.stopPropagation();
                  setMenuOpen(!menuOpen);
                }}
                aria-label="Conversation options"
              >
                <MoreVertical size={12} />
              </button>
              {menuOpen ? (
                <ContextMenu
                  onClose={() => setMenuOpen(false)}
                  onDelete={(e) => {
                    setMenuOpen(false);
                    onDelete(e, conversation);
                  }}
                  onSelect={(e) => {
                    e.stopPropagation();
                    setMenuOpen(false);
                    onEnterSelectMode();
                    onToggleSelect(conversation.name);
                  }}
                />
              ) : null}
            </div>
          ) : null}
        </div>
      </div>
    );
  },
);
ConversationItem.displayName = 'ConversationItem';

const ContextMenu: React.FC<{
  onClose: () => void;
  onDelete: (e: React.MouseEvent) => void;
  onSelect: (e: React.MouseEvent) => void;
}> = ({ onClose, onDelete, onSelect }) => (
  <>
    <div
      className="fixed inset-0 z-50"
      onClick={onClose}
      onKeyDown={onClose}
      role="button"
      tabIndex={0}
    />
    <div className="absolute right-0 top-6 z-50 bg-popover border border-border rounded-md shadow-lg py-1 min-w-[120px]">
      <button
        className="w-full px-3 py-1.5 text-xs text-left hover:bg-muted flex items-center gap-2"
        onClick={onDelete}
      >
        <Trash2 size={12} className="text-destructive" />
        Delete
      </button>
      <button
        className="w-full px-3 py-1.5 text-xs text-left hover:bg-muted flex items-center gap-2"
        onClick={onSelect}
      >
        <CheckSquare size={12} />
        Select
      </button>
    </div>
  </>
);
