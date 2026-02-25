import { useState, useCallback, useRef } from 'react';
import { searchHubModels } from '../utils/tauriCommands';
import type { HubModel, HubSortField } from '../utils/tauriCommands';

export type { HubModel, HubSortField };

export function useHubSearch() {
  const [models, setModels] = useState<HubModel[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sort, setSort] = useState<HubSortField>('downloads');
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const searchModels = useCallback(async (query: string, sortField?: HubSortField) => {
    setIsLoading(true);
    setError(null);
    try {
      const results = await searchHubModels(query.trim() || 'GGUF', 20, sortField ?? sort);
      setModels(results);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Search failed');
      setModels([]);
    } finally {
      setIsLoading(false);
    }
  }, [sort]);

  const debouncedSearch = useCallback((query: string) => {
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => searchModels(query), 400);
  }, [searchModels]);

  const changeSort = useCallback((newSort: HubSortField, currentQuery: string) => {
    setSort(newSort);
    searchModels(currentQuery, newSort);
  }, [searchModels]);

  return { models, isLoading, error, sort, searchModels, debouncedSearch, changeSort };
}
