let reachable = true;
const listeners = new Set<() => void>();

function tell(): void {
  for (const listener of listeners) listener();
}

export function noteUnreachable(): void {
  if (!reachable) return;
  reachable = false;
  tell();
}

export function noteReachable(): void {
  if (reachable) return;
  reachable = true;
  tell();
}

export function serverReachable(): boolean {
  return reachable;
}

export function subscribeReach(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function isNetworkFailure(err: unknown): boolean {
  return !(err instanceof DOMException && err.name === 'AbortError');
}
