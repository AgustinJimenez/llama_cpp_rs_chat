import { Layers } from 'lucide-react';
import React from 'react';
import { useTranslation } from 'react-i18next';

import { useResidentModelsVram } from '@/hooks/useResidentModelsVram';

/**
 * Which models are resident and how many slots remain.
 *
 * Activating an agent can evict another one (AGENT_TASKS/005). Without this the eviction
 * is invisible until the user notices their other agent went cold, so the point of
 * showing occupancy is to make "activating this will unload X" predictable beforehand.
 */
export const SlotOccupancyBar: React.FC<{ isOpen: boolean }> = ({ isOpen }) => {
  const { t } = useTranslation();
  const { resident, slotCap, slotsUsed } = useResidentModelsVram(isOpen, '');

  // Web-mode only: in desktop mode listWorkers throws and slotCap stays undefined.
  if (slotCap === undefined) return null;

  const used = slotsUsed ?? resident.length;
  const free = Math.max(0, slotCap - used);

  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border bg-muted/30 px-5 py-2 text-xs">
      <span className="flex items-center gap-1.5 font-medium text-foreground/80">
        <Layers className="size-3.5" />
        {t('agentSelector.slots', { used, cap: slotCap })}
      </span>
      {resident.map((m) => (
        <span
          key={m.workerId}
          className="flex items-center gap-1 rounded-full bg-background px-2 py-0.5 text-muted-foreground"
          title={t('agentSelector.slotHeld', { name: m.name, size: m.vramGb.toFixed(1) })}
        >
          <span
            className={`size-1.5 rounded-full ${m.generating ? 'animate-pulse bg-amber-400' : 'bg-emerald-400'}`}
          />
          {m.name}
          <span className="font-mono opacity-60">{m.vramGb.toFixed(1)}G</span>
        </span>
      ))}
      {free === 0 && (
        <span className="text-amber-600 dark:text-amber-400">
          {t('agentSelector.slotsFullHint')}
        </span>
      )}
    </div>
  );
};
