import { useState, useEffect, useRef } from 'react';
import { Unplug, Radio } from 'lucide-react';
import { Card } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Toaster, toast } from 'react-hot-toast';
import { MessageBubble } from './components/MessageBubble';
import { LoadingIndicator } from './components/LoadingIndicator';
import { MessageInput } from './components/MessageInput';
import { SettingsModal } from './components/SettingsModal';
import { WelcomeMessage } from './components/WelcomeMessage';
import { ModelSelector } from './components/ModelSelector';
import Sidebar from './components/Sidebar';
import { useChat } from './hooks/useChat';
import { useModel } from './hooks/useModel';
import type { SamplerConfig } from './types';

type ViewMode = 'text' | 'markdown';

function App() {
  const { messages, isLoading, sendMessage, clearMessages, loadConversation, currentConversationId, tokensUsed, maxTokens, isWsConnected } = useChat();
  const { status: modelStatus, isLoading: isModelLoading, error: modelError, loadModel, unloadModel } = useModel();
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isSidebarOpen, setIsSidebarOpen] = useState(false);
  const [viewMode, setViewMode] = useState<ViewMode>('markdown');
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [messages, isLoading]);

  const handleNewConversation = () => {
    clearMessages();
  };

  const toggleSidebar = () => {
    setIsSidebarOpen(!isSidebarOpen);
  };

  const handleOpenSettings = () => {
    setIsSettingsOpen(true);
  };

  const handleModelLoad = async (modelPath: string, config: SamplerConfig) => {
    const result = await loadModel(modelPath, config);
    if (result.success) {
      toast.success('Model loaded successfully!');
    } else {
      toast.error(`Failed to load model: ${result.message}`, { duration: 5000 });
    }
  };

  const handleModelUnload = async () => {
    const result = await unloadModel();
    if (result.success) {
      toast.success('Model unloaded successfully');
      // Clear any existing conversation when model is unloaded
      clearMessages();
    } else {
      toast.error(`Failed to unload model: ${result.message}`, { duration: 5000 });
    }
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
        className={`flex-1 transition-all duration-300 ${
          isSidebarOpen ? 'md:ml-280' : 'md:ml-60'
        }`}
        style={{ marginLeft: isSidebarOpen ? '280px' : '60px' }}
      >
        <div className="flex flex-col h-full flat-card">
          {/* Header */}
          <div className="flex items-center justify-between p-6 flat-header" data-testid="chat-header">
            <button
              onClick={toggleSidebar}
              className="md:hidden p-2 hover:bg-muted rounded-lg"
              data-testid="mobile-sidebar-toggle"
            >
              ☰
            </button>
            <div className="flex-1 flex justify-center items-center">
              {modelStatus.loaded && (
                <div className="flex items-center gap-3">
                  <p className="text-lg font-semibold">
                    {(() => {
                      const fullPath = modelStatus.model_path || '';
                      const fileName = fullPath.split(/[/\\]/).pop() || 'Model loaded';
                      // Remove .gguf extension if present
                      return fileName.replace(/\.gguf$/i, '');
                    })()}
                  </p>
                  <button
                    onClick={handleModelUnload}
                    disabled={isModelLoading}
                    className="flat-button bg-destructive text-white px-4 py-2 disabled:opacity-50"
                    title="Unload model"
                  >
                    <Unplug className="h-4 w-4" />
                  </button>
                </div>
              )}
            </div>
            {messages.length > 0 && (
              <div className="flex items-center gap-4">
                {/* WebSocket Debug Info */}
                {isWsConnected && currentConversationId && (
                  <div className="flex items-center gap-2 px-3 py-1.5 bg-flat-green rounded-full">
                    <Radio className="h-3.5 w-3.5 text-white animate-pulse" />
                    <span className="text-xs font-medium text-white" title={`Connected to: ${currentConversationId}`}>
                      {currentConversationId.length > 20
                        ? `...${currentConversationId.slice(-20)}`
                        : currentConversationId}
                    </span>
                  </div>
                )}

                {/* View Mode Toggle */}
                <div className="flex items-center gap-2 bg-muted rounded-lg p-1">
                  <button
                    onClick={() => setViewMode('markdown')}
                    className={`px-4 py-2 font-medium text-sm transition-all rounded-md ${
                      viewMode === 'markdown'
                        ? 'bg-flat-red text-white'
                        : 'hover:bg-background'
                    }`}
                    title="Markdown view"
                  >
                    Markdown
                  </button>
                  <button
                    onClick={() => setViewMode('text')}
                    className={`px-4 py-2 font-medium text-sm transition-all rounded-md ${
                      viewMode === 'text'
                        ? 'bg-flat-red text-white'
                        : 'hover:bg-background'
                    }`}
                    title="Plain text view"
                  >
                    Plain Text
                  </button>
                </div>
              </div>
            )}
          </div>

          {/* Messages */}
          <div className="flex-1 overflow-y-auto p-6 space-y-4" data-testid="messages-container">
            {messages.length === 0 ? (
              <WelcomeMessage modelLoaded={modelStatus.loaded} isModelLoading={isModelLoading} />
            ) : (
              <>
                {messages.map((message) => (
                  <MessageBubble key={message.id} message={message} viewMode={viewMode} />
                ))}
                {isLoading && <LoadingIndicator />}
                <div ref={messagesEndRef} />
              </>
            )}
          </div>

          {/* Input / Model Selection */}
          <div className="border-t border-border bg-card p-6" data-testid="input-container">
            {modelStatus.loaded ? (
              <>
                {tokensUsed !== undefined && maxTokens !== undefined && (
                  <div className="mb-3 text-center text-sm font-medium text-muted-foreground">
                    Context: <span className="font-mono px-2 py-1 bg-muted rounded text-foreground">{tokensUsed}</span> / <span className="font-mono px-2 py-1 bg-muted rounded text-foreground">{maxTokens}</span> tokens
                  </div>
                )}
                <MessageInput onSendMessage={sendMessage} disabled={isLoading} />
              </>
            ) : (
              <div className="flex justify-center">
                <ModelSelector
                  onModelLoad={handleModelLoad}
                  currentModelPath={modelStatus.model_path || undefined}
                  isLoading={isModelLoading}
                  error={modelError}
                />
              </div>
            )}
          </div>
        </div>
      </div>

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