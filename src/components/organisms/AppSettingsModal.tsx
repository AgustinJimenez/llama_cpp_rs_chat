import React, { useState, useEffect } from 'react';
import { Settings } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogDescription,
  DialogTitle,
} from '../atoms/dialog';
import { useSettings } from '../../hooks/useSettings';
import { toast } from 'react-hot-toast';
import type { SamplerConfig } from '../../types';

interface AppSettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export const AppSettingsModal: React.FC<AppSettingsModalProps> = ({ isOpen, onClose }) => {
  const { config, updateConfig } = useSettings();
  const [localConfig, setLocalConfig] = useState<SamplerConfig | null>(null);

  useEffect(() => {
    if (config) setLocalConfig(config);
  }, [config]);

  const handleSave = async () => {
    if (localConfig) {
      await updateConfig(localConfig);
      toast.success('Settings saved');
      onClose();
    }
  };

  const provider = localConfig?.web_search_provider || 'DuckDuckGo';
  const apiKey = localConfig?.web_search_api_key || '';

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Settings className="h-5 w-5" />
            App Settings
          </DialogTitle>
          <DialogDescription className="sr-only">
            Application settings
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* Web Search Provider */}
          <div className="space-y-2">
            <label htmlFor="web-search-provider" className="text-sm font-medium text-foreground">
              Web Search Provider
            </label>
            <p className="text-xs text-muted-foreground">
              Choose which search engine the model uses for web_search tool calls.
            </p>
            <select
              id="web-search-provider"
              className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm text-foreground"
              value={provider}
              onChange={(e) =>
                setLocalConfig(prev =>
                  prev ? { ...prev, web_search_provider: e.target.value } : prev
                )
              }
            >
              <option value="DuckDuckGo">DuckDuckGo (API + HTML scraping)</option>
              <option value="Brave">Brave (API key required)</option>
              <option value="Google">Google (via headless Chrome)</option>
            </select>
          </div>

          {provider === 'Brave' ? (
            <div className="space-y-2">
              <label htmlFor="brave-api-key" className="text-sm font-medium text-foreground">
                Brave API Key
              </label>
              <p className="text-xs text-muted-foreground">
                Stored in your local database and used only for Brave web_search.
              </p>
              <input
                id="brave-api-key"
                type="password"
                autoComplete="off"
                className="w-full px-3 py-2 rounded-lg bg-muted border border-border text-sm text-foreground"
                value={apiKey}
                placeholder="BRAVE_SEARCH_API_KEY"
                onChange={(e) =>
                  setLocalConfig(prev =>
                    prev ? { ...prev, web_search_api_key: e.target.value } : prev
                  )
                }
              />
            </div>
          ) : null}
        </div>

        <DialogFooter>
          <button className="flat-button bg-muted px-6 py-2" onClick={onClose}>
            Cancel
          </button>
          <button
            className="flat-button bg-primary text-white px-6 py-2"
            onClick={handleSave}
          >
            Save
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
