import { findVariables } from './env';

export function pathPlaceholderNote(...values: (string | undefined | null)[]): string | null {
  const names = new Set<string>();
  for (const value of values) {
    for (const name of findVariables(value ?? '')) names.add(name);
  }
  if (names.size === 0) return null;
  const written = [...names].map(n => `{{${n}}}`).join(', ');
  return `${written} — used as written: this is a path, read from this file's directory, and nothing substitutes it.`;
}
