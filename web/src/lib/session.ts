import { readText, writeText } from 'luvo/data/storage';

const KEY = 'grpctestify-session';

export function getSessionId(): string {
  const held = readText(KEY);
  if (held !== '') return held;
  const id = Math.random().toString(36).slice(2, 8);
  writeText(KEY, id);
  return id;
}
