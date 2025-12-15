import { useState, useEffect, useCallback } from 'react';
import type { ModelMetadata } from '@/types';

// Check if we're running in Tauri environment
const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

interface UseModelPathValidationOptions {
  /** The model path to validate */
  modelPath: string;
  /** Callback when path should be auto-completed */
  onPathChange?: (newPath: string) => void;
  /** Debounce delay in milliseconds (default: 500) */
  debounceMs?: number;
}

interface UseModelPathValidationResult {
  /** Whether the file exists at the given path */
  fileExists: boolean | null;
  /** Whether we're currently checking the file */
  isCheckingFile: boolean;
  /** Directory error message if path is a directory */
  directoryError: string | null;
  /** Suggested .gguf files if path is a directory */
  directorySuggestions: string[];
  /** Model metadata if file exists and is valid */
  modelInfo: ModelMetadata | null;
  /** Maximum GPU layers based on model architecture */
  maxLayers: number;
  /** Whether it's running in Tauri environment */
  isTauri: boolean;
}

/**
 * Custom hook for validating model file paths and fetching metadata.
 *
 * This hook handles:
 * - Debounced file existence checking
 * - Model metadata fetching via Tauri or HTTP API
 * - Directory detection with .gguf file suggestions
 * - Auto-completion when only one .gguf file is found
 * - Saving validated paths to history
 */
export const useModelPathValidation = ({
  modelPath,
  onPathChange,
  debounceMs = 500,
}: UseModelPathValidationOptions): UseModelPathValidationResult => {
  const [fileExists, setFileExists] = useState<boolean | null>(null);
  const [isCheckingFile, setIsCheckingFile] = useState(false);
  const [directorySuggestions, setDirectorySuggestions] = useState<string[]>([]);
  const [directoryError, setDirectoryError] = useState<string | null>(null);
  const [modelInfo, setModelInfo] = useState<ModelMetadata | null>(null);
  const [maxLayers, setMaxLayers] = useState(99);

  // Helper function to save model path to history
  const saveToHistory = useCallback(async (path: string) => {
    try {
      await fetch('/api/model/history', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ model_path: path }),
      });
    } catch (error) {
      console.error('Failed to save model path to history:', error);
    }
  }, []);

  // Debounced file existence check
  useEffect(() => {
    if (!modelPath.trim()) {
      setFileExists(null);
      setModelInfo(null);
      setDirectoryError(null);
      setDirectorySuggestions([]);
      return;
    }

    const checkFileExists = async () => {
      setIsCheckingFile(true);
      try {
        if (isTauri) {
          // For Tauri, use the filesystem API via invoke
          const { invoke } = await import('@tauri-apps/api/core');
          try {
            const metadata = await invoke<ModelMetadata>('get_model_metadata', { modelPath });
            setFileExists(true);
            setDirectoryError(null);
            setDirectorySuggestions([]);

            // Save to history when file is validated
            await saveToHistory(modelPath);

            // Set model metadata
            console.log('[DEBUG] Tauri metadata received:', metadata);
            setModelInfo(metadata);

            // Update max layers if available
            if (metadata.estimated_layers) {
              setMaxLayers(metadata.estimated_layers);
            }
          } catch {
            setFileExists(false);
            setModelInfo(null);
          }
        } else {
          // For web, make GET request to check if file exists on server
          const trimmedPath = modelPath.trim();
          const encodedPath = encodeURIComponent(trimmedPath);

          try {
            const response = await fetch(`/api/model/info?path=${encodedPath}`);

            if (response.ok) {
              const data = await response.json();
              setFileExists(true);
              setDirectoryError(null);
              setDirectorySuggestions([]);

              // Save to history when file is validated
              await saveToHistory(trimmedPath);

              // Parse file_size_gb from file_size string (e.g., "11.65 GB" -> 11.65)
              let fileSizeGb: number | undefined;
              if (data.file_size && typeof data.file_size === 'string') {
                const match = data.file_size.match(/([\d.]+)\s*GB/i);
                if (match) {
                  fileSizeGb = parseFloat(match[1]);
                }
              }

              setModelInfo({
                name: data.name || trimmedPath.split(/[\\/]/).pop() || 'Unknown',
                architecture: data.architecture || 'Unknown',
                parameters: data.parameters || 'Unknown',
                quantization: data.quantization || 'Unknown',
                file_size: data.file_size || 'Unknown',
                file_size_gb: fileSizeGb,
                context_length: data.context_length || 'Unknown',
                file_path: trimmedPath,
                estimated_layers: data.estimated_layers,
                gguf_metadata: data.gguf_metadata,
                default_system_prompt: data.default_system_prompt,
                // Extract architecture details if available
                block_count: data.gguf_metadata?.['gemma3.block_count'] ||
                            data.gguf_metadata?.['llama.block_count'],
                attention_head_count_kv: data.gguf_metadata?.['gemma3.attention.head_count_kv'] ||
                                         data.gguf_metadata?.['llama.attention.head_count_kv'],
                embedding_length: data.gguf_metadata?.['gemma3.embedding_length'] ||
                                 data.gguf_metadata?.['llama.embedding_length'],
              });

              // Update max layers if available
              if (data.estimated_layers) {
                setMaxLayers(data.estimated_layers);
              }
            } else {
              // Check if it's a directory error with suggestions
              const errorData = await response.json();

              if (errorData.is_directory && errorData.suggestions) {
                setDirectoryError(errorData.error);
                setDirectorySuggestions(errorData.suggestions);
                setFileExists(false);

                // Auto-complete if there's only one .gguf file
                if (errorData.suggestions.length === 1 && onPathChange) {
                  const autoPath = trimmedPath.endsWith('\\') || trimmedPath.endsWith('/')
                    ? `${trimmedPath}${errorData.suggestions[0]}`
                    : `${trimmedPath}\\${errorData.suggestions[0]}`;
                  onPathChange(autoPath);
                }
              } else {
                setFileExists(false);
                setDirectoryError(null);
                setDirectorySuggestions([]);
                setModelInfo(null);
              }
            }
          } catch (error) {
            console.error('[DEBUG] Error checking file:', error);
            setFileExists(false);
            setDirectoryError(null);
            setDirectorySuggestions([]);
            setModelInfo(null);
          }
        }
      } catch {
        setFileExists(false);
        setModelInfo(null);
      } finally {
        setIsCheckingFile(false);
      }
    };

    // Debounce the check
    const timeoutId = setTimeout(checkFileExists, debounceMs);
    return () => clearTimeout(timeoutId);
  }, [modelPath, debounceMs, onPathChange, saveToHistory]);

  return {
    fileExists,
    isCheckingFile,
    directoryError,
    directorySuggestions,
    modelInfo,
    maxLayers,
    isTauri,
  };
};
