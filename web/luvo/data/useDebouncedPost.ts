import { useEffect, useRef, useState } from 'react';

export function useDebouncedPost<T>(
  url: string,
  body: unknown | null,
  delay = 300,
): { data: T | null; error: string | null; busy: boolean } {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const seq = useRef(0);

  const key = body === null ? null : JSON.stringify(body);

  useEffect(() => {
    /* Nothing to ask is not "the last answer still stands": the panels share
       one hook across tabs, so a tab with nothing to send used to show the
       previous tab's answer until something else replaced it. */
    if (key === null) {
      seq.current++;
      setData(null);
      setError(null);
      setBusy(false);
      return;
    }
    const mySeq = ++seq.current;
    const controller = new AbortController();
    setBusy(true);
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
        if (said.ok) {
          setData(said.data);
          setError(null);
        } else {
          setError(said.reason);
          setData(null);
        }
      } catch (e: any) {
        if (e?.name !== 'AbortError' && mySeq === seq.current) setError(e?.message || String(e));
      } finally {
        if (mySeq === seq.current) setBusy(false);
      }
    }, delay);
    return () => { clearTimeout(timer); controller.abort(); };
  }, [url, key, delay]);

  return { data, error, busy };
}
