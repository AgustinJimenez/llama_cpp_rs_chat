import { createContext, useContext, useState, useCallback, useMemo, type ReactNode } from 'react';
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
  const [viewMode, setViewModeRaw] = useState<ViewMode>(
    () => (localStorage.getItem('viewMode') as ViewMode) || 'markdown'
  );
  const [isRightSidebarOpen, setIsRightSidebarOpen] = useState(false);
  const [isConfigSidebarOpen, setIsConfigSidebarOpen] = useState(false);
  const [isAppSettingsOpen, setIsAppSettingsOpen] = useState(false);
  const [isModelConfigOpen, setIsModelConfigOpen] = useState(false);

  const setViewMode = useCallback((mode: ViewMode) => {
    setViewModeRaw(mode);
    localStorage.setItem('viewMode', mode);
  }, []);

  const toggleRightSidebar = useCallback(() => setIsRightSidebarOpen(p => !p), []);
  const closeRightSidebar = useCallback(() => setIsRightSidebarOpen(false), []);
  const toggleConfigSidebar = useCallback(() => setIsConfigSidebarOpen(p => !p), []);
  const closeConfigSidebar = useCallback(() => setIsConfigSidebarOpen(false), []);
  const openAppSettings = useCallback(() => setIsAppSettingsOpen(true), []);
  const closeAppSettings = useCallback(() => setIsAppSettingsOpen(false), []);
  const openModelConfig = useCallback(() => setIsModelConfigOpen(true), []);
  const closeModelConfig = useCallback(() => setIsModelConfigOpen(false), []);

  const value = useMemo<UIContextValue>(() => ({
    viewMode, setViewMode,
    isRightSidebarOpen, toggleRightSidebar, closeRightSidebar,
    isConfigSidebarOpen, toggleConfigSidebar, closeConfigSidebar,
    isAppSettingsOpen, openAppSettings, closeAppSettings,
    isModelConfigOpen, openModelConfig, closeModelConfig,
  }), [
    viewMode, setViewMode,
    isRightSidebarOpen, toggleRightSidebar, closeRightSidebar,
    isConfigSidebarOpen, toggleConfigSidebar, closeConfigSidebar,
    isAppSettingsOpen, openAppSettings, closeAppSettings,
    isModelConfigOpen, openModelConfig, closeModelConfig,
  ]);

  return (
    <UIContext.Provider value={value}>
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
