import { useState, useCallback, useEffect } from 'react';
import { TauriAPI } from '../utils/tauri';
import type { SamplerConfig } from '../types';

export function useSettings() {
  const [config, setConfig] = useState<SamplerConfig | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadConfig = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    
    try {
      const samplerConfig = await TauriAPI.getSamplerConfig();
      setConfig(samplerConfig);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to load configuration';
      setError(errorMessage);
    } finally {
      setIsLoading(false);
    }
  }, []);

  const updateConfig = useCallback(async (newConfig: SamplerConfig) => {
    setIsLoading(true);
    setError(null);
    
    try {
      await TauriAPI.updateSamplerConfig(newConfig);
      setConfig(newConfig);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to update configuration';
      setError(errorMessage);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  return {
    config,
    isLoading,
    error,
    updateConfig,
    reloadConfig: loadConfig,
  };
}