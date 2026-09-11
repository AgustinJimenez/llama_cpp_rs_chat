import { useEffect, useState } from 'react';

import { listWorkers } from '@/utils/tauriCommands';

const POLL_INTERVAL_MS = 4000;

/** Same-file heuristic the backend uses to decide a worker already holds this model. */
function sameModel(a: string | null | undefined, b: string | null | undefined): boolean {
  if (!a || !b) return false;
  const base = (p: string) => p.replaceAll('\\', '/').split('/').pop()?.toLowerCase() ?? '';
  return base(a) === base(b);
}

export interface ResidentModel {
  workerId: string;
  name: string;
  vramGb: number;
  generating: boolean;
}

export interface ResidentModelsInfo {
  /** VRAM held by loaded models OTHER than `modelPath`, in GB. */
  otherModelsVramGb: number;
  /** Every resident model, for slot-occupancy display. */
  resident: ResidentModel[];
  /** Max concurrently loaded models, from app config (undefined until fetched). */
  slotCap?: number;
  slotsUsed?: number;
}

/**
 * Live VRAM held by *other* resident models, for the load modal's fit check.
 *
 * The memory estimate in the modal budgets against the card's total VRAM, which is why
 * AGENT_TASKS/003 went unnoticed: a config projected at 20.4 GB on a 22.5 GB card looks
 * fine, yet an already-resident 7 GB model made the real load overflow into RAM and
 * decode dropped to ~0 tok/s with no error anywhere.
 *
 * The model being configured is excluded on purpose. Reloading it releases its own VRAM
 * first, so counting it would make the warning fire on the most common path — and a
 * warning that cries wolf is one the user learns to ignore.
 */
export function useResidentModelsVram(
  isOpen: boolean,
  modelPath: string,
): ResidentModelsInfo {
  const [info, setInfo] = useState<ResidentModelsInfo>({
    otherModelsVramGb: 0,
    resident: [],
  });

  useEffect(() => {
    if (!isOpen) return;
    let cancelled = false;

    const poll = async () => {
      try {
        const res = await listWorkers();
        if (cancelled) return;
        const loaded = res.workers.filter((w) => w.loaded);
        const otherModelsVramGb = loaded
          .filter((w) => !sameModel(w.model_path, modelPath))
          .reduce((sum, w) => sum + (w.vram_gb ?? 0), 0);
        setInfo({
          otherModelsVramGb,
          resident: loaded.map((w) => ({
            workerId: w.id,
            name: w.general_name || w.model_path?.split(/[\\/]/).pop() || w.id,
            vramGb: w.vram_gb ?? 0,
            generating: w.generating,
          })),
          slotCap: res.slot_cap,
          slotsUsed: res.slots_used,
        });
      } catch {
        // Desktop (Tauri) mode has no multi-worker endpoint, and a transient fetch
        // failure shouldn't surface as a scary warning. Report "no contention" and
        // let the existing total-VRAM check stand on its own.
        if (!cancelled) setInfo({ otherModelsVramGb: 0, resident: [] });
      }
    };

    poll();
    const timer = setInterval(poll, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [isOpen, modelPath]);

  return info;
}
