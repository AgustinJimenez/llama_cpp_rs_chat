import { useState } from 'react';
import { Toaster, toast } from 'react-hot-toast';
import { ChatHeader, SettingsModal, Sidebar, RightSidebar } from './components/organisms';
import { ChatInputArea, MessagesArea } from './components/templates';
import { useChat } from './hooks/useChat';
import { useModel } from './hooks/useModel';
import type { SamplerConfig, ViewMode } from './types';
import { logToastError } from './utils/toastLogger';

// eslint-disable-next-line max-lines-per-function
function App() {
  const { messages, isLoading, sendMessage, clearMessages, loadConversation, currentConversationId, tokensUsed, maxTokens, isWsConnected } = useChat();
  const { status: modelStatus, isLoading: isModelLoading, loadingAction, error: modelError, hasStatusError, loadModel, unloadModel, hardUnload } = useModel();
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isSidebarOpen, setIsSidebarOpen] = useState(false);
  const [isRightSidebarOpen, setIsRightSidebarOpen] = useState(false);
  const [viewMode, setViewMode] = useState<ViewMode>('markdown');

  const handleNewConversation = () => {
    clearMessages();
  };

  const toggleSidebar = () => {
    setIsSidebarOpen(!isSidebarOpen);
  };

  const toggleRightSidebar = () => {
    setIsRightSidebarOpen(!isRightSidebarOpen);
  };

  const handleOpenSettings = () => {
    setIsSettingsOpen(true);
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
        isOpen={isSidebarOpen}
        onToggle={toggleSidebar}
        onNewChat={handleNewConversation}
        onOpenSettings={handleOpenSettings}
        onLoadConversation={loadConversation}
        currentConversationId={currentConversationId}
      />

      {/* Main Content */}
      <div
        className={`flex-1 transition-all duration-300`}
        style={{
          marginLeft: isSidebarOpen ? '280px' : '60px'
        }}
      >
        <div className="flex flex-col h-full flat-card">
          {/* Header */}
          <ChatHeader
            isSidebarOpen={isSidebarOpen}
            modelLoaded={modelStatus.loaded}
            modelPath={modelStatus.model_path ?? undefined}
            isModelLoading={isModelLoading}
            messagesLength={messages.length}
            tokensUsed={tokensUsed}
            maxTokens={maxTokens}
            isWsConnected={isWsConnected}
            currentConversationId={currentConversationId ?? undefined}
            viewMode={viewMode}
            isRightSidebarOpen={isRightSidebarOpen}
            onToggleSidebar={toggleSidebar}
            onModelUnload={handleModelUnload}
            onForceUnload={handleForceUnload}
            hasStatusError={hasStatusError}
            onViewModeChange={setViewMode}
            onToggleRightSidebar={toggleRightSidebar}
          />

          {/* Messages */}
          <MessagesArea
            messages={messages}
            isLoading={isLoading}
            modelLoaded={modelStatus.loaded}
            isModelLoading={isModelLoading}
            loadingAction={loadingAction}
            viewMode={viewMode}
          />

          {/* Input / Model Selection */}
          <ChatInputArea
            modelLoaded={modelStatus.loaded}
            currentModelPath={modelStatus.model_path ?? undefined}
            isModelLoading={isModelLoading}
            modelError={modelError}
            isLoading={isLoading}
            isWsConnected={isWsConnected}
            currentConversationId={currentConversationId}
            onSendMessage={sendMessage}
            onModelLoad={handleModelLoad}
          />
        </div>
      </div>

      {/* Right Sidebar - System Monitor */}
      <RightSidebar
        isOpen={isRightSidebarOpen}
        onClose={() => setIsRightSidebarOpen(false)}
      />

      {/* Settings Modal */}
      <SettingsModal
        isOpen={isSettingsOpen}
        onClose={() => setIsSettingsOpen(false)}
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
