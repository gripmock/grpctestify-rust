import { drop, keys } from 'luvo/data/storage';

export function isViewState(key: string): boolean {
  return key === 'grpctestify-tabs' || key.startsWith('play.');
}

export function resetViewState(): string[] {
  const dropped = keys().filter(isViewState);
  for (const key of dropped) drop(key);
  return dropped;
}
