import React from 'react';

type RefKind = 'head' | 'local' | 'remote' | 'tag';

function parseRef(r: string): { label: string; kind: RefKind } {
  if (r.startsWith('HEAD -> ')) return { label: r.slice('HEAD -> '.length), kind: 'head' };
  if (r === 'HEAD') return { label: 'HEAD', kind: 'head' };
  if (r.startsWith('tag: ')) return { label: r.slice('tag: '.length), kind: 'tag' };
  if (r.includes('/')) return { label: r, kind: 'remote' };
  return { label: r, kind: 'local' };
}

const REF_CLS: Record<RefKind, string> = {
  head: 'bg-emerald-600 text-white',
  local: 'bg-violet-600 text-white',
  remote: 'bg-blue-600 text-white',
  tag: 'bg-amber-400 text-black',
};

export const RefBadge: React.FC<{ refStr: string }> = ({ refStr }) => {
  const { label, kind } = parseRef(refStr);
  return (
    <span
      className={`inline-flex max-w-[110px] shrink-0 items-center truncate rounded px-1 font-mono text-[10px] leading-[1.6] ${REF_CLS[kind]}`}
    >
      {label}
    </span>
  );
};
