import { useState, useEffect, useRef } from 'react';
import { X } from 'lucide-react';
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
  const { messages, isLoading, sendMessage, clearMessages, loadConversation, currentConversationId } = useChat();
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
      console.log('Model loaded successfully:', result.message);
    } else {
      toast.error(`Failed to load model: ${result.message}`, { duration: 5000 });
      console.error('Failed to load model:', result.message);
    }
  };

  const handleModelUnload = async () => {
    const result = await unloadModel();
    if (result.success) {
      toast.success('Model unloaded successfully');
      console.log('Model unloaded successfully:', result.message);
      // Clear any existing conversation when model is unloaded
      clearMessages();
    } else {
      toast.error(`Failed to unload model: ${result.message}`, { duration: 5000 });
      console.error('Failed to unload model:', result.message);
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
        className={`flex-1 p-4 transition-all duration-300 ${
          isSidebarOpen ? 'md:ml-280' : 'md:ml-60'
        }`}
        style={{ marginLeft: isSidebarOpen ? '280px' : '60px' }}
      >
        <Card className="flex flex-col h-full bg-card border shadow-2xl">
          {/* Header */}
          <div className="flex items-center justify-between p-6 border-b bg-gradient-to-r from-slate-700 to-slate-600 text-white rounded-t-lg" data-testid="chat-header">
            <button
              onClick={toggleSidebar}
              className="md:hidden p-2 text-white hover:bg-white/10 rounded-lg"
              data-testid="mobile-sidebar-toggle"
            >
              ☰
            </button>
            <div className="flex-1 flex justify-center items-center">
              {modelStatus.loaded && (
                <div className="flex items-center gap-3">
                  <p className="text-lg font-semibold text-white">
                    {(() => {
                      const fullPath = modelStatus.model_path || '';
                      const fileName = fullPath.split(/[/\\]/).pop() || 'Model loaded';
                      // Remove .gguf extension if present
                      return fileName.replace(/\.gguf$/i, '');
                    })()}
                  </p>
                  <Button
                    onClick={handleModelUnload}
                    disabled={isModelLoading}
                    variant="outline"
                    size="sm"
                    className="bg-white/10 hover:bg-white/20 border-white/20 text-white"
                    title="Unload model"
                  >
                    <X className="h-4 w-4" />
                  </Button>
                </div>
              )}
            </div>
            <div className="flex items-center">
              <Button
                onClick={() => setViewMode('markdown')}
                variant="outline"
                size="sm"
                className={`${
                  viewMode === 'markdown'
                    ? 'bg-white/20 border-white/40'
                    : 'bg-white/10 border-white/20'
                } hover:bg-white/20 text-white rounded-l-full rounded-r-none border-r-0`}
                title="Markdown view"
              >
                Markdown
              </Button>
              <Button
                onClick={() => setViewMode('text')}
                variant="outline"
                size="sm"
                className={`${
                  viewMode === 'text'
                    ? 'bg-white/20 border-white/40'
                    : 'bg-white/10 border-white/20'
                } hover:bg-white/20 text-white rounded-r-full rounded-l-none`}
                title="Plain text view"
              >
                Plain Text
              </Button>
            </div>
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
          <div className="border-t bg-muted/20 p-6" data-testid="input-container">
            {modelStatus.loaded ? (
              <MessageInput onSendMessage={sendMessage} disabled={isLoading} />
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
        </Card>
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
            background: '#363636',
            color: '#fff',
          },
          success: {
            duration: 3000,
            iconTheme: {
              primary: '#10b981',
              secondary: '#fff',
            },
          },
          error: {
            duration: 5000,
            iconTheme: {
              primary: '#ef4444',
              secondary: '#fff',
            },
          },
        }}
      />
    </div>
  );
}

export default App;