import { useState, useCallback, useMemo, type ReactNode } from 'react';

import type { ViewMode } from '../types';

import { UIContext } from './uiState';
import type { UIContextValue } from './uiState';

export const UIProvider = ({ children }: { children: ReactNode }) => {
  const [viewModeRaw, setViewModeRaw] = useState<ViewMode>(
    () => (localStorage.getItem('viewMode') as ViewMode) || 'markdown',
  );
  const [isRightSidebarOpen, setIsRightSidebarOpen] = useState(false);
  const [isAppSettingsOpen, setIsAppSettingsOpen] = useState(false);
  const [isModelConfigOpen, setIsModelConfigOpen] = useState(false);
  const [isEventLogOpen, setIsEventLogOpen] = useState(false);
  const [isProviderSelectorOpen, setIsProviderSelectorOpen] = useState(false);
  const [isMobileSidebarOpen, setIsMobileSidebarOpen] = useState(false);
  const [browserViewUrl, setBrowserViewUrl] = useState<string | null>(null);
  const [browserViewTabId, setBrowserViewTabId] = useState<string | null>(null);
  const [isBrowserViewOpen, setIsBrowserViewOpen] = useState(false);

  const setViewMode = useCallback((mode: ViewMode) => {
    setViewModeRaw(mode);
    localStorage.setItem('viewMode', mode);
  }, []);

  const toggleRightSidebar = useCallback(() => setIsRightSidebarOpen((p) => !p), []);
  const closeRightSidebar = useCallback(() => setIsRightSidebarOpen(false), []);
  const openAppSettings = useCallback(() => setIsAppSettingsOpen(true), []);
  const closeAppSettings = useCallback(() => setIsAppSettingsOpen(false), []);
  const openModelConfig = useCallback(() => setIsModelConfigOpen(true), []);
  const closeModelConfig = useCallback(() => setIsModelConfigOpen(false), []);
  const toggleEventLog = useCallback(() => setIsEventLogOpen((p) => !p), []);
  const openProviderSelector = useCallback(() => setIsProviderSelectorOpen(true), []);
  const closeProviderSelector = useCallback(() => setIsProviderSelectorOpen(false), []);
  const toggleMobileSidebar = useCallback(() => setIsMobileSidebarOpen((p) => !p), []);
  const closeMobileSidebar = useCallback(() => setIsMobileSidebarOpen(false), []);
  const openBrowserView = useCallback((url: string, camofoxTabId?: string) => {
    setBrowserViewUrl(url);
    setBrowserViewTabId(camofoxTabId ?? null);
    setIsBrowserViewOpen(true);
  }, []);
  // Set URL without opening the panel (for agent background navigation)
  const setBrowserViewUrlOnly = useCallback((url: string) => {
    setBrowserViewUrl(url);
  }, []);
  const closeBrowserView = useCallback(() => {
    setIsBrowserViewOpen(false);
    setBrowserViewUrl(null);
    setBrowserViewTabId(null);
  }, []);
  const clearBrowserView = closeBrowserView;
  const toggleBrowserView = useCallback(() => setIsBrowserViewOpen((p) => !p), []);

  const value = useMemo<UIContextValue>(
    () => ({
      viewMode: viewModeRaw,
      setViewMode,
      isRightSidebarOpen,
      toggleRightSidebar,
      closeRightSidebar,
      isAppSettingsOpen,
      openAppSettings,
      closeAppSettings,
      isModelConfigOpen,
      openModelConfig,
      closeModelConfig,
      isEventLogOpen,
      toggleEventLog,
      isProviderSelectorOpen,
      openProviderSelector,
      closeProviderSelector,
      isMobileSidebarOpen,
      toggleMobileSidebar,
      closeMobileSidebar,
      browserViewUrl,
      browserViewTabId,
      isBrowserViewOpen,
      openBrowserView,
      setBrowserViewUrlOnly,
      closeBrowserView,
      clearBrowserView,
      toggleBrowserView,
    }),
    [
      viewModeRaw,
      setViewMode,
      isRightSidebarOpen,
      toggleRightSidebar,
      closeRightSidebar,
      isAppSettingsOpen,
      openAppSettings,
      closeAppSettings,
      isModelConfigOpen,
      openModelConfig,
      closeModelConfig,
      isEventLogOpen,
      toggleEventLog,
      isProviderSelectorOpen,
      openProviderSelector,
      closeProviderSelector,
      isMobileSidebarOpen,
      toggleMobileSidebar,
      closeMobileSidebar,
      browserViewUrl,
      browserViewTabId,
      isBrowserViewOpen,
      openBrowserView,
      setBrowserViewUrlOnly,
      closeBrowserView,
      toggleBrowserView,
      clearBrowserView,
    ],
  );

  return <UIContext.Provider value={value}>{children}</UIContext.Provider>;
};
