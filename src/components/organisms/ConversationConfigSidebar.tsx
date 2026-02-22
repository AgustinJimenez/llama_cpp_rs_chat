import { useState, useEffect, useCallback } from 'react';
import { X, Save } from 'lucide-react';
import { toast } from 'react-hot-toast';
import { SamplingParametersSection } from './model-config/SamplingParametersSection';
import { getConversationConfig, saveConversationConfig } from '../../utils/tauriCommands';
import type { SamplerConfig } from '../../types';

interface ConversationConfigSidebarProps {
  isOpen: boolean;
  onClose: () => void;
  conversationId: string | null;
}

export function ConversationConfigSidebar({
  isOpen,
  onClose,
  conversationId,
}: ConversationConfigSidebarProps) {
  const [localConfig, setLocalConfig] = useState<SamplerConfig | null>(null);
  const [isSaving, setIsSaving] = useState(false);

  // Load config when sidebar opens or conversation changes
  useEffect(() => {
    if (!isOpen || !conversationId) {
      setLocalConfig(null);
      return;
    }
    getConversationConfig(conversationId)
      .then(setLocalConfig)
      .catch(() => {
        toast.error('Failed to load conversation config');
      });
  }, [isOpen, conversationId]);

  const handleConfigChange = useCallback(
    (field: keyof SamplerConfig, value: string | number | boolean) => {
      setLocalConfig((prev) =>
        prev ? { ...prev, [field]: value } : prev
      );
    },
    []
  );

  const handleSave = async () => {
    if (!localConfig || !conversationId) return;
    setIsSaving(true);
    try {
      await saveConversationConfig(conversationId, localConfig);
      toast.success('Config saved');
    } catch {
      toast.error('Failed to save config');
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <>
      {/* Mobile overlay */}
      {isOpen && (
        <div
          className="fixed inset-0 bg-black/50 z-40 md:hidden"
          role="button"
          tabIndex={0}
          onClick={onClose}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') onClose();
          }}
        />
      )}

      {/* Sidebar panel */}
      <div
        className={`fixed top-0 right-0 h-full bg-card border-l border-border z-50 transition-transform duration-300 flex flex-col ${
          isOpen ? 'translate-x-0' : 'translate-x-full'
        }`}
        style={{ width: '360px' }}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
          <h2 className="text-lg font-semibold">Conversation Config</h2>
          <button
            onClick={onClose}
            className="p-2 hover:bg-muted rounded-lg transition-colors"
            title="Close sidebar"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Scrollable content */}
        <div className="flex-1 overflow-y-auto p-4">
          {!conversationId && (
            <p className="text-sm text-muted-foreground">
              No conversation selected
            </p>
          )}

          {conversationId && !localConfig && (
            <p className="text-sm text-muted-foreground">Loading...</p>
          )}

          {conversationId && localConfig && (
            <SamplingParametersSection
              config={localConfig}
              onConfigChange={handleConfigChange}
            />
          )}
        </div>

        {/* Footer with Save button */}
        {conversationId && localConfig && (
          <div className="shrink-0 px-4 py-3 border-t border-border">
            <button
              onClick={handleSave}
              disabled={isSaving}
              className="w-full flex items-center justify-center gap-2 px-4 py-2 bg-primary text-primary-foreground rounded-md hover:bg-primary/90 transition-colors disabled:opacity-50 text-sm font-medium"
            >
              <Save className="h-4 w-4" />
              {isSaving ? 'Saving...' : 'Save Config'}
            </button>
          </div>
        )}
      </div>
    </>
  );
}
