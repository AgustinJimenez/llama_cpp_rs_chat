import { useState, useEffect, useRef } from 'react';
import { X } from 'lucide-react';
import { Card } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
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

function App() {
  const { messages, isLoading, sendMessage, clearMessages, loadConversation, currentConversationId } = useChat();
  const { status: modelStatus, isLoading: isModelLoading, error: modelError, loadModel, unloadModel } = useModel();
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isSidebarOpen, setIsSidebarOpen] = useState(false);
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
      // Optionally show a success message or refresh something
      console.log('Model loaded successfully:', result.message);
    } else {
      // Optionally show an error message
      console.error('Failed to load model:', result.message);
    }
  };

  const handleModelUnload = async () => {
    const result = await unloadModel();
    if (result.success) {
      console.log('Model unloaded successfully:', result.message);
      // Clear any existing conversation when model is unloaded
      clearMessages();
    } else {
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
                    {modelStatus.model_path?.split('/').pop() || 'Model loaded'}
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
            <div className="w-10"></div> {/* Spacer for centering */}
          </div>

          {/* Messages */}
          <div className="flex-1 overflow-y-auto p-6 space-y-4" data-testid="messages-container">
            {messages.length === 0 ? (
              <WelcomeMessage modelLoaded={modelStatus.loaded} />
            ) : (
              <>
                {messages.map((message) => (
                  <MessageBubble key={message.id} message={message} />
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
    </div>
  );
}

export default App;