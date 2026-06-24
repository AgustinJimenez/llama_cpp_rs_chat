import React, { useEffect, useRef } from 'react';
import toast from 'react-hot-toast';
import { useTranslation } from 'react-i18next';

import type { AssignedCommit } from '../../../utils/gitGraph';

import type { CtxMenuDef, CtxMenuState } from './types';
import { CTX_DEFS } from './types';

export const ContextMenu: React.FC<{
  state: CtxMenuState;
  onClose: () => void;
  onAction: (key: string, commit: AssignedCommit) => void;
}> = ({ state, onClose, onAction }) => {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [onClose]);

  const handleItem = (item: CtxMenuDef) => {
    if (item.copyFn) {
      void navigator.clipboard.writeText(item.copyFn(state.commit)).then(() => {
        toast.success(t('gitGraph.copiedNotice'));
      });
    } else {
      onAction(item.key, state.commit);
    }
    onClose();
  };

  return (
    <div
      ref={ref}
      style={{ position: 'fixed', top: state.y, left: state.x }}
      className="z-50 min-w-[170px] rounded-md border border-border bg-popover py-1 shadow-lg"
      role="menu"
    >
      {CTX_DEFS.map((item) => (
        <React.Fragment key={item.key}>
          {!!item.sep && <div className="my-1 border-t border-border/40" />}
          <button
            type="button"
            role="menuitem"
            className={`flex w-full items-center px-3 py-1.5 text-xs hover:bg-muted ${item.kind === 'danger' ? 'text-red-400' : 'text-foreground'}`}
            onClick={() => handleItem(item)}
          >
            {t(item.labelKey)}
          </button>
        </React.Fragment>
      ))}
    </div>
  );
};
