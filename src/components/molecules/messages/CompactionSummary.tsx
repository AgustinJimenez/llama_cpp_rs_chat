import { Pencil, Archive, ChevronDown, ChevronRight, Trash2 } from 'lucide-react';
import React, { useState, useRef, useEffect, useCallback } from 'react';

import { useChatContext } from '../../../contexts/ChatContext';
import { useModelContext } from '../../../contexts/ModelContext';
import type { Message } from '../../../types';
import { updateConversationSummary, deleteConversationSummary } from '../../../utils/tauriCommands';

const MAX_EDIT_ROWS = 20;

export const CompactionSummary: React.FC<{ message: Message; cleanContent: string }> = ({
  message,
  cleanContent,
}) => {
  const { currentConversationId } = useChatContext();
  const { status, activeProvider } = useModelContext();
  const modelReady = status.loaded || activeProvider !== 'local';
  const [expanded, setExpanded] = useState(false);
  const [isEditing, setIsEditing] = useState(false);
  const [editText, setEditText] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const expandChevron = expanded ? (
    <ChevronDown className="h-3 w-3 shrink-0" />
  ) : (
    <ChevronRight className="h-3 w-3 shrink-0" />
  );
  const saveLabel = isSaving ? 'Saving\u2026' : 'Save';

  // Extract the summary text (after the header line)
  const lines = cleanContent.split('\n');
  const summaryBody = lines.slice(1).join('\n').trim();

  const handleStartEdit = useCallback(() => {
    setEditText(summaryBody);
    setIsEditing(true);
    setExpanded(true);
  }, [summaryBody]);

  useEffect(() => {
    if (isEditing && textareaRef.current) {
      textareaRef.current.focus();
      textareaRef.current.setSelectionRange(editText.length, editText.length);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isEditing]);

  const handleSave = useCallback(async () => {
    if (!currentConversationId || !editText.trim()) return;
    setIsSaving(true);
    try {
      await updateConversationSummary(currentConversationId, editText.trim());
      window.dispatchEvent(new CustomEvent('conversation-compacted'));
      setIsEditing(false);
    } catch {
      /* ignore */
    } finally {
      setIsSaving(false);
    }
  }, [currentConversationId, editText]);

  const handleDelete = useCallback(async () => {
    if (!currentConversationId) return;
    try {
      await deleteConversationSummary(currentConversationId);
      window.dispatchEvent(new CustomEvent('conversation-compacted'));
    } catch {
      /* ignore */
    }
  }, [currentConversationId]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSave();
    } else if (e.key === 'Escape') {
      setIsEditing(false);
    }
  };

  return (
    <div
      className="my-2 flex w-full justify-center"
      data-testid="compaction-summary"
      data-message-id={message.id}
    >
      <div className="w-full max-w-[90%]">
        <div className="group flex w-full items-center gap-2 border-b border-t border-white/10 px-3 py-1.5 text-xs text-white/70 hover:border-white/20">
          <Archive className="h-3 w-3 shrink-0" />
          <button
            onClick={() => setExpanded(!expanded)}
            className="flex-1 truncate text-left transition-colors hover:text-white"
          >
            Earlier messages summarized
          </button>
          {!!modelReady && (
            <div className="flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100">
              <button
                onClick={handleStartEdit}
                className="rounded p-1 transition-colors hover:text-white/70"
                title="Edit summary"
              >
                <Pencil className="h-3 w-3" />
              </button>
              <button
                onClick={handleDelete}
                className="rounded p-1 transition-colors hover:text-red-400"
                title="Delete summary (reverts compaction)"
              >
                <Trash2 className="h-3 w-3" />
              </button>
            </div>
          )}
          <button
            onClick={() => setExpanded(!expanded)}
            className="transition-colors hover:text-white"
          >
            {expandChevron}
          </button>
        </div>
        {!!expanded && (
          <div className="border-b border-white/10 bg-white/5">
            {!!isEditing && (
              <div className="space-y-2 p-2">
                <textarea
                  ref={textareaRef}
                  value={editText}
                  onChange={(e) => setEditText(e.target.value)}
                  onKeyDown={handleKeyDown}
                  className="w-full resize-none rounded border border-white/20 bg-black/20 px-2 py-1.5 text-xs text-white/70 focus:outline-none focus:ring-1 focus:ring-primary"
                  rows={Math.min(editText.split('\n').length + 1, MAX_EDIT_ROWS)}
                />
                <div className="flex justify-end gap-2">
                  <button
                    onClick={() => setIsEditing(false)}
                    className="rounded px-2 py-1 text-xs text-white/50 transition-colors hover:bg-white/10 hover:text-white/70"
                  >
                    Cancel
                  </button>
                  <button
                    onClick={handleSave}
                    disabled={isSaving || !editText.trim() || !modelReady}
                    className="rounded bg-primary px-2 py-1 text-xs text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
                  >
                    {saveLabel}
                  </button>
                </div>
              </div>
            )}
            {!!summaryBody && !isEditing && (
              <div className="whitespace-pre-wrap px-3 py-2 text-xs text-white/80">
                {summaryBody}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
};
