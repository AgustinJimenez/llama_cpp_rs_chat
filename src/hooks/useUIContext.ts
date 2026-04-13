import { useContext } from 'react';

import { UIContext } from '../contexts/uiState';

export function useUIContext() {
  const ctx = useContext(UIContext);
  if (!ctx) throw new Error('useUIContext must be used within UIProvider');
  return ctx;
}
