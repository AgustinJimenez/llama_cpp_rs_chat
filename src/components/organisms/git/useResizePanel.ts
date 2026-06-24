import { useCallback, useState } from 'react';

import { DETAIL_PANEL_W, RESIZE_PANEL_MAX_W, RESIZE_PANEL_MIN_W } from './constants';

export function useResizePanel(): {
  detailPanelWidth: number;
  onResizeStart: (clientX: number) => void;
} {
  const [detailPanelWidth, setDetailPanelWidth] = useState(() => {
    const saved = localStorage.getItem('gitGraphDetailPanelWidth');
    return saved ? parseInt(saved, 10) : DETAIL_PANEL_W;
  });

  const onResizeStart = useCallback(
    (startX: number) => {
      const startW = detailPanelWidth;
      const onMove = (ev: globalThis.MouseEvent) => {
        const newW = Math.max(RESIZE_PANEL_MIN_W, Math.min(RESIZE_PANEL_MAX_W, startW + (startX - ev.clientX)));
        setDetailPanelWidth(newW);
      };
      const onUp = () => {
        setDetailPanelWidth((w) => {
          localStorage.setItem('gitGraphDetailPanelWidth', String(w));
          return w;
        });
        document.removeEventListener('mousemove', onMove);
        document.removeEventListener('mouseup', onUp);
      };
      document.addEventListener('mousemove', onMove);
      document.addEventListener('mouseup', onUp);
    },
    [detailPanelWidth],
  );

  return { detailPanelWidth, onResizeStart };
}
