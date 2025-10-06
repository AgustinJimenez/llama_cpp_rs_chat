import React, { useState, useEffect } from 'react';
import { Menu, ChevronLeft, Plus, Settings, RotateCcw } from 'lucide-react';
import './Sidebar.css';

interface ConversationFile {
  name: string;
  displayName: string;
  timestamp: string;
}

interface SidebarProps {
  isOpen: boolean;
  onToggle: () => void;
  onNewChat: () => void;
  onOpenSettings: () => void;
  onLoadConversation: (filename: string) => void;
  currentConversationId?: string | null;
}

const Sidebar: React.FC<SidebarProps> = ({ isOpen, onToggle, onNewChat, onOpenSettings, onLoadConversation, currentConversationId }) => {
  const [conversations, setConversations] = useState<ConversationFile[]>([]);
  const [loading, setLoading] = useState(false);

  const fetchConversations = async () => {
    setLoading(true);
    try {
      const response = await fetch('/api/conversations');
      if (response.ok) {
        const data = await response.json();
        setConversations(data.conversations || []);
      } else {
        console.error('Failed to fetch conversations');
      }
    } catch (error) {
      console.error('Error fetching conversations:', error);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (isOpen) {
      fetchConversations();
    }
  }, [isOpen]);

  const formatTimestamp = (timestamp: string) => {
    // Parse timestamp format: YYYY-MM-DD-HH-mm-ss-SSS
    const parts = timestamp.split('-');
    if (parts.length >= 6) {
      const [year, month, day, hour, minute] = parts;
      return `${month}/${day}/${year} ${hour}:${minute}`;
    }
    return timestamp;
  };

  return (
    <>
      {/* Overlay for mobile */}
      {isOpen && (
        <div 
          className="sidebar-overlay" 
          onClick={onToggle}
          data-testid="sidebar-overlay"
        />
      )}
      
      {/* Sidebar */}
      <div 
        className={`sidebar ${isOpen ? 'sidebar-open' : 'sidebar-closed'}`}
        data-testid="sidebar"
      >
        {/* Header */}
        <div className="sidebar-header">
          <button 
            className="sidebar-toggle-btn"
            onClick={onToggle}
            data-testid="sidebar-toggle"
            aria-label={isOpen ? 'Close sidebar' : 'Open sidebar'}
          >
{isOpen ? <ChevronLeft size={20} /> : <Menu size={20} />}
          </button>
          {isOpen && <h2 className="sidebar-title">LLaMA Chat</h2>}
        </div>

        {/* Action Buttons */}
        {isOpen && (
          <div className="sidebar-actions">
            <button 
              className="sidebar-btn new-chat-btn"
              onClick={onNewChat}
              data-testid="new-chat-btn"
            >
              <Plus size={16} />
              New Chat
            </button>
            <button 
              className="sidebar-btn settings-btn"
              onClick={onOpenSettings}
              data-testid="settings-btn"
            >
              <Settings size={16} />
              Settings
            </button>
          </div>
        )}

        {/* Conversations List */}
        {isOpen && (
          <div className="conversations-section">
            <div className="conversations-header">
              <h3>Recent Conversations</h3>
              <button 
                className="refresh-btn"
                onClick={fetchConversations}
                disabled={loading}
                data-testid="refresh-conversations"
                aria-label="Refresh conversations"
              >
                <RotateCcw size={16} className={loading ? 'animate-spin' : ''} />
              </button>
            </div>
            
            <div className="conversations-list" data-testid="conversations-list">
              {loading ? (
                <div className="conversations-loading">Loading...</div>
              ) : conversations.length === 0 ? (
                <div className="conversations-empty">No conversations yet</div>
              ) : (
                conversations.map((conversation, index) => {
                  const isActive = currentConversationId === conversation.name;
                  return (
                    <div 
                      key={conversation.name}
                      className={`conversation-item ${isActive ? 'conversation-item-active' : ''}`}
                      onClick={() => onLoadConversation(conversation.name)}
                      data-testid={`conversation-${index}`}
                    >
                      <div className="conversation-title">
                        Chat {formatTimestamp(conversation.timestamp)}
                      </div>
                      <div className="conversation-filename">
                        {conversation.name}
                      </div>
                    </div>
                  );
                })
              )}
            </div>
          </div>
        )}

        {/* Collapsed state indicator */}
        {!isOpen && (
          <div className="sidebar-collapsed-content">
            <button 
              className="sidebar-icon-btn"
              onClick={onNewChat}
              data-testid="collapsed-new-chat"
              aria-label="New chat"
            >
              <Plus size={20} />
            </button>
            <button 
              className="sidebar-icon-btn"
              onClick={onOpenSettings}
              data-testid="collapsed-settings"
              aria-label="Settings"
            >
              <Settings size={20} />
            </button>
          </div>
        )}
      </div>
    </>
  );
};

export default Sidebar;