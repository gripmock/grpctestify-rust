import { useEffect, useRef, useState } from 'react';

interface Answer<T> {
  key: string;
  data: T | null;
  error: string | null;
}

export function useDebouncedPost<T>(
  url: string,
  body: unknown | null,
  delay = 300,
): { data: T | null; error: string | null; busy: boolean; stale: boolean } {
  const [answer, setAnswer] = useState<Answer<T> | null>(null);
  const seq = useRef(0);

  const key = body === null ? null : JSON.stringify(body);

  useEffect(() => {
    if (key === null) {
      seq.current++;
      return;
    }
    const mySeq = ++seq.current;
    const controller = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const res = await fetch(url, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: key,
          signal: controller.signal,
        });
        /* Read the whole answer first, then check whether it is still the one
           being waited for: the check used to happen before the body was
           parsed, so a slow answer that lost the race still landed on top of
           the newer one it lost to. */
        const said = res.ok
          ? { ok: true as const, data: (await res.json()) as T }
          : { ok: false as const, reason: await res.text().catch(() => `HTTP ${res.status}`) };
        if (mySeq !== seq.current) return;
        setAnswer(said.ok ? { key, data: said.data, error: null } : { key, data: null, error: said.reason });
      } catch (e: any) {
        if (e?.name !== 'AbortError' && mySeq === seq.current) {
          setAnswer({ key, data: null, error: e?.message || String(e) });
        }
      }
    }, delay);
    return () => { clearTimeout(timer); controller.abort(); };
  }, [url, key, delay]);

  const held = key === null ? null : answer;
  return {
    data: held?.data ?? null,
    error: held?.error ?? null,
    busy: key !== null && held?.key !== key,
    stale: held !== null && held.key !== key,
  };
}
