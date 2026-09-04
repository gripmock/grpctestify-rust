export function pollsWhile(visibility: DocumentVisibilityState): boolean {
  return visibility === 'visible';
}

export function mtimeMoved(known: number | undefined, seen: unknown): seen is number {
  return typeof seen === 'number' && seen !== known;
}

export const SYNC_EVERY = 10;

export function syncsAnyway(tick: number, every: number = SYNC_EVERY): boolean {
  return every > 0 && tick > 0 && tick % every === 0;
}
