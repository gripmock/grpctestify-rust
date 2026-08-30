import { readJson, writeJson } from 'luvo/data/storage';

const LIMIT = 12;

export function pushRecent(list: string[], value: string): string[] {
  const v = value.trim();
  if (!v) return list;
  return [v, ...list.filter(x => x !== v)].slice(0, LIMIT);
}

export function loadRecent(key: string): string[] {
  const parsed = readJson<unknown>(key, []);
  if (!Array.isArray(parsed)) return [];
  return parsed.filter((x): x is string => typeof x === 'string').slice(0, LIMIT);
}

export function saveRecent(key: string, list: string[]): void {
  writeJson(key, list.slice(0, LIMIT));
}

export function sortByRecency<T extends { timestamp: number }>(entries: T[]): T[] {
  return entries.slice().sort((a, b) => b.timestamp - a.timestamp);
}
