import { ChevronDown } from 'lucide-react';
import React, { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

import type { AssignedCommit } from '../../../utils/gitGraph';

import { BRANCH_PANEL_W } from './constants';
import type { BranchEntry } from './types';
import { extractBranches } from './utils';

const BranchSection: React.FC<{
  titleKey: string;
  entries: BranchEntry[];
  selectedHash: string | null;
  onSelect: (hash: string) => void;
  dotBase: string;
}> = ({ titleKey, entries, selectedHash, onSelect, dotBase }) => {
  const { t } = useTranslation();
  const [open, setOpen] = useState(true);
  const chevronCls = `size-3 shrink-0 transition-transform ${open ? '' : '-rotate-90'}`;
  const entriesContent = open
    ? entries.map((e) => {
        const isActive = e.hash === selectedHash;
        const rowCls = `flex w-full min-w-0 items-center gap-1.5 px-3 py-1 text-xs ${isActive ? 'bg-muted/70 text-foreground' : 'text-foreground/70 hover:bg-muted/40 hover:text-foreground'}`;
        const dotCls = `size-1.5 shrink-0 rounded-full ${e.isCurrent ? 'bg-emerald-400' : dotBase}`;
        return (
          <button type="button" key={e.name} className={rowCls} onClick={() => onSelect(e.hash)}>
            <span className={dotCls} />
            <span className="min-w-0 truncate">{e.name}</span>
          </button>
        );
      })
    : null;
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen((p) => !p)}
        className="flex w-full items-center gap-1.5 px-3 py-1.5 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/50 hover:text-muted-foreground"
      >
        <ChevronDown className={chevronCls} />
        {t(titleKey)}
        <span className="ml-auto text-muted-foreground/40">{entries.length}</span>
      </button>
      {entriesContent}
    </>
  );
};

export const BranchPanel: React.FC<{
  commits: AssignedCommit[];
  selectedHash: string | null;
  onSelect: (hash: string) => void;
}> = ({ commits, selectedHash, onSelect }) => {
  const { local, remote, tags } = useMemo(() => extractBranches(commits), [commits]);
  return (
    <div
      style={{ width: BRANCH_PANEL_W }}
      className="flex shrink-0 flex-col overflow-y-auto border-r border-border/40 bg-muted/5 py-1"
    >
      <BranchSection
        titleKey="gitGraph.panelLocal"
        entries={local}
        selectedHash={selectedHash}
        onSelect={onSelect}
        dotBase="bg-violet-500"
      />
      <BranchSection
        titleKey="gitGraph.panelRemote"
        entries={remote}
        selectedHash={selectedHash}
        onSelect={onSelect}
        dotBase="bg-blue-500"
      />
      {tags.length > 0 && (
        <BranchSection
          titleKey="gitGraph.panelTags"
          entries={tags}
          selectedHash={selectedHash}
          onSelect={onSelect}
          dotBase="bg-amber-400"
        />
      )}
    </div>
  );
};
