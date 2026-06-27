import { useEffect, useLayoutEffect, useRef, useState } from 'react';

const PACE_INTERVAL_MS = 24;
const SNAP_PATTERN = /[\s.,!?;:)\]]/;
const SNAP_LOOKAHEAD = 8;

// Named thresholds for the step-size ladder
const STEP_SM = 12;
const STEP_MD = 48;
const STEP_LG = 96;

function stepSize(remaining: number): number {
  if (remaining <= STEP_SM) return 2;
  if (remaining <= STEP_MD) return 4;
  if (remaining <= STEP_LG) return 8;
  return Math.min(24, Math.ceil(remaining / 8));
}

function nextBoundary(text: string, from: number): number {
  const end = Math.min(text.length, from + stepSize(text.length - from));
  const max = Math.min(text.length, end + SNAP_LOOKAHEAD);
  for (let i = end; i < max; i++) {
    if (SNAP_PATTERN.test(text[i] ?? '')) return i + 1;
  }
  return end;
}

/**
 * Paces a streaming text value so it advances in word-boundary chunks at ~24ms
 * intervals instead of on every token. Non-streaming content is returned immediately.
 */
export function usePacedContent(value: string, isStreaming: boolean): string {
  const [shown, setShown] = useState(value);
  // Refs track latest values synchronously — avoids stale closure bugs in timer callbacks.
  const shownRef = useRef(value);
  const valueRef = useRef(value);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Keep valueRef in sync without touching render flow.
  useLayoutEffect(() => {
    valueRef.current = value;
  }, [value]);

  const clear = () => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  };

  const snap = (text: string) => {
    clear();
    shownRef.current = text;
    setShown(text);
  };

  useEffect(() => {
    if (!isStreaming) {
      snap(value);
      return;
    }

    // If the value diverged (reset / different content), snap immediately.
    if (!value.startsWith(shownRef.current)) {
      snap(value);
      return;
    }

    // Already caught up or a timer is already running.
    if (value.length <= shownRef.current.length || timerRef.current !== null) return;

    const tick = () => {
      timerRef.current = null;
      const latest = valueRef.current;
      const next = nextBoundary(latest, shownRef.current.length);
      const slice = latest.slice(0, next);
      shownRef.current = slice;
      setShown(slice);
      if (next < latest.length) {
        timerRef.current = setTimeout(tick, PACE_INTERVAL_MS);
      }
    };

    timerRef.current = setTimeout(tick, PACE_INTERVAL_MS);
    return clear;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value, isStreaming]);

  return isStreaming ? shown : value;
}
