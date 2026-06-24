import type { DiffLineType } from './types';

// ── Layout constants ──────────────────────────────────────────────────────────
export const ROW_H = 30;
export const LANE_W = 22;
export const NODE_R = 9;
export const NODE_FONT_SIZE = 8;
export const SVG_PAD_R = 10;
export const EDGE_STROKE_W = 1.5;
export const EDGE_OPACITY = 0.75;
export const SELECTED_R_DELTA = 2;
export const SELECTED_STROKE_W = 2;

export const HASH_TRUNC_LEN = 7;

// Horizontal layout constants
export const COL_W = 30;
export const H_LANE_H = 22;

// Vertical table column widths
export const REFS_COL_W = 140;
export const AUTHOR_COL_W = 100;
export const DATE_COL_W = 40;
export const BRANCH_PANEL_W = 190;
export const DETAIL_PANEL_W = 240;
export const RESIZE_PANEL_MIN_W = 200;
export const RESIZE_PANEL_MAX_W = 600;

// ── Time helpers ──────────────────────────────────────────────────────────────
export const MS_PER_S = 1000;
export const S_PER_MIN = 60;
export const MIN_PER_H = 60;
export const H_PER_DAY = 24;
export const DAY_PER_MO = 30;
export const MO_PER_YR = 12;

export const MSG_LEN_WARN = 50;
export const MSG_LEN_ERROR = 72;

// ── Diff constants ────────────────────────────────────────────────────────────
export const DIFF_LINE_CLS: Record<DiffLineType, string> = {
  add: 'bg-emerald-950/50 text-emerald-300',
  remove: 'bg-red-950/50 text-red-300',
  hunk: 'bg-muted/40 text-blue-400 font-semibold',
  meta: 'text-foreground/60',
  context: 'text-foreground/80',
};

// ── File status constants ─────────────────────────────────────────────────────
export const FILE_STATUS_CLS: Record<string, string> = {
  A: 'text-emerald-400',
  D: 'text-red-400',
  M: 'text-amber-400',
  R: 'text-blue-400',
};

export const STATUS_ORDER = ['M', 'A', 'D', 'R'];
