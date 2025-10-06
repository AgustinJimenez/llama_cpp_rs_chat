import { useState, useEffect, useCallback } from 'react';
import type { SamplerConfig } from '../types';

interface ModelStatus {
  loaded: boolean;
  model_path: string | null;
  last_used: string | null;
  memory_usage_mb: number | null;
}

interface ModelResponse {
  success: boolean;
  message: string;
  status?: ModelStatus;
}

export const useModel = () => {
  const [status, setStatus] = useState<ModelStatus>({
    loaded: false,
    model_path: null,
    last_used: null,
    memory_usage_mb: null,
  });
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchStatus = useCallback(async () => {
    try {
      const response = await fetch('/api/model/status');
      if (response.ok) {
        const data = await response.json();
        setStatus(data);
        setError(null);
      } else {
        setError('Failed to fetch model status');
      }
    } catch (err) {
      setError('Network error while fetching model status');
      console.error('Model status fetch error:', err);
    }
  }, []);

  const loadModel = useCallback(async (modelPath: string, config?: SamplerConfig) => {
    setIsLoading(true);
    setError(null);
    
    try {
      // First update the configuration if provided
      if (config) {
        const configResponse = await fetch('/api/config', {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
          },
          body: JSON.stringify({
            ...config,
            model_path: modelPath,
          }),
        });
        
        if (!configResponse.ok) {
          throw new Error('Failed to update configuration');
        }
      }

      // Then load the model
      const response = await fetch('/api/model/load', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          model_path: modelPath,
        }),
      });

      const data: ModelResponse = await response.json();
      
      if (data.success && data.status) {
        setStatus(data.status);
        setError(null);
        return { success: true, message: data.message };
      } else {
        setError(data.message);
        // Refresh status even when loading fails to ensure UI is accurate
        await fetchStatus();
        return { success: false, message: data.message };
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Unknown error occurred';
      setError(errorMessage);
      // Refresh status on error to ensure UI is accurate
      await fetchStatus();
      return { success: false, message: errorMessage };
    } finally {
      setIsLoading(false);
    }
  }, []);

  const unloadModel = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    
    try {
      const response = await fetch('/api/model/unload', {
        method: 'POST',
      });

      const data: ModelResponse = await response.json();
      
      if (data.success && data.status) {
        setStatus(data.status);
        setError(null);
        return { success: true, message: data.message };
      } else {
        setError(data.message);
        return { success: false, message: data.message };
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Unknown error occurred';
      setError(errorMessage);
      // Refresh status on error to ensure UI is accurate
      await fetchStatus();
      return { success: false, message: errorMessage };
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Fetch status on mount
  useEffect(() => {
    fetchStatus();
  }, [fetchStatus]);

  return {
    status,
    isLoading,
    error,
    loadModel,
    unloadModel,
    refreshStatus: fetchStatus,
  };
};