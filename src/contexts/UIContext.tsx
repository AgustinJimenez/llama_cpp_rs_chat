import { createContext, useContext, useState, useCallback, type ReactNode } from 'react';
import type { ViewMode } from '../types';

interface UIContextValue {
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
}

const UIContext = createContext<UIContextValue | null>(null);

export function UIProvider({ children }: { children: ReactNode }) {
  const [viewMode, setViewMode] = useState<ViewMode>('markdown');
  const [isRightSidebarOpen, setIsRightSidebarOpen] = useState(false);
  const [isConfigSidebarOpen, setIsConfigSidebarOpen] = useState(false);
  const [isAppSettingsOpen, setIsAppSettingsOpen] = useState(false);
  const [isModelConfigOpen, setIsModelConfigOpen] = useState(false);

  const toggleRightSidebar = useCallback(() => setIsRightSidebarOpen(p => !p), []);
  const closeRightSidebar = useCallback(() => setIsRightSidebarOpen(false), []);
  const toggleConfigSidebar = useCallback(() => setIsConfigSidebarOpen(p => !p), []);
  const closeConfigSidebar = useCallback(() => setIsConfigSidebarOpen(false), []);
  const openAppSettings = useCallback(() => setIsAppSettingsOpen(true), []);
  const closeAppSettings = useCallback(() => setIsAppSettingsOpen(false), []);
  const openModelConfig = useCallback(() => setIsModelConfigOpen(true), []);
  const closeModelConfig = useCallback(() => setIsModelConfigOpen(false), []);

  return (
    <UIContext.Provider value={{
      viewMode,
      setViewMode,
      isRightSidebarOpen,
      toggleRightSidebar,
      closeRightSidebar,
      isConfigSidebarOpen,
      toggleConfigSidebar,
      closeConfigSidebar,
      isAppSettingsOpen,
      openAppSettings,
      closeAppSettings,
      isModelConfigOpen,
      openModelConfig,
      closeModelConfig,
    }}>
      {children}
    </UIContext.Provider>
  );
}

// eslint-disable-next-line react-refresh/only-export-components
export function useUIContext() {
  const ctx = useContext(UIContext);
  if (!ctx) throw new Error('useUIContext must be used within UIProvider');
  return ctx;
}
