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
  <div className="flex items-center gap-2 border-t border-border bg-muted/50 px-3 py-2">
    <button
      className="rounded bg-muted px-2 py-1 text-xs transition-colors hover:bg-accent"
      onClick={onSelectAll}
    >
      Select all
    </button>
    <button
      className="rounded bg-destructive/10 px-2 py-1 text-xs text-destructive transition-colors hover:bg-destructive/20 disabled:opacity-50"
      disabled={selectedCount === 0}
      onClick={onDeleteSelected}
    >
      Delete ({selectedCount})
    </button>
    <button
      className="ml-auto rounded bg-muted px-2 py-1 text-xs transition-colors hover:bg-accent"
      onClick={onCancel}
    >
      Cancel
    </button>
  </div>
);
