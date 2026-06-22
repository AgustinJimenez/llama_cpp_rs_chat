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
    <div className="flex h-screen bg-background" data-testid="chat-app">
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
                {/* eslint-disable i18next/no-literal-string */}
                {t.type === 'error' && (
                  <button
                    onClick={() => toast.dismiss(t.id)}
                    className="ml-2 text-lg leading-none text-white/70 hover:text-white"
                    aria-label="Dismiss"
                  >
                    ✕
                  </button>
                )}
                {/* eslint-enable i18next/no-literal-string */}
              </>
            )}
          </ToastBar>
        )}
      </Toaster>
    </div>
  );
};

function isProviderReady(
  modelLoaded: boolean,
  activeProvider: string,
  conversationAgent: { provider_id: string } | null,
  stagedAgent: { provider_id: string } | null,
): boolean {
  return (
    modelLoaded ||
    activeProvider !== 'local' ||
    conversationAgent?.provider_id !== 'local' ||
    stagedAgent?.provider_id !== 'local'
  );
}

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
  const providerReady = isProviderReady(
    modelStatus.loaded,
    activeProvider,
    conversationAgent,
    stagedAgent,
  );

  return (
    <main
      className="ml-0 flex-1 md:ml-[var(--sidebar-w)]"
      style={{ '--sidebar-w': `${sidebarWidth}px` } as React.CSSProperties}
    >
      {/* eslint-disable-next-line i18next/no-literal-string */}
      <h1 className="sr-only">LLaMA Chat</h1>
      <div className="flex h-full flex-col">
        <ConnectionBanner />
        {/* Agent selector modal */}
        <AgentSelector isOpen={isAgentSelectorOpen} onClose={closeAgentSelector} />
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
            className="absolute left-3 top-3 z-30 rounded-md p-2 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground md:hidden"
            title="Toggle sidebar"
            aria-label="Toggle sidebar"
          >
            <Menu className="size-5" />
          </button>
        )}

        {/* BrowserView stays mounted (hidden via CSS) so the Tauri native panel isn't destroyed on toggle */}
        <div className={browserViewClass}>
          <BrowserView />
        </div>
        {!isBrowserViewOpen && messages.length === 0 && (
          <WelcomeMessage>
            {!!providerReady && (
              <div className="w-full max-w-2xl space-y-1 px-3 md:px-6">
                {!!conversationAgent && (
                  <button
                    onClick={openAgentSelector}
                    className="flex items-center gap-1.5 rounded-full border border-border/50 px-2 py-0.5 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                  >
                    <Bot className="size-3" />
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
            {!!providerReady && (
              <div
                className="animate-in slide-in-from-bottom-4 px-3 pb-4 pt-2 duration-300 md:px-6"
                data-testid="input-container"
              >
                <div className="mx-auto max-w-3xl space-y-1">
                  {!!conversationAgent && (
                    <button
                      onClick={openAgentSelector}
                      className="flex items-center gap-1.5 rounded-full border border-border/50 px-2 py-0.5 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                    >
                      <Bot className="size-3" />
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
    </main>
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
  } = useUIContext();

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
      <DownloadFloat />
    </>
  );
};
