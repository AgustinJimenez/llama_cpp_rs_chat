export interface RawCommit {
  hash: string;
  short_hash: string;
  parents: string[];
  subject: string;
  refs: string[];
  date: string;
  author: string;
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
