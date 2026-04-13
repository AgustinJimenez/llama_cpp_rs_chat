import { createContext } from 'react';

import type { ViewMode } from '../types';

export interface UIContextValue {
  viewMode: ViewMode;
  setViewMode: (mode: ViewMode) => void;
  isRightSidebarOpen: boolean;
  toggleRightSidebar: () => void;
  closeRightSidebar: () => void;
  isConfigSidebarOpen: boolean;
  toggleConfigSidebar: () => void;
  closeConfigSidebar: () => void;
  isAppSettingsOpen: boolean;
  openAppSettings: () => void;
  closeAppSettings: () => void;
  isModelConfigOpen: boolean;
  openModelConfig: () => void;
  closeModelConfig: () => void;
  isEventLogOpen: boolean;
  toggleEventLog: () => void;
  isProviderSelectorOpen: boolean;
  openProviderSelector: () => void;
  closeProviderSelector: () => void;
  isMobileSidebarOpen: boolean;
  toggleMobileSidebar: () => void;
  closeMobileSidebar: () => void;
}

export const UIContext = createContext<UIContextValue | null>(null);
