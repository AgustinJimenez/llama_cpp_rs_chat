import React, { useMemo } from 'react';

import type { AssignedCommit, GraphEdge } from '../../../utils/gitGraph';

import {
  EDGE_OPACITY,
  EDGE_STROKE_W,
  LANE_W,
  NODE_FONT_SIZE,
  NODE_R,
  ROW_H,
  SELECTED_R_DELTA,
  SELECTED_STROKE_W,
  SVG_PAD_R,
} from './constants';
import { useAvatars } from './useAvatars';

function edgeSvgPath(e: GraphEdge): string {
  const x1 = e.fromLane * LANE_W + LANE_W / 2;
  const x2 = e.toLane * LANE_W + LANE_W / 2;
  const y1 = e.fromRow * ROW_H + ROW_H / 2;
  const y2 = e.toRow * ROW_H + ROW_H / 2;
  if (e.fromLane === e.toLane) return `M${x1},${y1}L${x1},${y2}`;
  const ym = (y1 + y2) / 2;
  return `M${x1},${y1}C${x1},${ym} ${x2},${ym} ${x2},${y2}`;
}

export const GraphSvg: React.FC<{
  commits: AssignedCommit[];
  edges: GraphEdge[];
  maxLane: number;
  rowCount: number;
  selectedHash: string | null;
  onSelect: (hash: string) => void;
  onContextMenu: (clientX: number, clientY: number, hash: string) => void;
}> = ({ commits, edges, maxLane, rowCount, selectedHash, onSelect, onContextMenu }) => {
  const emails = useMemo(() => commits.map((c) => c.author_email), [commits]);
  const avatars = useAvatars(emails);

  const svgW = (maxLane + 1) * LANE_W + SVG_PAD_R;
  const svgH = rowCount * ROW_H;
  return (
    <svg width={svgW} height={svgH} className="shrink-0" aria-hidden="true">
      <g>
        {commits.map((c) => {
          const rowCls = c.hash === selectedHash ? 'fill-muted/70' : 'fill-transparent';
          return (
            <rect
              key={`row-${c.hash}`}
              x={0}
              y={c.row * ROW_H}
              width={svgW}
              height={ROW_H}
              className={rowCls}
              style={{ cursor: 'pointer' }}
              onClick={() => onSelect(c.hash)}
              onContextMenu={(e) => {
                e.preventDefault();
                onContextMenu(e.clientX, e.clientY, c.hash);
              }}
            />
          );
        })}
      </g>
      <defs>
        {commits.map((c) => {
          const cx = c.lane * LANE_W + LANE_W / 2;
          const cy = c.row * ROW_H + ROW_H / 2;
          const r = c.hash === selectedHash ? NODE_R + SELECTED_R_DELTA : NODE_R;
          return (
            <clipPath key={c.hash} id={`avc-${c.hash}`}>
              <circle cx={cx} cy={cy} r={r} />
            </clipPath>
          );
        })}
      </defs>
      <g>
        {edges.map((e) => (
          <path
            key={e.id}
            d={edgeSvgPath(e)}
            stroke={e.color}
            strokeWidth={EDGE_STROKE_W}
            fill="none"
            opacity={EDGE_OPACITY}
          />
        ))}
      </g>
      <g>
        {commits.map((c) => {
          const cx = c.lane * LANE_W + LANE_W / 2;
          const cy = c.row * ROW_H + ROW_H / 2;
          const isSelected = c.hash === selectedHash;
          const r = isSelected ? NODE_R + SELECTED_R_DELTA : NODE_R;
          const stroke = isSelected ? 'white' : c.color;
          const strokeW = isSelected ? SELECTED_STROKE_W : 0;
          const avatarUrl = avatars.get(c.author_email);
          const nodeContent = avatarUrl ? (
            <image
              href={avatarUrl}
              x={cx - r}
              y={cy - r}
              width={r * 2}
              height={r * 2}
              clipPath={`url(#avc-${c.hash})`}
              style={{ pointerEvents: 'none' }}
            />
          ) : (
            <text
              x={cx}
              y={cy}
              textAnchor="middle"
              dominantBaseline="central"
              fontSize={NODE_FONT_SIZE}
              fontWeight="bold"
              fill="white"
              style={{ userSelect: 'none', pointerEvents: 'none' }}
            >
              {c.author.charAt(0).toUpperCase()}
            </text>
          );
          return (
            <g
              key={c.hash}
              style={{ cursor: 'pointer' }}
              onClick={() => onSelect(c.hash)}
              onContextMenu={(e) => {
                e.preventDefault();
                onContextMenu(e.clientX, e.clientY, c.hash);
              }}
            >
              <circle cx={cx} cy={cy} r={r} fill={c.color} stroke={stroke} strokeWidth={strokeW} />
              {nodeContent}
            </g>
          );
        })}
      </g>
    </svg>
  );
};
