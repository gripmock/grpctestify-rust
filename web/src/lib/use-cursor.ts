import { useCallback, useState } from 'react';

export function cursorAt(held: { key: string; at: number } | null, key: string, count: number): number {
  if (held === null || held.key !== key) return 0;
  if (count === 0) return 0;
  return Math.min(Math.max(0, held.at), count - 1);
}

export function useKeyedCursor(key: string, count: number): [number, (at: number) => void, (step: number) => void] {
  const [held, setHeld] = useState<{ key: string; at: number } | null>(null);
  const at = cursorAt(held, key, count);
  const put = useCallback((next: number) => setHeld({ key, at: next }), [key]);
  const step = useCallback(
    (by: number) => setHeld(current => ({ key, at: count === 0 ? 0 : (cursorAt(current, key, count) + by + count) % count })),
    [key, count],
  );
  return [at, put, step];
}
