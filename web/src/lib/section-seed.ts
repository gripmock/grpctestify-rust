import type { RequestTab } from './types';

export function sectionSeed(key: RequestTab): Record<string, string> | null {
  if (key === 'bench') return { mode: 'fixed' };
  if (key === 'dataset') return {};
  return null;
}
