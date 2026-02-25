import { useState } from 'react';
import { Toaster, toast } from 'react-hot-toast';
import { ChatHeader, Sidebar, RightSidebar, ConversationConfigSidebar, AppSettingsModal } from './components/organisms';
import { ModelConfigModal } from './components/organisms/model-config';
import { MessagesArea } from './components/templates';
import { WelcomeMessage } from './components/atoms';
import { MessageInput } from './components/molecules';
import { useChat } from './hooks/useChat';
import { useModel } from './hooks/useModel';
import type { SamplerConfig, ViewMode } from './types';
import { logToastError } from './utils/toastLogger';

// eslint-disable-next-line max-lines-per-function
function App() {
  const { messages, isLoading, sendMessage, stopGeneration, clearMessages, loadConversation, currentConversationId, tokensUsed, maxTokens, lastTimings } = useChat();
  const { status: modelStatus, isLoading: isModelLoading, loadingAction, hasStatusError, loadModel, unloadModel, hardUnload } = useModel();

  // Compute clean model display name from path
  const modelName = modelStatus.model_path
    ? (modelStatus.model_path.split(/[/\\]/).pop() || '').replace(/\.gguf$/i, '')
    : '';
  const [isRightSidebarOpen, setIsRightSidebarOpen] = useState(false);
  const [isConfigSidebarOpen, setIsConfigSidebarOpen] = useState(false);
  const [isAppSettingsOpen, setIsAppSettingsOpen] = useState(false);
  const [isModelConfigOpen, setIsModelConfigOpen] = useState(false);
  const [viewMode, setViewMode] = useState<ViewMode>('markdown');

  const handleNewConversation = () => {
    clearMessages();
    // Focus the chat input after React re-renders
    requestAnimationFrame(() => {
      const input = document.querySelector<HTMLTextAreaElement>('[data-testid="message-input"]');
      input?.focus();
    });
  };

  const toggleRightSidebar = () => {
    setIsRightSidebarOpen(!isRightSidebarOpen);
  };

  const toggleConfigSidebar = () => {
    setIsConfigSidebarOpen(p => !p);
  };

  const handleModelConfigSave = (config: SamplerConfig) => {
    if (config.model_path) {
      handleModelLoad(config.model_path, config);
    }
    setIsModelConfigOpen(false);
  };

  const handleModelLoad = async (modelPath: string, config: SamplerConfig) => {
    const result = await loadModel(modelPath, config);
    if (result.success) {
      toast.success('Model loaded successfully!');
    } else {
      const display = `Failed to load model: ${result.message}`;
      logToastError('App.handleModelLoad', display);
      toast.error(display, { duration: 5000 });
    }
  };

  const handleModelUnload = async () => {
    const result = await unloadModel();
    if (result.success) {
      toast.success('Model unloaded successfully');
      // Clear any existing conversation when model is unloaded
      clearMessages();
    } else {
      const display = `Failed to unload model: ${result.message}`;
      logToastError('App.handleModelUnload', display);
      toast.error(display, { duration: 5000 });
    }
  };

  const handleForceUnload = async () => {
    await hardUnload();
    toast('Force-unloaded backend to free memory', { icon: '🧹' });
    clearMessages();
  };

  return (
    <div className="h-screen bg-background flex" data-testid="chat-app">
      {/* Sidebar */}
      <Sidebar
        onNewChat={handleNewConversation}
        onLoadConversation={loadConversation}
        currentConversationId={currentConversationId}
        onOpenAppSettings={() => setIsAppSettingsOpen(true)}
      />

      {/* Main Content */}
      <div
        className="flex-1 ml-[240px]"
      >
        <div className="flex flex-col h-full">
          {/* Header */}
          <ChatHeader
            modelLoaded={modelStatus.loaded}
            modelPath={modelStatus.model_path ?? undefined}
            isModelLoading={isModelLoading}
            tokensUsed={tokensUsed}
            maxTokens={maxTokens}
            genTokPerSec={lastTimings?.genTokPerSec}
            viewMode={viewMode}
            isRightSidebarOpen={isRightSidebarOpen}
            onOpenModelConfig={() => setIsModelConfigOpen(true)}
            onModelUnload={handleModelUnload}
            onForceUnload={handleForceUnload}
            hasStatusError={hasStatusError}
            onViewModeChange={setViewMode}
            onToggleRightSidebar={toggleRightSidebar}
            isConfigSidebarOpen={isConfigSidebarOpen}
            onToggleConfigSidebar={toggleConfigSidebar}
          />

          {messages.length === 0 ? (
            /* Centered welcome + input */
            <WelcomeMessage
              isModelLoading={isModelLoading}
              loadingAction={loadingAction}
              modelLoaded={modelStatus.loaded}
              modelName={modelName}
              onSelectModel={() => setIsModelConfigOpen(true)}
            >
              <div className="w-full max-w-2xl px-6">
                <MessageInput
                  onSendMessage={sendMessage}
                  onStopGeneration={stopGeneration}
                  disabled={isLoading}
                />
              </div>
            </WelcomeMessage>
          ) : (
            /* Normal chat layout */
            <>
              <MessagesArea
                messages={messages}
                isLoading={isLoading}
                viewMode={viewMode}
              />
              <div className="px-6 pb-4 pt-2 animate-in slide-in-from-bottom-4 duration-300" data-testid="input-container">
                <div className="max-w-3xl mx-auto">
                  <MessageInput
                    onSendMessage={sendMessage}
                    onStopGeneration={stopGeneration}
                    disabled={isLoading}
                  />
                </div>
              </div>
            </>
          )}
        </div>
      </div>

      {/* Right Sidebar - System Monitor */}
      <RightSidebar
        isOpen={isRightSidebarOpen}
        onClose={() => setIsRightSidebarOpen(false)}
      />

      {/* Right Sidebar - Conversation Config */}
      <ConversationConfigSidebar
        isOpen={isConfigSidebarOpen}
        onClose={() => setIsConfigSidebarOpen(false)}
        conversationId={currentConversationId}
        currentModelPath={modelStatus.model_path ?? undefined}
        onReloadModel={handleModelLoad}
      />

      {/* App Settings Modal */}
      <AppSettingsModal
        isOpen={isAppSettingsOpen}
        onClose={() => setIsAppSettingsOpen(false)}
      />

      {/* Model Config Modal */}
      <ModelConfigModal
        isOpen={isModelConfigOpen}
        onClose={() => setIsModelConfigOpen(false)}
        onSave={handleModelConfigSave}
        isLoading={isModelLoading}
        initialModelPath={modelStatus.model_path ?? undefined}
      />

      {/* Toast Notifications */}
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

export default App;
