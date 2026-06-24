export interface GitStatusEntry { status: string; path: string; }
export interface GitStatusResult { staged: GitStatusEntry[]; unstaged: GitStatusEntry[]; error: string | null; }

export async function fetchGitStatus(repoPath: string): Promise<GitStatusResult> {
  const res = await fetch(`/api/git/status?path=${encodeURIComponent(repoPath)}`);
  return res.json() as Promise<GitStatusResult>;
}

export async function gitStage(repoPath: string, files: string[]): Promise<{ ok: boolean; error: string | null }> {
  const res = await fetch('/api/git/stage', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath, files }) });
  return res.json();
}

export async function gitUnstage(repoPath: string, files: string[]): Promise<{ ok: boolean; error: string | null }> {
  const res = await fetch('/api/git/unstage', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath, files }) });
  return res.json();
}

export async function gitCommit(repoPath: string, message: string, description: string): Promise<{ ok: boolean; output: string; error: string | null }> {
  const res = await fetch('/api/git/commit', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath, message, description }) });
  return res.json();
}

export async function gitFetch(repoPath: string): Promise<{ ok: boolean; output: string; error: string | null }> {
  const res = await fetch('/api/git/fetch', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath }) });
  return res.json();
}

export async function gitPull(repoPath: string): Promise<{ ok: boolean; output: string; error: string | null }> {
  const res = await fetch('/api/git/pull', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath }) });
  return res.json();
}

export async function gitPush(repoPath: string): Promise<{ ok: boolean; output: string; error: string | null }> {
  const res = await fetch('/api/git/push', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath }) });
  return res.json();
}

export async function gitCheckout(repoPath: string, hash: string): Promise<{ ok: boolean; output: string; error: string | null }> {
  const res = await fetch('/api/git/checkout', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath, hash }) });
  return res.json();
}

export async function gitStashPush(repoPath: string): Promise<{ ok: boolean; output: string; error: string | null }> {
  const res = await fetch('/api/git/stash-push', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath }) });
  return res.json();
}

export async function gitStashPop(repoPath: string): Promise<{ ok: boolean; output: string; error: string | null }> {
  const res = await fetch('/api/git/stash-pop', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath }) });
  return res.json();
}

export async function gitCreateBranch(repoPath: string, name: string, hash?: string): Promise<{ ok: boolean; output: string; error: string | null }> {
  const res = await fetch('/api/git/create-branch', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath, name, hash: hash ?? '' }) });
  return res.json();
}

export async function gitRevert(repoPath: string, hash: string): Promise<{ ok: boolean; output: string; error: string | null }> {
  const res = await fetch('/api/git/revert', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath, hash }) });
  return res.json();
}

export async function gitReset(repoPath: string, mode: 'soft' | 'mixed' | 'hard', hash: string): Promise<{ ok: boolean; output: string; error: string | null }> {
  const res = await fetch('/api/git/reset', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath, mode, hash }) });
  return res.json();
}

export async function gitCherryPick(repoPath: string, hash: string): Promise<{ ok: boolean; output: string; error: string | null }> {
  const res = await fetch('/api/git/cherry-pick', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath, hash }) });
  return res.json();
}

export async function gitAmend(repoPath: string, message: string, description: string): Promise<{ ok: boolean; output: string; error: string | null }> {
  const res = await fetch('/api/git/amend', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: repoPath, message, description }) });
  return res.json();
}

export interface RawCommit {
  hash: string;
  short_hash: string;
  parents: string[];
  subject: string;
  body: string;
  refs: string[];
  date: string;
  author: string;
  author_email: string;
}

export interface AssignedCommit extends RawCommit {
  row: number;
  lane: number;
  color: string;
}

export interface GraphEdge {
  id: string;
  fromRow: number;
  fromLane: number;
  toRow: number;
  toLane: number;
  color: string;
}

const LANE_COLORS = [
  '#58a6ff',
  '#3fb950',
  '#d29922',
  '#f85149',
  '#bc8cff',
  '#79c0ff',
  '#56d364',
  '#e3b341',
  '#ff7b72',
  '#db61a2',
];

export function processCommits(rawCommits: RawCommit[]): {
  commits: AssignedCommit[];
  edges: GraphEdge[];
  maxLane: number;
} {
  // lanes[i] = hash expected in that slot (null = free)
  const lanes: (string | null)[] = [];
  const laneColorMap: Record<number, string> = {};
  const commits: AssignedCommit[] = [];
  const posMap = new Map<string, { row: number; lane: number }>();

  for (let row = 0; row < rawCommits.length; row++) {
    const commit = rawCommits[row];

    // Find all lanes expecting this commit
    const mergingLanes: number[] = [];
    for (let i = 0; i < lanes.length; i++) {
      if (lanes[i] === commit.hash) mergingLanes.push(i);
    }

    // Primary lane: first merging, first free, or new
    let primaryLane: number;
    if (mergingLanes.length > 0) {
      primaryLane = mergingLanes[0];
    } else {
      const free = lanes.indexOf(null);
      primaryLane = free !== -1 ? free : lanes.length;
    }
    while (lanes.length <= primaryLane) lanes.push(null);

    if (!laneColorMap[primaryLane]) {
      laneColorMap[primaryLane] = LANE_COLORS[primaryLane % LANE_COLORS.length];
    }

    posMap.set(commit.hash, { row, lane: primaryLane });

    // Free all merging lanes
    for (const l of mergingLanes) lanes[l] = null;

    // Primary lane follows first parent
    lanes[primaryLane] = commit.parents[0] ?? null;

    // Extra parents (merge commits) get new/existing lanes
    for (let pi = 1; pi < commit.parents.length; pi++) {
      const parent = commit.parents[pi];
      if (lanes.indexOf(parent) === -1) {
        const free = lanes.indexOf(null);
        const slot = free !== -1 ? free : lanes.length;
        while (lanes.length <= slot) lanes.push(null);
        lanes[slot] = parent;
        if (!laneColorMap[slot]) {
          laneColorMap[slot] = LANE_COLORS[slot % LANE_COLORS.length];
        }
      }
    }

    commits.push({
      ...commit,
      row,
      lane: primaryLane,
      color: laneColorMap[primaryLane],
    });
  }

  // Build parent→child edges
  const edges: GraphEdge[] = [];
  for (const c of commits) {
    for (const parentHash of c.parents) {
      const pos = posMap.get(parentHash);
      if (pos) {
        edges.push({
          id: `${c.hash}-${parentHash}`,
          fromRow: c.row,
          fromLane: c.lane,
          toRow: pos.row,
          toLane: pos.lane,
          color: c.color,
        });
      }
    }
  }

  const maxLane = commits.reduce((m, c) => Math.max(m, c.lane), 0);
  return { commits, edges, maxLane };
}
