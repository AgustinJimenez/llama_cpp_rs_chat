import React from 'react';

import type { AssignedCommit, GraphEdge } from '../../../utils/gitGraph';

import { CommitRow } from './CommitRow';
import { HGraphSvg } from './HGraphSvg';

interface HorizontalViewProps {
  commits: AssignedCommit[];
  edges: GraphEdge[];
  maxLane: number;
  selectedHash: string | null;
  onSelectHash: (hash: string) => void;
  onContextMenu: (clientX: number, clientY: number, hash: string) => void;
  onDoubleClick: (hash: string) => void;
}

export const HorizontalView: React.FC<HorizontalViewProps> = ({
  commits, edges, maxLane, selectedHash, onSelectHash, onContextMenu, onDoubleClick,
}) => (
  <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
    <div className="min-w-0 shrink-0 overflow-x-auto overflow-y-hidden border-b border-border/40 bg-background/50 px-4">
      <HGraphSvg commits={commits} edges={edges} maxLane={maxLane} selectedHash={selectedHash} onSelect={onSelectHash} onContextMenu={onContextMenu} />
    </div>
    <div className="min-h-0 flex-1 overflow-auto px-4">
      {commits.map((c) => (
        <CommitRow key={c.hash} commit={c} isSelected={c.hash === selectedHash} onSelect={onSelectHash} onDoubleClick={onDoubleClick} onContextMenu={onContextMenu} />
      ))}
    </div>
  </div>
);
