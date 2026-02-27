import { useCallback } from 'react';
import { Toaster } from 'react-hot-toast';
import { ChatHeader, Sidebar, RightSidebar, ConversationConfigSidebar, AppSettingsModal } from './components/organisms';
import { ModelConfigModal } from './components/organisms/model-config';
import { MessagesArea } from './components/templates';
import { WelcomeMessage } from './components/atoms';
import { MessageInput, MessageStatistics } from './components/molecules';
import { useModelContext } from './contexts/ModelContext';
import { useChatContext } from './contexts/ChatContext';
import { useUIContext } from './contexts/UIContext';
import type { SamplerConfig } from './types';

function App() {
  const { status: modelStatus, loadModel, unloadModel, forceUnload } = useModelContext();
  const { clearMessages } = useChatContext();
  const { closeModelConfig } = useUIContext();

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

      <MainContent
        handleModelUnload={handleModelUnload}
        handleForceUnload={handleForceUnload}
      />

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
  const { status: modelStatus, isLoading: isModelLoading } = useModelContext();
  const { messages, lastTimings, isLoading, tokensUsed, maxTokens } = useChatContext();

  return (
    <div className="flex-1 ml-[240px]">
      <div className="flex flex-col h-full">
        {(messages.length > 0 || modelStatus.loaded || isModelLoading) && (
          <ChatHeader
            onModelUnload={handleModelUnload}
            onForceUnload={handleForceUnload}
          />
        )}

        {messages.length === 0 ? (
          <WelcomeMessage>
            <div className="w-full max-w-2xl px-6">
              <MessageInput />
            </div>
          </WelcomeMessage>
        ) : (
          <>
            <MessagesArea />
            <div className="px-6 pb-4 pt-2 animate-in slide-in-from-bottom-4 duration-300" data-testid="input-container">
              <div className="max-w-3xl mx-auto">
                {lastTimings?.genTokPerSec && !isLoading ? (
                  <MessageStatistics timings={lastTimings} tokensUsed={tokensUsed} maxTokens={maxTokens} />
                ) : null}
                <MessageInput />
              </div>
            </div>
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
      <RightSidebar isOpen={isRightSidebarOpen} onClose={closeRightSidebar} />
      <ConversationConfigSidebar
        isOpen={isConfigSidebarOpen}
        onClose={closeConfigSidebar}
        conversationId={currentConversationId}
        currentModelPath={modelPath}
        onReloadModel={onReloadModel}
      />
      <AppSettingsModal isOpen={isAppSettingsOpen} onClose={closeAppSettings} />
      <ModelConfigModal
        isOpen={isModelConfigOpen}
        onClose={closeModelConfig}
        onSave={onModelConfigSave}
        initialModelPath={modelPath}
      />
    </>
  );
}

export default App;
