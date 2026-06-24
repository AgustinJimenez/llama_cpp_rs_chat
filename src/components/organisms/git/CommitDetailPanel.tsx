import React from 'react';
import { useTranslation } from 'react-i18next';

import type { AssignedCommit } from '../../../utils/gitGraph';

import { DETAIL_PANEL_W, HASH_TRUNC_LEN } from './constants';
import { FileListView } from './FileListView';
import type { FileChange } from './types';

export const CommitDetailPanel: React.FC<{
  commit: AssignedCommit;
  files: FileChange[];
  filesLoading: boolean;
  width: number;
  onSelectParent: (hash: string) => void;
  onSelectFile: (file: string) => void;
}> = ({ commit, files, filesLoading, width, onSelectParent, onSelectFile }) => {
  const { t } = useTranslation();

  const initials = commit.author
    .split(' ')
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase() ?? '')
    .join('');
  const dateStr = commit.date.slice(0, 16).replace('T', ' ');
  const panelWidth = width === 0 ? DETAIL_PANEL_W : width;

  return (
    <div style={{ width: panelWidth }} className="flex shrink-0 flex-col overflow-hidden bg-muted/5">
      <div className="flex shrink-0 items-center gap-2 border-b border-border/40 px-3 py-2">
        <span className="text-xs text-muted-foreground/60">{t('gitGraph.commitLabel')}</span>
        <code className="min-w-0 truncate font-mono text-xs text-foreground/80">{commit.hash}</code>
      </div>
      <p className="shrink-0 px-3 py-2.5 text-sm font-medium leading-snug text-foreground">
        {commit.subject}
      </p>
      {commit.body.trim() && (
        <pre className="shrink-0 whitespace-pre-wrap break-words border-t border-border/40 px-3 py-2 font-sans text-sm leading-relaxed text-muted-foreground/80">
          {commit.body.trim()}
        </pre>
      )}
      <div className="flex shrink-0 items-center gap-2.5 border-t border-border/40 px-3 py-2">
        <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-violet-600 text-xs font-bold text-white">
          {initials}
        </div>
        <div className="min-w-0">
          <p className="truncate text-sm font-medium text-foreground">{commit.author}</p>
          <p className="text-sm text-muted-foreground/70">{dateStr}</p>
        </div>
      </div>
      {commit.parents.length > 0 && (
        <div className="shrink-0 border-t border-border/40 px-3 py-2">
          <p className="mb-1 text-xs font-semibold uppercase tracking-widest text-muted-foreground/50">
            {t('gitGraph.parent')}
          </p>
          {commit.parents.map((p) => (
            <button
              type="button"
              key={p}
              onClick={() => onSelectParent(p)}
              className="block font-mono text-sm text-blue-400 hover:underline"
            >
              {p.slice(0, HASH_TRUNC_LEN)}
            </button>
          ))}
        </div>
      )}
      <FileListView files={files} filesLoading={filesLoading} onSelectFile={onSelectFile} />
    </div>
  );
};
