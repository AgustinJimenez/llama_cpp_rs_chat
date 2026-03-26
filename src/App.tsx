import React, { useCallback, useEffect, Suspense } from 'react';
import { Toaster } from 'react-hot-toast';
import { ChatHeader, Sidebar } from './components/organisms';
import { ConversationLog } from './components/organisms/ConversationLog';
import { MessagesArea } from './components/templates';
import { WelcomeMessage, ErrorBoundary } from './components/atoms';
import { ConnectionBanner, MessageInput } from './components/molecules';
import { useModelContext } from './contexts/ModelContext';
import { useChatContext } from './contexts/ChatContext';
import { useUIContext } from './contexts/UIContext';
import type { SamplerConfig } from './types';
import { DownloadFloat } from './components/organisms/DownloadFloat';
import { isTauriEnv } from './utils/tauri';

// Lazy-load overlay components (only rendered when opened)
const RightSidebar = React.lazy(() => import('./components/organisms/RightSidebar').then(m => ({ default: m.RightSidebar })));
const ConversationConfigSidebar = React.lazy(() => import('./components/organisms/ConversationConfigSidebar').then(m => ({ default: m.ConversationConfigSidebar })));
const AppSettingsModal = React.lazy(() => import('./components/organisms/AppSettingsModal').then(m => ({ default: m.AppSettingsModal })));
const ModelConfigModal = React.lazy(() => import('./components/organisms/model-config').then(m => ({ default: m.ModelConfigModal })));

function App() {
  const { status: modelStatus, loadModel, unloadModel, forceUnload } = useModelContext();
  const { clearMessages } = useChatContext();
  const { closeModelConfig, openAppSettings } = useUIContext();

  // Listen for Tauri menu/tray events
  useEffect(() => {
    if (!isTauriEnv()) return;
    let unlisten: Array<() => void> = [];
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      unlisten.push(await listen('new-chat', () => {
        clearMessages();
        requestAnimationFrame(() => {
          document.querySelector<HTMLTextAreaElement>('[data-testid="message-input"]')?.focus();
        });
      }));
      unlisten.push(await listen('open-settings', () => {
        openAppSettings();
      }));
    })();
    return () => { unlisten.forEach(fn => fn()); };
  }, [clearMessages, openAppSettings]);

  const handleNewConversation = useCallback(() => {
    clearMessages();
    requestAnimationFrame(() => {
      const input = document.querySelector<HTMLTextAreaElement>('[data-testid="message-input"]');
      input?.focus();
    });
  }, [clearMessages]);

  const handleModelConfigSave = useCallback((config: SamplerConfig) => {
    if (config.model_path) {
      loadModel(config.model_path, config);
    }
    closeModelConfig();
  }, [loadModel, closeModelConfig]);

  const handleModelUnload = useCallback(async () => {
    await unloadModel();
    clearMessages();
  }, [unloadModel, clearMessages]);

  const handleForceUnload = useCallback(async () => {
    await forceUnload();
    clearMessages();
  }, [forceUnload, clearMessages]);

  const handleReloadModel = useCallback((modelPath: string, config: SamplerConfig) => {
    loadModel(modelPath, config);
  }, [loadModel]);

  return (
    <div className="h-screen bg-background flex" data-testid="chat-app">
      <Sidebar onNewChat={handleNewConversation} />

      <ErrorBoundary>
        <MainContent
          handleModelUnload={handleModelUnload}
          handleForceUnload={handleForceUnload}
        />
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
            duration: 5000,
            style: {
              background: 'hsl(var(--flat-red))',
              color: '#fff',
              border: 'none',
              borderRadius: '0.5rem',
              fontWeight: '500',
            },
            iconTheme: {
              primary: '#fff',
              secondary: 'hsl(var(--flat-red))',
            },
          },
        }}
      />
    </div>
  );
}

/** Main content area: header + messages/welcome + input */
function MainContent({
  handleModelUnload,
  handleForceUnload,
}: {
  handleModelUnload: () => void;
  handleForceUnload: () => void;
}) {
  const { status: modelStatus, activeProvider, activeProviderModel } = useModelContext();
  const { messages, lastTimings, tokensUsed, maxTokens, streamStatus, providerRef } = useChatContext();

  // Sync provider ref with model context
  if (providerRef) {
    providerRef.current = { provider: activeProvider, model: activeProviderModel };
  }

  return (
    <div className="flex-1 ml-[240px]">
      <div className="flex flex-col h-full">
        <ConnectionBanner />
        {/* Header hidden during loading with no conversation — WelcomeMessage shows the loading progress instead (only one loading indicator at a time) */}
        {(messages.length > 0 || modelStatus.loaded || activeProvider !== 'local') ? (
          <ChatHeader
            onModelUnload={handleModelUnload}
            onForceUnload={handleForceUnload}
          />
        ) : null}

        {messages.length === 0 ? (
          <WelcomeMessage>
            {(modelStatus.loaded || activeProvider !== 'local') ? (
              <div className="w-full max-w-2xl px-6">
                <MessageInput />
              </div>
            ) : null}
          </WelcomeMessage>
        ) : (
          <>
            <MessagesArea />
            <ConversationLog />
            {(modelStatus.loaded || activeProvider !== 'local') ? (
              <div className="px-6 pb-4 pt-2 animate-in slide-in-from-bottom-4 duration-300" data-testid="input-container">
                <div className="max-w-3xl mx-auto">
                  <MessageInput timings={lastTimings} tokensUsed={tokensUsed} maxTokens={maxTokens} streamStatus={streamStatus} />
                </div>
              </div>
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}

/** Sidebars and modals that overlay the main content */
function Overlays({
  modelPath,
  onModelConfigSave,
  onReloadModel,
}: {
  modelPath?: string;
  onModelConfigSave: (config: SamplerConfig) => void;
  onReloadModel: (modelPath: string, config: SamplerConfig) => void;
}) {
  const { isRightSidebarOpen, closeRightSidebar, isConfigSidebarOpen, closeConfigSidebar, isAppSettingsOpen, closeAppSettings, isModelConfigOpen, closeModelConfig } = useUIContext();
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
}

export default App;
