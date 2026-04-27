import React from 'react';

interface SelectBarProps {
  selectedCount: number;
  onSelectAll: () => void;
  onDeleteSelected: () => void;
  onCancel: () => void;
}

export const SelectBar: React.FC<SelectBarProps> = ({
  selectedCount,
  onSelectAll,
  onDeleteSelected,
  onCancel,
}) => (
  <div className="px-3 py-2 border-t border-border bg-muted/50 flex items-center gap-2">
    <button
      className="text-xs px-2 py-1 rounded bg-muted hover:bg-accent transition-colors"
      onClick={onSelectAll}
    >
      Select all
    </button>
    <button
      className="text-xs px-2 py-1 rounded bg-destructive/10 text-destructive hover:bg-destructive/20 transition-colors disabled:opacity-50"
      disabled={selectedCount === 0}
      onClick={onDeleteSelected}
    >
      Delete ({selectedCount})
    </button>
    <button
      className="text-xs px-2 py-1 rounded bg-muted hover:bg-accent transition-colors ml-auto"
      onClick={onCancel}
    >
      Cancel
    </button>
  </div>
);
