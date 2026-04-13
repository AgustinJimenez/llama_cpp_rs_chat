import { useState, useCallback, useRef } from 'react';

const DEFAULT_SEARCH_LIMIT = 20;
const SEARCH_DEBOUNCE_MS = 400;

import { searchHubModels, fetchHubTree } from '../utils/tauriCommands';
import type { HubModel, HubSortField } from '../utils/tauriCommands';

export type { HubModel, HubSortField };

/** Check if input looks like a HuggingFace repo ID (author/model) or URL */
function extractRepoId(input: string): string | null {
  const trimmed = input.trim();
  // URL: https://huggingface.co/unsloth/gemma-4-26B-A4B-it-GGUF
  const urlMatch = trimmed.match(/huggingface\.co\/([^/]+\/[^/\s?#]+)/);
  if (urlMatch) return urlMatch[1];
  // Direct repo ID: unsloth/gemma-4-26B-A4B-it-GGUF
  if (/^[a-zA-Z0-9_.-]+\/[a-zA-Z0-9_.-]+$/.test(trimmed)) return trimmed;
  return null;
}

export function useHubSearch() {
  const [models, setModels] = useState<HubModel[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sort, setSort] = useState<HubSortField>('downloads');
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const searchModels = useCallback(
    async (query: string, sortField?: HubSortField) => {
      setIsLoading(true);
      setError(null);
      try {
        // If input looks like a repo ID or HF URL, fetch tree directly
        const repoId = extractRepoId(query);
        if (repoId) {
          const files = await fetchHubTree(repoId);
          // Convert tree response to a single HubModel with files
          setModels([
            {
              id: repoId,
              author: repoId.split('/')[0] || '',
              downloads: 0,
              likes: 0,
              last_modified: '',
              pipeline_tag: '',
              files: files || [],
            },
          ]);
        } else {
          const results = await searchHubModels(
            query.trim() || 'GGUF',
            DEFAULT_SEARCH_LIMIT,
            sortField ?? sort,
          );
          setModels(results);
        }
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setError(msg || 'Search failed — check network connection');
        setModels([]);
      } finally {
        setIsLoading(false);
      }
    },
    [sort],
  );

  const debouncedSearch = useCallback(
    (query: string) => {
      if (timerRef.current) clearTimeout(timerRef.current);
      timerRef.current = setTimeout(() => searchModels(query), SEARCH_DEBOUNCE_MS);
    },
    [searchModels],
  );

  const changeSort = useCallback(
    (newSort: HubSortField, currentQuery: string) => {
      setSort(newSort);
      searchModels(currentQuery, newSort);
    },
    [searchModels],
  );

  return { models, isLoading, error, sort, searchModels, debouncedSearch, changeSort };
}
