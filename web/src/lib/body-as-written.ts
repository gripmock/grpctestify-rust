import type { CollectionParsed } from './types';

export interface AsWritten {
  text: string;
  kind: 'json5' | 'layout';
}

export function bodyAsWritten(
  parsed: CollectionParsed | null,
  index: number,
  shown: string,
): AsWritten | null {
  const text = parsed?.bodies_as_written?.[index]?.trim();
  if (!text || text === shown.trim()) return null;
  return { text, kind: isStrictJson(text) ? 'layout' : 'json5' };
}

function isStrictJson(text: string): boolean {
  try {
    JSON.parse(text);
    return true;
  } catch {
    return false;
  }
}

export function sectionAsWritten(parsed: CollectionParsed | null, section: string): string | null {
  return parsed?.sections_as_written?.[section]?.trim() || null;
}
