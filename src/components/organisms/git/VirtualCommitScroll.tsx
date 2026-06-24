import { useVirtualizer } from '@tanstack/react-virtual';
import React, { useEffect, useRef } from 'react';

import type { AssignedCommit, GraphEdge } from '../../../utils/gitGraph';

import { CommitRow } from './CommitRow';
import { REFS_COL_W, ROW_H } from './constants';
import { GraphSvg } from './GraphSvg';
import { RefBadge } from './RefBadge';

interface VirtualCommitScrollProps {
  displayRows: Array<{ commit: AssignedCommit; refIndex: number }>;
  adjCommits: AssignedCommit[];
  adjEdges: GraphEdge[];
  maxLane: number;
  selectedHash: string | null;
  scrollToHash?: string | null;
  onSelectHash: (hash: string) => void;
  onContextMenu: (clientX: number, clientY: number, hash: string) => void;
  onDoubleClick: (hash: string) => void;
}

export const VirtualCommitScroll: React.FC<VirtualCommitScrollProps> = ({
  displayRows,
  adjCommits,
  adjEdges,
  maxLane,
  selectedHash,
  scrollToHash,
  onSelectHash,
  onContextMenu,
  onDoubleClick,
}) => {
  const scrollRef = useRef<HTMLDivElement>(null);
  const rowVirtualizer = useVirtualizer({
    count: displayRows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 20,
  });

  useEffect(() => {
    if (!scrollToHash) return;
    const idx = displayRows.findIndex((r) => r.commit.hash === scrollToHash && r.refIndex === 0);
    if (idx >= 0) rowVirtualizer.scrollToIndex(idx, { align: 'center' });
  }, [scrollToHash]); // eslint-disable-line react-hooks/exhaustive-deps

  const totalSize = rowVirtualizer.getTotalSize();
  const virtualItems = rowVirtualizer.getVirtualItems();

  return (
    <div ref={scrollRef} className="flex min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-4">
      <div style={{ width: REFS_COL_W }} className="shrink-0">
        <div style={{ height: totalSize, position: 'relative' }}>
          {virtualItems.map((vi) => {
            const { commit: c, refIndex } = displayRows[vi.index];
            const ref = c.refs[refIndex];
            const refCellCls = `flex items-center gap-1 overflow-hidden ${c.hash === selectedHash ? 'bg-muted/70' : ''}`;
            return (
              <div
                key={`${c.hash}-${refIndex}`}
                style={{ position: 'absolute', top: vi.start, height: ROW_H, left: 0, right: 0 }}
                className={refCellCls}
              >
                {!!ref && <RefBadge refStr={ref} />}
              </div>
            );
          })}
        </div>
      </div>
      <GraphSvg
        commits={adjCommits}
        edges={adjEdges}
        maxLane={maxLane}
        rowCount={displayRows.length}
        selectedHash={selectedHash}
        onSelect={onSelectHash}
        onContextMenu={onContextMenu}
      />
      <div className="min-w-0 flex-1 border-l border-border/40">
        <div style={{ height: totalSize, position: 'relative' }}>
          {virtualItems.map((vi) => {
            const { commit: c, refIndex } = displayRows[vi.index];
            const isSelected = c.hash === selectedHash;
            const extraRowCls = `flex h-full cursor-pointer items-center gap-1.5 border-b border-border/20 px-2 ${isSelected ? 'bg-muted/70' : 'hover:bg-muted/40'}`;
            const commitCellEl =
              refIndex === 0 ? (
                <CommitRow
                  commit={c}
                  isSelected={isSelected}
                  onSelect={onSelectHash}
                  onDoubleClick={onDoubleClick}
                  onContextMenu={onContextMenu}
                />
              ) : (
                <div
                  className={extraRowCls}
                  role="button"
                  tabIndex={0}
                  onClick={() => onSelectHash(c.hash)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') onSelectHash(c.hash);
                  }}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    onContextMenu(e.clientX, e.clientY, c.hash);
                  }}
                >
                  <span className="w-[50px] shrink-0 select-none font-mono text-xs text-muted-foreground/25">
                    {c.short_hash}
                  </span>
                  <span className="min-w-0 flex-1 select-none truncate text-sm text-muted-foreground/25">
                    {c.subject}
                  </span>
                </div>
              );
            return (
              <div
                key={`${c.hash}-${refIndex}`}
                style={{ position: 'absolute', top: vi.start, height: ROW_H, left: 0, right: 0 }}
              >
                {commitCellEl}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
};
