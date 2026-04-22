import { Menu } from 'lucide-react';
import React, { useCallback, useEffect, Suspense } from 'react';
import toast, { Toaster, ToastBar } from 'react-hot-toast';

import { WelcomeMessage, ErrorBoundary } from './components/atoms';
import { ConnectionBanner, MessageInput } from './components/molecules';
import { ChatHeader, Sidebar } from './components/organisms';
import { BrowserView } from './components/organisms/BrowserView';
import { ConversationLog } from './components/organisms/ConversationLog';
import { DownloadFloat } from './components/organisms/DownloadFloat';
import { ProviderSelector } from './components/organisms/ProviderSelector';
import { MessagesArea } from './components/templates';
import { useChatContext } from './contexts/ChatContext';
import { useModelContext } from './contexts/ModelContext';
import { useCamofoxCaptcha } from './hooks/useCamofoxCaptcha';
import { useUIContext } from './hooks/useUIContext';
import type { SamplerConfig } from './types';
import { isTauriEnv } from './utils/tauri';

// Lazy-load overlay components (only rendered when opened)
const RightSidebar = React.lazy(() =>
  import('./components/organisms/RightSidebar').then((m) => ({ default: m.RightSidebar })),
);
const ConversationConfigSidebar = React.lazy(() =>
  import('./components/organisms/ConversationConfigSidebar').then((m) => ({
    default: m.ConversationConfigSidebar,
  })),
);
const AppSettingsModal = React.lazy(() =>
  import('./components/organisms/AppSettingsModal').then((m) => ({ default: m.AppSettingsModal })),
);
const ModelConfigModal = React.lazy(() =>
  import('./components/organisms/model-config').then((m) => ({ default: m.ModelConfigModal })),
);

// eslint-disable-next-line max-lines-per-function
const App = () => {
  const { status: modelStatus, loadModel, unloadModel, forceUnload } = useModelContext();
  const { clearMessages } = useChatContext();
  const { closeModelConfig, openAppSettings, openBrowserView, closeBrowserView } = useUIContext();

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
      openBrowserView(url);
    };
    (
      window as Window & {
        __openBrowserView?: (url: string) => void;
        __closeBrowserView?: () => void;
      }
    ).__closeBrowserView = () => {
      closeBrowserView();
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
  }, [openBrowserView, closeBrowserView]);

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

  const handleReloadModel = useCallback(
    (modelPath: string, config: SamplerConfig) => {
      loadModel(modelPath, config);
    },
    [loadModel],
  );

  return (
    <div className="h-screen bg-background flex" data-testid="chat-app">
      <Sidebar onNewChat={handleNewConversation} />

      <ErrorBoundary>
        <MainContent handleModelUnload={handleModelUnload} handleForceUnload={handleForceUnload} />
      </ErrorBoundary>

      <Overlays
        modelPath={modelStatus.model_path ?? undefined}
        onModelConfigSave={handleModelConfigSave}
        onReloadModel={handleReloadModel}
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
                {t.type === 'error' ? (
                  <button
                    onClick={() => toast.dismiss(t.id)}
                    className="ml-2 text-white/70 hover:text-white text-lg leading-none"
                    aria-label="Dismiss"
                  >
                    ✕
                  </button>
                ) : null}
              </>
            )}
          </ToastBar>
        )}
      </Toaster>
    </div>
  );
};

/** Main content area: header + messages/welcome + input */
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
    setRemoteProvider,
    setLocalProvider,
  } = useModelContext();
  const { messages, lastTimings, tokensUsed, maxTokens, streamStatus, providerRef } =
    useChatContext();
  const {
    isProviderSelectorOpen,
    closeProviderSelector,
    openModelConfig,
    toggleMobileSidebar,
    isBrowserViewOpen,
  } = useUIContext();

  // Poll for CAPTCHA status + agent browser view — auto-opens browser view when detected
  useCamofoxCaptcha();

  // Sync provider ref with model context
  if (providerRef) {
    providerRef.current = { provider: activeProvider, model: activeProviderModel };
  }

  return (
    <div className="flex-1 ml-0 md:ml-[240px]">
      <div className="flex flex-col h-full">
        <ConnectionBanner />
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
        {/* Header hidden during loading with no conversation — WelcomeMessage shows the loading progress instead (only one loading indicator at a time) */}
        {messages.length > 0 || modelStatus.loaded || activeProvider !== 'local' ? (
          <ChatHeader onModelUnload={handleModelUnload} onForceUnload={handleForceUnload} />
        ) : (
          /* Mobile hamburger when header is hidden */
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
        <div className={isBrowserViewOpen ? 'flex flex-col flex-1 overflow-hidden' : 'hidden'}>
          <BrowserView />
        </div>
        {!isBrowserViewOpen && messages.length === 0 && (
          <WelcomeMessage>
            {modelStatus.loaded || activeProvider !== 'local' ? (
              <div className="w-full max-w-2xl px-3 md:px-6">
                <MessageInput />
              </div>
            ) : null}
          </WelcomeMessage>
        )}
        {!isBrowserViewOpen && messages.length > 0 && (
          <>
            <MessagesArea />
            <ConversationLog />
            {modelStatus.loaded || activeProvider !== 'local' ? (
              <div
                className="px-3 md:px-6 pb-4 pt-2 animate-in slide-in-from-bottom-4 duration-300"
                data-testid="input-container"
              >
                <div className="max-w-3xl mx-auto">
                  <MessageInput
                    timings={lastTimings}
                    tokensUsed={tokensUsed}
                    maxTokens={maxTokens}
                    streamStatus={streamStatus}
                  />
                </div>
              </div>
            ) : null}
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
  onReloadModel,
}: {
  modelPath?: string;
  onModelConfigSave: (config: SamplerConfig) => void;
  onReloadModel: (modelPath: string, config: SamplerConfig) => void;
}) => {
  const {
    isRightSidebarOpen,
    closeRightSidebar,
    isConfigSidebarOpen,
    closeConfigSidebar,
    isAppSettingsOpen,
    closeAppSettings,
    isModelConfigOpen,
    closeModelConfig,
  } = useUIContext();
  const { currentConversationId } = useChatContext();

  return (
    <>
      {isRightSidebarOpen ? (
        <ErrorBoundary>
          <Suspense fallback={null}>
            <RightSidebar isOpen={isRightSidebarOpen} onClose={closeRightSidebar} />
          </Suspense>
        </ErrorBoundary>
      ) : null}
      {isConfigSidebarOpen ? (
        <ErrorBoundary>
          <Suspense fallback={null}>
            <ConversationConfigSidebar
              isOpen={isConfigSidebarOpen}
              onClose={closeConfigSidebar}
              conversationId={currentConversationId}
              currentModelPath={modelPath}
              onReloadModel={onReloadModel}
            />
          </Suspense>
        </ErrorBoundary>
      ) : null}
      {isAppSettingsOpen ? (
        <ErrorBoundary>
          <Suspense fallback={null}>
            <AppSettingsModal isOpen={isAppSettingsOpen} onClose={closeAppSettings} />
          </Suspense>
        </ErrorBoundary>
      ) : null}
      {isModelConfigOpen ? (
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
      ) : null}
      <DownloadFloat />
    </>
  );
};

export default App;
