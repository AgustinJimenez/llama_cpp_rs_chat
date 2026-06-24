import React, { useMemo } from 'react';

import type { AssignedCommit, GraphEdge } from '../../../utils/gitGraph';

import {
  COL_W,
  EDGE_OPACITY,
  EDGE_STROKE_W,
  H_LANE_H,
  NODE_FONT_SIZE,
  NODE_R,
  SELECTED_R_DELTA,
  SELECTED_STROKE_W,
  SVG_PAD_R,
} from './constants';
import { useAvatars } from './useAvatars';

function hEdgeSvgPath(e: GraphEdge): string {
  const x1 = e.fromRow * COL_W + COL_W / 2;
  const x2 = e.toRow * COL_W + COL_W / 2;
  const y1 = e.fromLane * H_LANE_H + H_LANE_H / 2;
  const y2 = e.toLane * H_LANE_H + H_LANE_H / 2;
  if (e.fromLane === e.toLane) return `M${x1},${y1}L${x2},${y1}`;
  const xm = (x1 + x2) / 2;
  return `M${x1},${y1}C${xm},${y1} ${xm},${y2} ${x2},${y2}`;
}

export const HGraphSvg: React.FC<{
  commits: AssignedCommit[];
  edges: GraphEdge[];
  maxLane: number;
  selectedHash: string | null;
  onSelect: (hash: string) => void;
  onContextMenu: (clientX: number, clientY: number, hash: string) => void;
}> = ({ commits, edges, maxLane, selectedHash, onSelect, onContextMenu }) => {
  const emails = useMemo(() => commits.map((c) => c.author_email), [commits]);
  const avatars = useAvatars(emails);

  const svgW = commits.length * COL_W + SVG_PAD_R;
  const svgH = (maxLane + 1) * H_LANE_H;
  return (
    <svg width={svgW} height={svgH} aria-hidden="true">
      <defs>
        {commits.map((c) => {
          const cx = c.row * COL_W + COL_W / 2;
          const cy = c.lane * H_LANE_H + H_LANE_H / 2;
          const r = c.hash === selectedHash ? NODE_R + SELECTED_R_DELTA : NODE_R;
          return (
            <clipPath key={c.hash} id={`havc-${c.hash}`}>
              <circle cx={cx} cy={cy} r={r} />
            </clipPath>
          );
        })}
      </defs>
      <g>
        {edges.map((e) => (
          <path
            key={e.id}
            d={hEdgeSvgPath(e)}
            stroke={e.color}
            strokeWidth={EDGE_STROKE_W}
            fill="none"
            opacity={EDGE_OPACITY}
          />
        ))}
      </g>
      <g>
        {commits.map((c) => {
          const cx = c.row * COL_W + COL_W / 2;
          const cy = c.lane * H_LANE_H + H_LANE_H / 2;
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
              clipPath={`url(#havc-${c.hash})`}
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
