import type { CollectionItem, SectionAttribute } from './types';

export function attributeTags(attributes: SectionAttribute[]): string[] {
  const out: string[] = [];
  for (const a of attributes) {
    if (a.name !== 'tag') continue;
    for (const part of a.value.split(',')) {
      const tag = part.trim();
      if (tag !== '' && !out.includes(tag)) out.push(tag);
    }
  }
  return out;
}

export function tagsInUse(
  collections: CollectionItem[],
  mine: string[],
  path: string | null,
): { tag: string; files: number }[] {
  const counts = new Map<string, number>();
  for (const item of collections) {
    if (item.is_dir || item.path === path) continue;
    for (const tag of item.tags ?? []) {
      counts.set(tag, (counts.get(tag) ?? 0) + 1);
    }
  }
  return [...counts.entries()]
    .filter(([tag]) => !mine.includes(tag))
    .map(([tag, files]) => ({ tag, files }))
    .sort((a, b) => b.files - a.files || a.tag.localeCompare(b.tag));
}

export function attributeMeta(attributes: SectionAttribute[], name: 'owner' | 'summary'): string | null {
  for (const a of attributes) {
    if (a.name !== name) continue;
    const value = a.value.trim();
    if (value !== '') return value;
  }
  return null;
}
