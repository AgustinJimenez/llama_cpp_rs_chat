import { useState, useEffect, useCallback } from 'react';
import { isTauriEnv } from '../utils/tauri';
import { getModelInfo, addModelHistory } from '../utils/tauriCommands';
import type { ModelMetadata } from '@/types';

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
// eslint-disable-next-line max-lines-per-function, complexity
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
      await addModelHistory(path);
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

    // eslint-disable-next-line complexity
    const checkFileExists = async () => {
      setIsCheckingFile(true);
      try {
        const trimmedPath = modelPath.trim();

        try {
          const data = await getModelInfo(trimmedPath);

          // Check if response indicates an error (directory, not found, etc.)
          if ((data as Record<string, unknown>).is_directory && (data as Record<string, unknown>).suggestions) {
            const errorData = data as Record<string, unknown>;
            setDirectoryError(errorData.error as string);
            setDirectorySuggestions(errorData.suggestions as string[]);
            setFileExists(false);

            // Auto-complete if there's only one .gguf file
            if ((errorData.suggestions as string[]).length === 1 && onPathChange) {
              const autoPath = trimmedPath.endsWith('\\') || trimmedPath.endsWith('/')
                ? `${trimmedPath}${(errorData.suggestions as string[])[0]}`
                : `${trimmedPath}\\${(errorData.suggestions as string[])[0]}`;
              onPathChange(autoPath);
            }
            return;
          }

          setFileExists(true);
          setDirectoryError(null);
          setDirectorySuggestions([]);

          // Save to history when file is validated
          await saveToHistory(trimmedPath);

          // Parse file_size_gb from file_size string (e.g., "11.65 GB" -> 11.65)
          let fileSizeGb: number | undefined;
          const fileSizeStr = (data as Record<string, unknown>).file_size;
          if (fileSizeStr && typeof fileSizeStr === 'string') {
            const match = fileSizeStr.match(/([\d.]+)\s*GB/i);
            if (match) {
              fileSizeGb = parseFloat(match[1]);
            }
          }

          const d = data as Record<string, unknown>;
          const meta = d.gguf_metadata as Record<string, string | number | boolean | null | undefined> | undefined;
          setModelInfo({
            name: (d.name as string) || trimmedPath.split(/[\\/]/).pop() || 'Unknown',
            architecture: (d.architecture as string) || 'Unknown',
            parameters: (d.parameters as string) || 'Unknown',
            quantization: (d.quantization as string) || 'Unknown',
            file_size: (d.file_size as string) || 'Unknown',
            file_size_gb: fileSizeGb,
            context_length: (d.context_length as string) || 'Unknown',
            file_path: trimmedPath,
            estimated_layers: d.estimated_layers as number | undefined,
            gguf_metadata: meta,
            default_system_prompt: d.default_system_prompt as string | undefined,
            general_name: d.general_name as string | undefined,
            recommended_params: d.recommended_params as ModelMetadata['recommended_params'],
            // Extract architecture details from top-level (parsed by backend) or raw GGUF metadata
            block_count: String(d.block_count ?? meta?.['gemma3.block_count'] ?? meta?.['llama.block_count'] ?? ''),
            attention_head_count: String(d.attention_head_count ?? meta?.['gemma3.attention.head_count'] ?? meta?.['llama.attention.head_count'] ?? ''),
            attention_head_count_kv: String(d.attention_head_count_kv ?? meta?.['gemma3.attention.head_count_kv'] ?? meta?.['llama.attention.head_count_kv'] ?? ''),
            embedding_length: String(d.embedding_length ?? meta?.['gemma3.embedding_length'] ?? meta?.['llama.embedding_length'] ?? ''),
          });

          // Update max layers if available
          if (d.estimated_layers) {
            setMaxLayers(d.estimated_layers as number);
          }
        } catch {
          setFileExists(false);
          setDirectoryError(null);
          setDirectorySuggestions([]);
          setModelInfo(null);
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
    isTauri: isTauriEnv(),
  };
};
