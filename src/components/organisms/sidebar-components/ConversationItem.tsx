import { MoreVertical, Trash2, CheckSquare, Square } from 'lucide-react';
import React, { useState } from 'react';

import type { ConversationFile } from './types';
import { relativeTime } from './utils';

function getItemStyle(isSelected: boolean, isActive: boolean): string {
  if (isSelected) return 'bg-primary/10 text-foreground';
  if (isActive) return 'bg-muted text-foreground font-semibold';
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
    const selectIcon = isSelected ? (
      <CheckSquare size={14} className="text-white" />
    ) : (
      <Square size={14} className="text-white/40" />
    );

    return (
      <li
        className={`group flex items-center justify-between rounded-lg text-sm transition-colors ${getItemStyle(
          isSelected,
          isActive,
        )}`}
        data-testid={`conversation-${flatIndex}`}
      >
        <button
          className="flex min-w-0 flex-1 items-center gap-1 truncate px-3 py-2 text-left"
          onClick={() => {
            if (selectMode) onToggleSelect(conversation.name);
            else onLoad(conversation.name);
          }}
        >
          {!!isGenerating && (
            <span className="relative flex size-2.5 flex-shrink-0">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-green-400 opacity-75" />
              <span className="relative inline-flex size-2.5 rounded-full bg-green-500" />
            </span>
          )}
          <span className="truncate" title={displayName}>
            {displayName}
          </span>
        </button>

        <div className="mr-2 flex flex-shrink-0 items-center gap-1">
          <span className="text-xs text-muted-foreground">
            {relativeTime(conversation.timestamp)}
          </span>
          {!!selectMode && (
            <button
              className="flex-shrink-0 p-0.5"
              onClick={(e) => {
                e.stopPropagation();
                onToggleSelect(conversation.name);
              }}
            >
              {selectIcon}
            </button>
          )}
          {!selectMode && (
            <div className="relative">
              <button
                className={`rounded p-0.5 opacity-0 transition-all group-hover:opacity-100 ${
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
              {!!menuOpen && (
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
              )}
            </div>
          )}
        </div>
      </li>
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
      aria-label="Close menu"
    />
    <div className="absolute right-0 top-6 z-50 min-w-[120px] rounded-md border border-border bg-popover py-1 shadow-lg">
      <button
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs hover:bg-muted"
        onClick={onDelete}
      >
        <Trash2 size={12} className="text-destructive" />
        Delete
      </button>
      <button
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs hover:bg-muted"
        onClick={onSelect}
      >
        <CheckSquare size={12} />
        Select
      </button>
    </div>
  </>
);
