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
      className="w-full flex justify-center my-2"
      data-testid="compaction-summary"
      data-message-id={message.id}
    >
      <div className="max-w-[90%] w-full">
        <div className="group w-full flex items-center gap-2 px-3 py-1.5 text-xs text-white/70 border-t border-b border-white/10 hover:border-white/20">
          <Archive className="h-3 w-3 shrink-0" />
          <button
            onClick={() => setExpanded(!expanded)}
            className="flex-1 text-left hover:text-white transition-colors truncate"
          >
            Earlier messages summarized
          </button>
          {modelReady ? (
            <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
              <button
                onClick={handleStartEdit}
                className="p-1 rounded hover:text-white/70 transition-colors"
                title="Edit summary"
              >
                <Pencil className="h-3 w-3" />
              </button>
              <button
                onClick={handleDelete}
                className="p-1 rounded hover:text-red-400 transition-colors"
                title="Delete summary (reverts compaction)"
              >
                <Trash2 className="h-3 w-3" />
              </button>
            </div>
          ) : null}
          <button
            onClick={() => setExpanded(!expanded)}
            className="hover:text-white transition-colors"
          >
            {expanded ? (
              <ChevronDown className="h-3 w-3 shrink-0" />
            ) : (
              <ChevronRight className="h-3 w-3 shrink-0" />
            )}
          </button>
        </div>
        {expanded ? (
          <div className="bg-white/5 border-b border-white/10">
            {isEditing ? (
              <div className="p-2 space-y-2">
                <textarea
                  ref={textareaRef}
                  value={editText}
                  onChange={(e) => setEditText(e.target.value)}
                  onKeyDown={handleKeyDown}
                  className="w-full px-2 py-1.5 text-xs bg-black/20 border border-white/20 rounded text-white/70 resize-none focus:outline-none focus:ring-1 focus:ring-primary"
                  rows={Math.min(editText.split('\n').length + 1, MAX_EDIT_ROWS)}
                />
                <div className="flex justify-end gap-2">
                  <button
                    onClick={() => setIsEditing(false)}
                    className="px-2 py-1 text-xs rounded text-white/50 hover:text-white/70 hover:bg-white/10 transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    onClick={handleSave}
                    disabled={isSaving || !editText.trim() || !modelReady}
                    className="px-2 py-1 text-xs rounded bg-primary text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
                  >
                    {isSaving ? 'Saving…' : 'Save'}
                  </button>
                </div>
              </div>
            ) : null}
            {summaryBody && !isEditing ? (
              <div className="px-3 py-2 text-xs text-white/80 whitespace-pre-wrap">
                {summaryBody}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
};
