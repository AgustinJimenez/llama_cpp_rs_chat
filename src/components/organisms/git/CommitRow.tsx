import React from 'react';

import type { AssignedCommit } from '../../../utils/gitGraph';

import { AUTHOR_COL_W, DATE_COL_W, ROW_H } from './constants';
import { relDate } from './utils';

export const CommitRow: React.FC<{
  commit: AssignedCommit;
  isSelected: boolean;
  onSelect: (hash: string) => void;
  onDoubleClick: (hash: string) => void;
  onContextMenu: (clientX: number, clientY: number, hash: string) => void;
}> = ({ commit, isSelected, onSelect, onDoubleClick, onContextMenu }) => {
  const rowCls = isSelected
    ? 'flex min-w-0 items-center gap-1.5 px-2 bg-muted/70 cursor-pointer'
    : 'flex min-w-0 items-center gap-1.5 px-2 hover:bg-muted/40 cursor-pointer';
  return (
    <div
      style={{ height: ROW_H, minHeight: ROW_H }}
      className={rowCls}
      role="button"
      tabIndex={0}
      onClick={() => onSelect(commit.hash)}
      onDoubleClick={() => onDoubleClick(commit.hash)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') onSelect(commit.hash);
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        onContextMenu(e.clientX, e.clientY, commit.hash);
      }}
    >
      <span className="w-[50px] shrink-0 font-mono text-xs text-foreground/80">
        {commit.short_hash}
      </span>
      <span className="min-w-0 flex-1 truncate text-sm text-foreground">{commit.subject}</span>
      {!!commit.body && (
        <span className="ml-2 shrink-0 truncate text-xs text-muted-foreground/60">
          {commit.body}
        </span>
      )}
      <span
        style={{ width: AUTHOR_COL_W }}
        className="shrink-0 truncate pl-2 text-xs text-foreground/70"
      >
        {commit.author}
      </span>
      <span
        style={{ width: DATE_COL_W }}
        className="shrink-0 pl-2 text-right text-xs text-foreground/80"
      >
        {relDate(commit.date)}
      </span>
    </div>
  );
};
