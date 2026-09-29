import { useCallback, useState } from 'react';

const kept = new Map<string, unknown>();

export function keptValue<T>(key: string, initial: () => T): T {
  if (!kept.has(key)) kept.set(key, initial());
  return kept.get(key) as T;
}

export function keep<T>(key: string, value: T): void {
  kept.set(key, value);
}

export function forgetScratch(): void {
  kept.clear();
}

export function useKept<T>(
  key: string,
  initial: () => T,
): [T, (next: T | ((prev: T) => T)) => void] {
  const [value, setValue] = useState<T>(() => keptValue(key, initial));
  const put = useCallback(
    (next: T | ((prev: T) => T)) => {
      setValue(prev => {
        const resolved = typeof next === 'function' ? (next as (p: T) => T)(prev) : next;
        keep(key, resolved);
        return resolved;
      });
    },
    [key],
  );
  return [value, put];
}
