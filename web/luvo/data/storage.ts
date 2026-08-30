/** Reading and writing the browser's own store, without trusting it.
 *
 *  A private window can refuse `localStorage` outright — the getter throws, and
 *  a component that reads one during render takes the whole workbench down with
 *  it. What comes back is not trustworthy either: these keys survive upgrades,
 *  a hand-edited value, and a half-written record from a tab that was closed
 *  mid-write. Every read here answers with the fallback rather than throwing,
 *  and numbers come back inside the range the caller can actually use. */

export function readText(key: string, fallback = ''): string {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}

export function writeText(key: string, value: string): boolean {
  try {
    localStorage.setItem(key, value);
    return true;
  } catch {
    /* Refused, or full. The caller keeps working without the memory. */
    return false;
  }
}

export function drop(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
    /* nothing else to try */
  }
}

/** Every key this browser holds, or nothing when it will not say.
 *
 *  A private window refuses the store outright, and a caller sweeping it has no
 *  business taking the page down for that. */
export function keys(): string[] {
  try {
    return Object.keys(localStorage);
  } catch {
    return [];
  }
}

/** A number inside the range that means something, whatever the store holds. */
export function readNumber(key: string, fallback: number, min: number, max: number): number {
  const raw = readText(key);
  const value = Number(raw);
  if (raw.trim() === '' || !Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, value));
}

export function readJson<T>(key: string, fallback: T): T {
  const raw = readText(key);
  if (raw === '') return fallback;
  try {
    const parsed = JSON.parse(raw) as T;
    return parsed === null || parsed === undefined ? fallback : parsed;
  } catch {
    return fallback;
  }
}

export function writeJson(key: string, value: unknown): boolean {
  try {
    return writeText(key, JSON.stringify(value));
  } catch {
    /* A value that will not serialise — a cycle — is not worth keeping. */
    return false;
  }
}
