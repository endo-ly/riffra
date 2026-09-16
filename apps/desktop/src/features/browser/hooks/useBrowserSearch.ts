import { useCallback, useMemo, useState } from 'react';

/** Owns the single query shared by every Browser section. */
export function useBrowserSearch(initialQuery = '') {
  const [query, setQueryState] = useState(initialQuery);
  const setQuery = useCallback((value: string) => setQueryState(value), []);
  const normalizedQuery = useMemo(() => query.trim().toLocaleLowerCase(), [query]);

  return { query, setQuery, normalizedQuery };
}
