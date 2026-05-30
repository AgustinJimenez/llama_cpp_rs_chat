import { Bot, Menu } from 'lucide-react';
import React, { useCallback, useEffect, Suspense } from 'react';
import toast, { Toaster, ToastBar } from 'react-hot-toast';

import { WelcomeMessage, ErrorBoundary } from './components/atoms';
import { ConnectionBanner, MessageInput } from './components/molecules';
import { ChatHeader, Sidebar } from './components/organisms';
import { AgentSelector } from './components/organisms/AgentSelector';
import { BrowserView } from './components/organisms/BrowserView';
import { ConversationLog } from './components/organisms/ConversationLog';
import { DownloadFloat } from './components/organisms/DownloadFloat';
import { ProviderSelector } from './components/organisms/ProviderSelector';
import { MessagesArea } from './components/templates';
import { useAgentContext } from './contexts/AgentContext';
import { useChatContext } from './contexts/ChatContext';
import { useModelContext } from './contexts/ModelContext';
import { useUIContext } from './hooks/useUIContext';
import type { SamplerConfig } from './types';
import { isTauriEnv } from './utils/tauri';

// Lazy-load overlay components (only rendered when opened)
const RightSidebar = React.lazy(() =>
  import('./components/organisms/RightSidebar').then((m) => ({ default: m.RightSidebar })),
);
const AppSettingsModal = React.lazy(() =>
  import('./components/organisms/AppSettingsModal').then((m) => ({ default: m.AppSettingsModal })),
);
const ConversationOverridesModal = React.lazy(() =>
  import('./components/organisms/ConversationOverridesModal').then((m) => ({
    default: m.ConversationOverridesModal,
  })),
);
const ModelConfigModal = React.lazy(() =>
  import('./components/organisms/model-config').then((m) => ({ default: m.ModelConfigModal })),
);

// eslint-disable-next-line max-lines-per-function
export const App = () => {
  const { status: modelStatus, loadModel, unloadModel, forceUnload } = useModelContext();
  const { clearMessages } = useChatContext();
  const {
    closeModelConfig,
    openAppSettings,
    openBrowserView,
    clearBrowserView,
    setBrowserViewUrlOnly,
  } = useUIContext();

  // Listen for Tauri menu/tray events
  useEffect(() => {
    if (!isTauriEnv()) return;
    const unlisten: Array<() => void> = [];
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      unlisten.push(
        await listen('new-chat', () => {
          clearMessages();
          requestAnimationFrame(() => {
            document.querySelector<HTMLTextAreaElement>('[data-testid="message-input"]')?.focus();
          });
        }),
      );
      unlisten.push(
        await listen('open-settings', () => {
          openAppSettings();
        }),
      );
      unlisten.push(
        await listen('conversation-title-updated', () => {
          window.dispatchEvent(new CustomEvent('conversation-title-updated'));
        }),
      );
    })();
    return () => {
      unlisten.forEach((fn) => fn());
    };
  }, [clearMessages, openAppSettings]);

  useEffect(() => {
    (
      window as Window & {
        __openBrowserView?: (url: string) => void;
        __closeBrowserView?: () => void;
      }
    ).__openBrowserView = (url: string) => {
      // Set the URL silently — agent browses in background.
      // User can open the globe icon to see what the agent is browsing.
      setBrowserViewUrlOnly(url);
    };
    (
      window as Window & {
        __openBrowserView?: (url: string) => void;
        __closeBrowserView?: () => void;
      }
    ).__closeBrowserView = () => {
      clearBrowserView();
    };

    return () => {
      delete (
        window as Window & {
          __openBrowserView?: (url: string) => void;
          __closeBrowserView?: () => void;
        }
      ).__openBrowserView;
      delete (
        window as Window & {
          __openBrowserView?: (url: string) => void;
          __closeBrowserView?: () => void;
        }
      ).__closeBrowserView;
    };
  }, [openBrowserView, clearBrowserView, setBrowserViewUrlOnly]);

  const handleNewConversation = useCallback(() => {
    clearMessages();
    requestAnimationFrame(() => {
      const input = document.querySelector<HTMLTextAreaElement>('[data-testid="message-input"]');
      input?.focus();
    });
  }, [clearMessages]);

  const handleModelConfigSave = useCallback(
    (config: SamplerConfig) => {
      if (config.model_path) {
        loadModel(config.model_path, config);
      }
      closeModelConfig();
    },
    [loadModel, closeModelConfig],
  );

  const handleModelUnload = useCallback(async () => {
    await unloadModel();
    clearMessages();
  }, [unloadModel, clearMessages]);

  const handleForceUnload = useCallback(async () => {
    await forceUnload();
    clearMessages();
  }, [forceUnload, clearMessages]);

  return (
    <div className="h-screen bg-background flex" data-testid="chat-app">
      <Sidebar onNewChat={handleNewConversation} />

      <ErrorBoundary>
        <MainContent handleModelUnload={handleModelUnload} handleForceUnload={handleForceUnload} />
      </ErrorBoundary>

      <Overlays
        modelPath={modelStatus.model_path ?? undefined}
        onModelConfigSave={handleModelConfigSave}
      />

      <Toaster
        position="top-right"
        toastOptions={{
          duration: 3000,
          style: {
            background: 'hsl(var(--card))',
            color: 'hsl(var(--foreground))',
            border: '1px solid hsl(var(--border))',
            borderRadius: '0.5rem',
            fontWeight: '500',
            padding: '16px',
          },
          success: {
            duration: 3000,
            style: {
              background: 'hsl(var(--flat-green))',
              color: '#fff',
              border: 'none',
              borderRadius: '0.5rem',
              fontWeight: '500',
            },
            iconTheme: {
              primary: '#fff',
              secondary: 'hsl(var(--flat-green))',
            },
          },
          error: {
            duration: Infinity,
            style: {
              background: 'hsl(var(--flat-red))',
              color: '#fff',
              border: 'none',
              borderRadius: '0.5rem',
              fontWeight: '500',
              cursor: 'pointer',
            },
            iconTheme: {
              primary: '#fff',
              secondary: 'hsl(var(--flat-red))',
            },
          },
        }}
      >
        {(t) => (
          <ToastBar toast={t}>
            {({ icon, message }) => (
              <>
                {icon}
                {message}
                {t.type === 'error' && (
                  <button
                    onClick={() => toast.dismiss(t.id)}
                    className="ml-2 text-white/70 hover:text-white text-lg leading-none"
                    aria-label="Dismiss"
                  >
                    ✕
                  </button>
                )}
              </>
            )}
          </ToastBar>
        )}
      </Toaster>
    </div>
  );
};

/** Main content area: header + messages/welcome + input */
// eslint-disable-next-line max-lines-per-function
const MainContent = ({
  handleModelUnload,
  handleForceUnload,
}: {
  handleModelUnload: () => void;
  handleForceUnload: () => void;
}) => {
  const {
    status: modelStatus,
    activeProvider,
    activeProviderModel,
    activeProviderParams,
    setRemoteProvider,
    setLocalProvider,
  } = useModelContext();
  const {
    messages,
    lastTimings,
    tokensUsed,
    maxTokens,
    streamStatus,
    providerRef,
    providerParamsRef,
    currentConversationId,
  } = useChatContext();
  const {
    isProviderSelectorOpen,
    closeProviderSelector,
    isAgentSelectorOpen,
    closeAgentSelector,
    openAgentSelector,
    openModelConfig,
    toggleMobileSidebar,
    isBrowserViewOpen,
    sidebarWidth,
  } = useUIContext();

  const {
    loadConversationAgent,
    conversationAgent,
    stagedAgent,
    setStagedAgent,
    setConversationAgent,
  } = useAgentContext();

  // Load agent when conversation changes; auto-assign staged agent only if conversation has none
  useEffect(() => {
    if (!currentConversationId) return;
    loadConversationAgent(currentConversationId)
      .then((existing) => {
        if (!existing && stagedAgent) {
          return setConversationAgent(currentConversationId, stagedAgent.id).then(() =>
            setStagedAgent(null),
          );
        }
      })
      .catch(() => {});
  }, [
    currentConversationId,
    loadConversationAgent,
    stagedAgent,
    setConversationAgent,
    setStagedAgent,
  ]);

  // Sync provider refs with model context (in effect to avoid updating refs during render)
  useEffect(() => {
    if (providerRef) {
      providerRef.current = { provider: activeProvider, model: activeProviderModel };
    }
    if (providerParamsRef) {
      providerParamsRef.current = activeProviderParams;
    }
  }, [providerRef, providerParamsRef, activeProvider, activeProviderModel, activeProviderParams]);

  const browserViewClass = isBrowserViewOpen ? 'flex flex-col flex-1 overflow-hidden' : 'hidden';

  return (
    <div
      className="flex-1 ml-0 md:ml-[var(--sidebar-w)]"
      style={{ '--sidebar-w': `${sidebarWidth}px` } as React.CSSProperties}
    >
      <div className="flex flex-col h-full">
        <ConnectionBanner />
        {/* Agent selector modal */}
        <AgentSelector
          isOpen={isAgentSelectorOpen}
          onClose={closeAgentSelector}
          conversationId={currentConversationId ?? undefined}
        />
        {/* Global provider selector — accessible from welcome screen and header */}
        <ProviderSelector
          isOpen={isProviderSelectorOpen}
          onClose={closeProviderSelector}
          onSelectLocal={() => {
            closeProviderSelector();
            setLocalProvider();
            openModelConfig();
          }}
          onSelectRemote={(provider, model) => {
            closeProviderSelector();
            setRemoteProvider(provider, model);
          }}
          currentProvider={activeProvider}
        />
        <ChatHeader
          onModelUnload={handleModelUnload}
          onForceUnload={handleForceUnload}
          showAgentSelector
        />
        {/* Mobile hamburger when header is not visible on small screens */}
        {!messages.length && !modelStatus.loaded && activeProvider === 'local' && (
          <button
            onClick={toggleMobileSidebar}
            className="absolute top-3 left-3 z-30 p-2 rounded-md text-muted-foreground hover:text-foreground hover:bg-muted transition-colors md:hidden"
            title="Toggle sidebar"
            aria-label="Toggle sidebar"
          >
            <Menu className="h-5 w-5" />
          </button>
        )}

        {/* BrowserView stays mounted (hidden via CSS) so the Tauri native panel isn't destroyed on toggle */}
        <div className={browserViewClass}>
          <BrowserView />
        </div>
        {!isBrowserViewOpen && messages.length === 0 && (
          <WelcomeMessage>
            {!!(modelStatus.loaded || activeProvider !== 'local') && (
              <div className="w-full max-w-2xl px-3 md:px-6 space-y-1">
                {!!conversationAgent && (
                  <button
                    onClick={openAgentSelector}
                    className="flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs text-muted-foreground hover:text-foreground hover:bg-muted transition-colors border border-border/50"
                  >
                    <Bot className="h-3 w-3" />
                    {conversationAgent.name}
                  </button>
                )}
                <MessageInput />
              </div>
            )}
          </WelcomeMessage>
        )}
        {!isBrowserViewOpen && messages.length > 0 && (
          <>
            <MessagesArea />
            <ConversationLog />
            {!!(modelStatus.loaded || activeProvider !== 'local') && (
              <div
                className="px-3 md:px-6 pb-4 pt-2 animate-in slide-in-from-bottom-4 duration-300"
                data-testid="input-container"
              >
                <div className="max-w-3xl mx-auto space-y-1">
                  {!!conversationAgent && (
                    <button
                      onClick={openAgentSelector}
                      className="flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs text-muted-foreground hover:text-foreground hover:bg-muted transition-colors border border-border/50"
                    >
                      <Bot className="h-3 w-3" />
                      {conversationAgent.name}
                    </button>
                  )}
                  <MessageInput
                    timings={lastTimings}
                    tokensUsed={tokensUsed}
                    maxTokens={maxTokens}
                    streamStatus={streamStatus}
                  />
                </div>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
};

/** Sidebars and modals that overlay the main content */
const Overlays = ({
  modelPath,
  onModelConfigSave,
}: {
  modelPath?: string;
  onModelConfigSave: (config: SamplerConfig) => void;
}) => {
  const {
    isRightSidebarOpen,
    closeRightSidebar,
    isAppSettingsOpen,
    closeAppSettings,
    isModelConfigOpen,
    closeModelConfig,
    isConversationOverridesOpen,
    closeConversationOverrides,
  } = useUIContext();
  const { currentConversationId } = useChatContext();

  return (
    <>
      {!!isRightSidebarOpen && (
        <ErrorBoundary>
          <Suspense fallback={null}>
            <RightSidebar isOpen={isRightSidebarOpen} onClose={closeRightSidebar} />
          </Suspense>
        </ErrorBoundary>
      )}
      {!!isAppSettingsOpen && (
        <ErrorBoundary>
          <Suspense fallback={null}>
            <AppSettingsModal isOpen={isAppSettingsOpen} onClose={closeAppSettings} />
          </Suspense>
        </ErrorBoundary>
      )}
      {!!isModelConfigOpen && (
        <ErrorBoundary>
          <Suspense fallback={null}>
            <ModelConfigModal
              isOpen={isModelConfigOpen}
              onClose={closeModelConfig}
              onSave={onModelConfigSave}
              initialModelPath={modelPath}
            />
          </Suspense>
        </ErrorBoundary>
      )}
      {!!isConversationOverridesOpen && (
        <ErrorBoundary>
          <Suspense fallback={null}>
            <ConversationOverridesModal
              isOpen={isConversationOverridesOpen}
              onClose={closeConversationOverrides}
              conversationId={currentConversationId}
            />
          </Suspense>
        </ErrorBoundary>
      )}
      <DownloadFloat />
    </>
  );
};
